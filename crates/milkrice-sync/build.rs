fn main() {
    // milkrice-core links ScreenCaptureKit's Swift runtime; binaries need the system Swift rpath.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,/usr/lib/swift");
        println!("cargo:rustc-link-arg-tests=-Wl,-rpath,/usr/lib/swift");
    }
}
