fn main() {
    // Embed the application icon into the Windows .exe.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=packaging/windows.rc");
        println!("cargo:rerun-if-changed=packaging/dray.ico");
        embed_resource::compile("packaging/windows.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
