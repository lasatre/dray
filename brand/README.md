# Dray brand

The rules and source values for Dray's look and voice.

| Path | What it is |
|---|---|
| `BRAND.md` | Voice, color, type, shape, motion and icon rules. Read this first. |
| `tokens.json` | Source of truth for every color, type, spacing, radius and shadow value. |
| `tokens.css` | The same tokens as CSS variables (light and dark), used by the previews. |
| `logo/` | The mark in three inks and the app icon (SVG, plus PNGs from 16 to 1024 px). `logo/README.md` says which file goes on which ground. |
| `reference/components/` | Visual specs for Button, FileRow, Status, Logo and the cover art. Open a `preview.html` in a browser; set `data-theme="dark"` on its `<html>` tag for the dark theme. |

In the app, `src/theme.rs` holds the same values as Rust constants and maps them onto egui. A
test checks it against `tokens.json`, so change `tokens.json` first, then `theme.rs`.
`src/widgets.rs` draws the reference components.
