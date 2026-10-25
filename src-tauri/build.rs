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
        println!("cargo:rerun-if-changed=native/selection.c");
        println!("cargo:rerun-if-changed=native/selection.h");
        cc::Build::new()
            .file("native/dictionary_services.c")
            .file("native/selection.c")
            .compile("mavoca_dictionary_services");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        // The Accessibility calls that read the selected word; see
        // src/selection.rs.
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }

    tauri_build::build()
}
