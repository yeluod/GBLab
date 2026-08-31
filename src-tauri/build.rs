fn main() {
    for icon in [
        "icons/app-icon.svg",
        "icons/32x32.png",
        "icons/64x64.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.png",
        "icons/icon.icns",
        "icons/icon.ico",
        "icons/StoreLogo.png",
        "icons/Square30x30Logo.png",
        "icons/Square44x44Logo.png",
        "icons/Square71x71Logo.png",
        "icons/Square89x89Logo.png",
        "icons/Square107x107Logo.png",
        "icons/Square142x142Logo.png",
        "icons/Square150x150Logo.png",
        "icons/Square284x284Logo.png",
        "icons/Square310x310Logo.png",
    ] {
        println!("cargo:rerun-if-changed={icon}");
    }

    tauri_build::build();
}
