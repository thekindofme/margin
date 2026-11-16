fn main() {
    // ScreenCaptureKit bindings link the Swift concurrency runtime, which lives in /usr/lib/swift.
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    tauri_build::build()
}
