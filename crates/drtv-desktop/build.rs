//! Give the binary a runtime search path for libmpv.
//!
//! `libmpv2-sys` emits `-lmpv` and nothing else, and a link search path does not survive into the
//! executable: without an rpath, a build against Homebrew's libmpv or a vendored copy starts and then
//! fails to load. Windows needs nothing here — `libmpv-2.dll` sits next to the executable.

include!("../../build-support/mpv.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DRTV_MPV_LIB_DIR");

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let Some(dir) = mpv_lib_dir(manifest) else {
        println!("cargo:warning=libmpv not found; the binary will not start until it is installed");
        return;
    };

    #[cfg(target_os = "macos")]
    if let Some(path) = dir.to_str() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(path) = dir.to_str() {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
        }
        // The bundled layout puts libmpv beside the binary in `lib/`, next to it in `../lib/`.
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib");
    }

    let _ = (dir, manifest);
}
