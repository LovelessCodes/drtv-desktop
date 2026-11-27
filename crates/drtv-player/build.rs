//! Point the compiler at a libmpv to link against, and say so clearly when there is none.
//!
//! libmpv2-sys emits `-lmpv` and nothing else, so the search path is this crate's job. Link search
//! paths from a build script are global to the workspace, so the binary crate that depends on this
//! one inherits them; the runtime search path is added by the binary crate's own build script.

include!("../../build-support/mpv.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DRTV_MPV_LIB_DIR");

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    match mpv_lib_dir(manifest) {
        Some(dir) => println!("cargo:rustc-link-search=native={}", dir.display()),
        // Not fatal: `cargo check` needs no library, and a hard failure here would make even that
        // impossible. The link error names the missing piece.
        None => println!(
            "cargo:warning=libmpv not found; set DRTV_MPV_LIB_DIR or install mpv \
             (macOS: brew install mpv, Debian: apt install libmpv-dev)"
        ),
    }
}
