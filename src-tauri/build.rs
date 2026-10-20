fn main() {
    // translation-rs links Swift's concurrency runtime but its own rpath does
    // not propagate to dependents, so the app binary must add it or dyld fails
    // at launch with "Library not loaded: @rpath/libswift_Concurrency.dylib".
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

        // Bridge to the dictionaries installed with macOS; see
        // src/macos_dictionary.rs.
        println!("cargo:rerun-if-changed=native/dictionary_services.c");
        println!("cargo:rerun-if-changed=native/dictionary_services.h");
        cc::Build::new()
            .file("native/dictionary_services.c")
            .compile("mavoca_dictionary_services");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }

    tauri_build::build()
}
