// Embeds an application manifest: Common Controls v6 (modern dialogs, needed for TaskDialogIndirect),
// per-monitor DPI awareness (sharp, not stretched, on scaled displays) and the UTF-8 code page.
// Also embeds the app icon (assets/icon.ico, drawn by assets/make_icon.py).
fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("phantomlink-sensors"))
            .expect("unable to embed the application manifest");
        embed_resource::compile("assets/icon.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("unable to embed the icon");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
