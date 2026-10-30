// Embeds an application manifest: Common Controls v6 (modern dialogs, needed for TaskDialogIndirect),
// per-monitor DPI awareness (sharp, not stretched, on scaled displays) and the UTF-8 code page.
// Also embeds the app icon (assets/icon.ico, drawn by assets/make_icon.py) and version info.
fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("phantomlink-sensors"))
            .expect("unable to embed the application manifest");

        let v = |part| std::env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap();
        let defines = [
            format!("VER_MAJOR={}", v("MAJOR")),
            format!("VER_MINOR={}", v("MINOR")),
            format!("VER_PATCH={}", v("PATCH")),
            format!("VER_STRING=\"{}\"", std::env::var("CARGO_PKG_VERSION").unwrap()),
        ];
        embed_resource::compile("assets/app.rc", &defines)
            .manifest_optional()
            .expect("unable to embed the icon and version info");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
