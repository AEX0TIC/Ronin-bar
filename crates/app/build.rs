//! Embeds the Ronin logo as the program's icon when building on Windows for Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/ronin.rc");
    println!("cargo:rerun-if-changed=assets/ronin.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/ronin.rc", embed_resource::NONE).manifest_optional().expect("embed the icon");
    }
}
