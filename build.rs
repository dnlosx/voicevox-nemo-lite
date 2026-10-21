// Links against the prebuilt VOICEVOX Nemo core (libvoicevox_core.so 0.15.0),
// whose directory is given by VOICEVOX_CORE_DIR at build time.
fn main() {
    println!("cargo:rerun-if-env-changed=VOICEVOX_CORE_DIR");
    let dir = std::env::var("VOICEVOX_CORE_DIR").unwrap_or_else(|_| "/opt/voicevox_core".into());
    println!("cargo:rustc-link-search=native={dir}");
    println!("cargo:rustc-link-lib=dylib=voicevox_core");
    // Find the .so (and its bundled libonnxruntime) next to where it is installed.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
}
