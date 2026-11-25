//! OAuth 2.0 for installed apps: loopback redirect + PKCE, refresh-token persistence.

use crate::config::{ClientCreds, write_private};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/drive";

#[derive(Clone, Serialize, Deserialize)]
struct StoredToken {
    refresh_token: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_at: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

pub struct Auth {
    creds: ClientCreds,
    http: reqwest::blocking::Client,
    token_path: PathBuf,
    state: Mutex<StoredToken>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_string(n: usize) -> String {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).expect("OS RNG unavailable");
    b64(&buf)
}

impl Auth {
    /// Load a previously saved refresh token, if any.
    pub fn load(creds: ClientCreds, http: reqwest::blocking::Client, token_path: PathBuf) -> Option<Auth> {
        let text = std::fs::read_to_string(&token_path).ok()?;
        let tok: StoredToken = serde_json::from_str(&text).ok()?;
        if tok.refresh_token.is_empty() {
            return None;
        }
        Some(Auth { creds, http, token_path, state: Mutex::new(tok) })
    }

    /// Interactive login: opens the system browser and waits for the redirect on a loopback port.
    pub fn login(creds: ClientCreds, http: reqwest::blocking::Client, token_path: PathBuf) -> Result<Auth> {
        let listener = TcpListener::bind("127.0.0.1:0").context("binding loopback port")?;
        let port = listener.local_addr()?.port();
        let redirect = format!("http://127.0.0.1:{port}");
        let verifier = random_string(48);
        let challenge = b64(&sha2::Sha256::digest(verifier.as_bytes()));
        let state = random_string(16);

        let mut url = reqwest::Url::parse(AUTH_URL)?;
        url.query_pairs_mut()
            .append_pair("client_id", &creds.client_id)
            .append_pair("redirect_uri", &redirect)
            .append_pair("response_type", "code")
            .append_pair("scope", SCOPE)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .append_pair("state", &state);
        open::that_detached(url.as_str()).context("opening browser")?;

        let code = wait_for_code(&listener, &state)?;

        let resp = http
            .post(TOKEN_URL)
            .form(&[
                ("code", code.as_str()),
                ("client_id", creds.client_id.as_str()),
                ("client_secret", creds.client_secret.as_str()),
                ("redirect_uri", redirect.as_str()),
                ("grant_type", "authorization_code"),
                ("code_verifier", verifier.as_str()),
            ])
            .send()?;
        if !resp.status().is_success() {
            bail!("token exchange failed: {}", resp.text().unwrap_or_default());
        }
        let tr: TokenResponse = resp.json()?;
        let refresh_token = tr
            .refresh_token
            .ok_or_else(|| anyhow!("Google did not return a refresh token"))?;
        let tok = StoredToken {
            refresh_token,
            access_token: tr.access_token,
            expires_at: now() + tr.expires_in,
        };
        write_private(&token_path, &serde_json::to_vec(&tok)?)?;
        Ok(Auth { creds, http, token_path, state: Mutex::new(tok) })
    }

    /// A valid access token, refreshing it if it expires within a minute.
    pub fn access_token(&self) -> Result<String> {
        let mut st = self.state.lock().unwrap();
        if st.access_token.is_empty() || st.expires_at <= now() + 60 {
            self.refresh_locked(&mut st)?;
        }
        Ok(st.access_token.clone())
    }

    /// Called after a 401: drop the current access token so the next call refreshes.
    pub fn invalidate(&self, token: &str) {
        let mut st = self.state.lock().unwrap();
        if st.access_token == token {
            st.access_token.clear();
        }
    }

    fn refresh_locked(&self, st: &mut StoredToken) -> Result<()> {
        let resp = self
            .http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.creds.client_id.as_str()),
                ("client_secret", self.creds.client_secret.as_str()),
                ("refresh_token", st.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .context("refreshing access token")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            if body.contains("invalid_grant") {
                bail!(AuthRevoked);
            }
            bail!("token refresh failed ({status}): {body}");
        }
        let tr: TokenResponse = resp.json()?;
        st.access_token = tr.access_token;
        st.expires_at = now() + tr.expires_in;
        if let Some(rt) = tr.refresh_token {
            st.refresh_token = rt;
        }
        let _ = write_private(&self.token_path, &serde_json::to_vec(&*st)?);
        Ok(())
    }
}

/// Error marker: the refresh token is no longer valid and the user must sign in again.
#[derive(Debug)]
pub struct AuthRevoked;
impl std::fmt::Display for AuthRevoked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sign-in expired or was revoked; please sign in again")
    }
}
impl std::error::Error for AuthRevoked {}

fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    listener.set_nonblocking(false)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    loop {
        if std::time::Instant::now() > deadline {
            bail!("timed out waiting for browser sign-in");
        }
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        // "GET /?code=...&state=... HTTP/1.1"
        let target = line.split_whitespace().nth(1).unwrap_or("/");
        let url = reqwest::Url::parse(&format!("http://localhost{target}"))?;
        let mut code = None;
        let mut state = None;
        let mut error = None;
        for (k, v) in url.query_pairs() {
            match &*k {
                "code" => code = Some(v.into_owned()),
                "state" => state = Some(v.into_owned()),
                "error" => error = Some(v.into_owned()),
                _ => {}
            }
        }
        if code.is_none() && error.is_none() {
            // favicon or similar
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            continue;
        }
        let ok = error.is_none() && state.as_deref() == Some(expected_state);
        let body = if ok {
            "<html><body style='font-family:sans-serif;padding:3em'><h2>Signed in to Dray</h2>You can close this tab.</body></html>"
        } else {
            "<html><body style='font-family:sans-serif;padding:3em'><h2>Sign-in failed</h2>Return to the app and try again.</body></html>"
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        if let Some(e) = error {
            bail!("sign-in was denied: {e}");
        }
        if state.as_deref() != Some(expected_state) {
            bail!("sign-in state mismatch");
        }
        return Ok(code.unwrap());
    }
}
