fn main() {
    tauri_build::build();

    // Tauri copies macOS native libraries into Contents/Frameworks.
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
}
