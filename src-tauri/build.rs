fn main() {
    // translation-rs links Swift's concurrency runtime but its own rpath does
    // not propagate to dependents, so the app binary must add it or dyld fails
    // at launch with "Library not loaded: @rpath/libswift_Concurrency.dylib".
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    tauri_build::build()
}
