fn main() {
    tauri_build::build();

    // Bundled FFmpeg dylibs live in the app's Resources directory.
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Resources");
}
