fn main() {
    // Link MediaPlayer framework for MPRemoteCommandCenter (media key support)
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-lib=framework=MediaPlayer");

    tauri_build::build()
}
