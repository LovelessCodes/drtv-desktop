//! Build chores that should not live in a shell script.
//!
//! `cargo xtask icon` draws the app icon at every size a dock, a taskbar or a store wants, straight
//! from `drtv_ui::icon` — the same code that gives the window its icon at runtime, so the two can
//! never disagree.
//!
//! `cargo xtask bundle` assembles what a platform can be handed: a `DRTV.app` whose install names
//! are rewritten so it runs on a Mac that has never seen Homebrew, a Linux directory with libmpv's
//! closure beside the binary and an `$ORIGIN` rpath joining them, or a Windows folder with
//! `libmpv-2.dll` next to the exe. The common rule is that libmpv travels with the app; what it
//! links against travels too, except for the pieces the operating system owns.

use std::path::{Path, PathBuf};
use std::process::Command;

// One module per platform's `bundle`, gated so that the others are not dead code on this one — an
// unused module of dead code fails CI's lints, and it should.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub(crate) const APP: &str = "DRTV";
pub(crate) const BINARY: &str = "drtv-desktop";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("icon") => icon(),
        Some("bundle") => bundle(),
        Some("help") | Some("-h") | Some("--help") | None => {
            usage();
            Ok(())
        }
        Some(other) => {
            usage();
            Err(format!("unknown command `{other}`"))
        }
    };
    if let Err(why) = result {
        eprintln!("xtask: {why}");
        std::process::exit(1);
    }
}

fn usage() {
    println!(
        "cargo xtask <command>

  icon     draw the app icon into assets/icon/ (and an .icns on macOS)
  bundle   assemble what this platform ships: target/DRTV.app, or a directory with libmpv in it"
    );
}

/// Every size the platforms ask for, plus the macOS `.iconset` names, which are not the sizes:
/// `icon_512x512@2x.png` is a 1024px image.
fn icon() -> Result<(), String> {
    let out = root().join("assets/icon");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;

    for size in [16u32, 32, 64, 128, 256, 512, 1024] {
        let path = out.join(format!("icon_{size}.png"));
        write_png(&path, size)?;
        println!("{}", path.display());
    }

    if cfg!(target_os = "macos") {
        let iconset = out.join("AppIcon.iconset");
        let _ = std::fs::remove_dir_all(&iconset);
        std::fs::create_dir_all(&iconset).map_err(|e| format!("{}: {e}", iconset.display()))?;
        for (name, size) in [
            ("icon_16x16.png", 16),
            ("icon_16x16@2x.png", 32),
            ("icon_32x32.png", 32),
            ("icon_32x32@2x.png", 64),
            ("icon_128x128.png", 128),
            ("icon_128x128@2x.png", 256),
            ("icon_256x256.png", 256),
            ("icon_256x256@2x.png", 512),
            ("icon_512x512.png", 512),
            ("icon_512x512@2x.png", 1024),
        ] {
            write_png(&iconset.join(name), size)?;
        }
        let icns = out.join("AppIcon.icns");
        run(Command::new("iconutil").args(["-c", "icns", "-o"]).arg(&icns).arg(&iconset))?;
        println!("{}", icns.display());
    }
    Ok(())
}

fn write_png(path: &Path, size: u32) -> Result<(), String> {
    let rgba = drtv_ui::icon::rgba(size);
    let buffer = image::RgbaImage::from_raw(size, size, rgba)
        .ok_or_else(|| format!("{}: icon buffer is not {size}²", path.display()))?;
    buffer.save(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn bundle() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let result = linux::bundle(&root());
    #[cfg(target_os = "macos")]
    let result = macos::bundle(&root());
    #[cfg(target_os = "windows")]
    let result = windows::bundle(&root());
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let result = Err(format!("there is no bundle for {}", std::env::consts::OS));
    result
}

/// Copy a file into a bundle, replacing what an earlier run left there: a copied library keeps its
/// source's read-only mode, and copying over one fails with `Permission denied`, which reads like a
/// problem with the source and is really just a bundle that has been built before.
pub(crate) fn copy_into(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        let _ = std::fs::remove_file(to);
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits one level below the workspace root")
        .to_path_buf()
}

/// Run a command and fail with everything it had to say, because "exit code 1" alone is useless.
pub(crate) fn run(command: &mut Command) -> Result<(), String> {
    let line = format!("{command:?}");
    let out = command.output().map_err(|e| format!("{line}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(format!("{line}\n{}{}", indent(&stdout), indent(&stderr)))
}

fn indent(text: &str) -> String {
    text.lines().map(|l| format!("  {l}\n")).collect()
}
