//! The Windows zip: the binary with `libmpv-2.dll` beside it.
//!
//! There is no rpath on Windows and no framework directory: the loader looks next to the
//! executable first, so "bundled" means "the DLL is in the same folder as the exe". The Microsoft
//! runtime is not copied — a Rust MSVC binary needs only the universal CRT, which every supported
//! Windows has.
//!
//! Finding the DLL is the same search the build script does, so a build that linked against a
//! particular libmpv also bundles that same one: `DRTV_MPV_LIB_DIR`, then `vendor/mpv/<target>/lib`,
//! then whatever pkg-config or a package manager left in the usual places.

use crate::{APP, BINARY, copy_into, run};

// `mpv.rs` brings its own `std::path` and `std::process` imports with it, so importing them here
// too would be a duplicate definition — which only shows up on a Windows build.

include!("../../build-support/mpv.rs");

/// Every name libmpv has shipped under on Windows, newest first.
const DLL_NAMES: &[&str] = &["libmpv-2.dll", "mpv-2.dll", "libmpv.dll"];

pub fn bundle(root: &Path) -> Result<(), String> {
    run(Command::new("cargo").current_dir(root).args(["build", "--release", "-p", BINARY]))?;

    let built = root.join("target/release").join(format!("{BINARY}.exe"));
    let out = root.join("target").join(format!("{BINARY}-windows"));
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;

    copy_into(&built, &out.join(format!("{BINARY}.exe")))?;

    let dll = dll(root)?;
    let name = dll.file_name().ok_or("libmpv has no file name")?.to_string_lossy().to_string();
    copy_into(&dll, &out.join(&name))?;

    copy_into(&root.join("LICENSE"), &out.join("LICENSE.txt"))?;
    copy_into(&root.join("README.md"), &out.join("README.md"))?;
    std::fs::write(out.join("RUNNING.txt"), running(&name)).map_err(|e| e.to_string())?;

    println!("\n{}", out.display());
    println!("  bundled {name}");
    Ok(())
}

/// The `libmpv-2.dll` this build linked against. `mpv_lib_dir` is the build script's own resolver, so
/// the two cannot disagree about which libmpv this binary needs at run time.
fn dll(root: &Path) -> Result<PathBuf, String> {
    let manifest = root.join("crates").join(BINARY);
    let dir = mpv_lib_dir(&manifest)
        .ok_or("libmpv not found: set DRTV_MPV_LIB_DIR or install mpv's development files")?;
    DLL_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.exists())
        .ok_or_else(|| format!("{} holds no libmpv DLL ({})", dir.display(), DLL_NAMES.join(", ")))
}

fn running(dll: &str) -> String {
    format!(
        "{APP} desktop — unofficial, not affiliated with DR.\n\
         \n\
         Run drtv-desktop.exe. Keep {dll} in the same folder: Windows looks for it beside the\n\
         executable, and the app will not start without it.\n\
         \n\
         Nothing is written next to the exe. Settings, the sign-in session and the local resume\n\
         points live in %APPDATA%\\drtv-desktop.\n"
    )
}
