//! Embeds an Info.plist into example binaries on macOS so TCC (calendar) prompts work
//! for unbundled dev runs. Only affects `cargo build --examples`.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        println!("cargo:rerun-if-changed=examples/Info.plist");
        println!("cargo:rustc-link-arg-examples=-Wl,-rpath,/usr/lib/swift");
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,/usr/lib/swift");
        println!(
            "cargo:rustc-link-arg-examples=-Wl,-sectcreate,__TEXT,__info_plist,{dir}/examples/Info.plist"
        );
    }
}
