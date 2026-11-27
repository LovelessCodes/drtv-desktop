// Where to find libmpv, shared by every build script that needs it.
//
// Included with `include!` rather than published as a crate: it is build-script code with no
// dependencies, and a crate for it would be a workspace member with one function.
//
// Resolution order, first hit wins:
//   1. `DRTV_MPV_LIB_DIR` — an explicit answer, for CI and for packagers.
//   2. `vendor/mpv/<arch-os>/lib` — the runtime `cargo xtask fetch-mpv` downloads.
//   3. `pkg-config --libs-only-L mpv` — the portable answer on Linux and Homebrew/Nix.
//   4. Platform defaults, including `brew --prefix mpv` on macOS.


use std::path::{Path, PathBuf};
use std::process::Command;

/// `manifest_dir` is the calling crate's `CARGO_MANIFEST_DIR`; the vendored directory is found
/// relative to it so the same file works from any member.
fn mpv_lib_dir(manifest_dir: &Path) -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DRTV_MPV_LIB_DIR") {
        let dir = PathBuf::from(dir);
        if has_mpv_library(&dir) {
            return Some(dir);
        }
    }
    if let Some(dir) = vendored_mpv(manifest_dir) {
        return Some(dir);
    }
    if let Some(dir) = pkg_config_mpv() {
        return Some(dir);
    }
    platform_mpv_defaults()
}

fn vendored_mpv(manifest_dir: &Path) -> Option<PathBuf> {
    // `crates/<member>` → workspace root
    let root = manifest_dir.join("../..").join("vendor/mpv");
    let host = format!(
        "{}-{}",
        std::env::consts::ARCH,
        match std::env::consts::OS {
            "macos" => "apple-darwin",
            "windows" => "pc-windows-msvc",
            "linux" => "unknown-linux-gnu",
            other => other,
        }
    );
    let dir = root.join(host).join("lib");
    has_mpv_library(&dir).then_some(dir)
}

fn pkg_config_mpv() -> Option<PathBuf> {
    let out = Command::new("pkg-config")
        .args(["--libs-only-L", "mpv"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    text.split_whitespace()
        .filter_map(|flag| flag.strip_prefix("-L"))
        .map(PathBuf::from)
        .find(|dir| has_mpv_library(dir))
}

fn platform_mpv_defaults() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![
        PathBuf::from("/opt/homebrew/lib"),
        PathBuf::from("/usr/local/lib"),
        PathBuf::from("/usr/lib"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu"),
        PathBuf::from("/usr/lib/aarch64-linux-gnu"),
    ];
    if let Some(prefix) = brew_prefix_mpv() {
        candidates.insert(0, prefix.join("lib"));
    }
    candidates.into_iter().find(|dir| has_mpv_library(dir))
}

fn brew_prefix_mpv() -> Option<PathBuf> {
    let out = Command::new("brew").args(["--prefix", "mpv"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    path.is_dir().then_some(path)
}

fn has_mpv_library(dir: &Path) -> bool {
    if ["libmpv.dylib", "libmpv.so", "libmpv-2.dll", "mpv.lib", "libmpv.lib"]
        .iter()
        .any(|name| dir.join(name).exists())
    {
        return true;
    }
    // A versioned soname (`libmpv.so.2`, `libmpv.2.dylib`) is what most distributions install.
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                name.starts_with("libmpv") && (name.contains(".so") || name.ends_with(".dylib"))
            })
        })
        .unwrap_or(false)
}
