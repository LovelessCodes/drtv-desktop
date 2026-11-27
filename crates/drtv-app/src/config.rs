//! Where the app keeps state, and how it decides where that is.
//!
//! One JSON file next to the platform's application directory: the session is a refresh token, a
//! profile token and a profile id, and losing it costs one sign-in. Nothing else is stored.

use std::path::PathBuf;

/// `~/Library/Application Support/drtv-desktop` on macOS, `$XDG_DATA_HOME/drtv-desktop` (or
/// `~/.local/share/drtv-desktop`) on Linux, `%APPDATA%\drtv-desktop` on Windows.
pub fn data_dir() -> PathBuf {
    platform_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn session_path() -> PathBuf {
    data_dir().join("session.json")
}

/// Where the machine remembers how far the viewer got, whether or not they signed in.
pub fn progress_path() -> PathBuf {
    data_dir().join("progress.json")
}

/// The typeface libmpv needs on disk to draw subtitles in.
///
/// egui gets its fonts compiled into the binary; mpv can only be told about a directory, so the
/// same Inter files are written out once — the first time the player is created — and reused. A
/// failure here is not fatal: mpv falls back to its own font and the subtitles still read.
pub fn fonts_dir() -> Option<PathBuf> {
    let dir = data_dir().join("fonts");
    std::fs::create_dir_all(&dir).ok()?;
    for (name, bytes) in [
        ("Inter-Regular.ttf", drtv_ui::FONT_REGULAR),
        ("Inter-Medium.ttf", drtv_ui::FONT_MEDIUM),
        ("Inter-SemiBold.ttf", drtv_ui::FONT_SEMIBOLD),
    ] {
        let path = dir.join(name);
        // Rewritten only when missing or truncated, so a normal launch does no I/O here.
        let current = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if current != bytes.len() as u64 {
            std::fs::write(&path, bytes).ok()?;
        }
    }
    Some(dir)
}

/// Log to stderr, and to `RUST_LOG`'s rules. `drtv_app=debug` shows screen and request chatter;
/// the default is warnings only, so a release build is silent.
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,drtv_api=info,drtv_app=info,drtv_net=info"));
    let _ = fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();
}

#[cfg(target_os = "macos")]
fn platform_dir() -> Option<PathBuf> {
    Some(home()?.join("Library/Application Support/drtv-desktop"))
}

#[cfg(target_os = "windows")]
fn platform_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("drtv-desktop"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg).join("drtv-desktop"));
    }
    Some(home()?.join(".local/share/drtv-desktop"))
}

// macOS and the XDG branch both build their directory from a home; Windows has `APPDATA`, so there
// this is dead code that clippy would rightly refuse.
#[cfg(not(target_os = "windows"))]
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}
