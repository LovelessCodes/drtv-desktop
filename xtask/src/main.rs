//! Build chores that should not live in a shell script.
//!
//! `cargo xtask icon` draws the app icon at every size a dock, a taskbar or a store wants, straight
//! from `drtv_ui::icon` — the same code that gives the window its icon at runtime, so the two can
//! never disagree.
//!
//! `cargo xtask bundle` assembles a runnable `DRTV.app`: the binary, an `Info.plist`, the icon, and
//! libmpv with everything it links against, install names rewritten so the bundle runs on a machine
//! that has never seen Homebrew.

use std::path::{Path, PathBuf};
use std::process::Command;

// macOS-only: `bundle` is the whole of it, and an unused module of dead code fails CI's lints.
#[cfg(target_os = "macos")]
mod macos;

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
  bundle   assemble a runnable app bundle into target/ (macOS today)"
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
    #[cfg(not(target_os = "macos"))]
    {
        Err("only the macOS bundle is written so far; Windows and Linux packaging come next".into())
    }
    #[cfg(target_os = "macos")]
    {
        macos::bundle(&root())
    }
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
