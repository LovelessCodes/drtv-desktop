//! The macOS `.app`: binary, `Info.plist`, icon, and libmpv with its whole dylib closure.
//!
//! Homebrew's libmpv links against sixty-odd libraries by absolute path (`/opt/homebrew/opt/…`).
//! Copying only `libmpv.2.dylib` produces an app that launches on the developer's machine and
//! nowhere else, so every non-system dependency is walked, copied into `Contents/Frameworks`, and
//! rewritten to `@rpath/<name>`. The binary and every copied library are re-signed ad hoc
//! afterwards, because changing an install name invalidates the signature.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::run;

const BUNDLE_ID: &str = "codes.loveless.drtv-desktop";
const APP: &str = "DRTV";
const BINARY: &str = "drtv-desktop";

pub fn bundle(root: &Path) -> Result<(), String> {
    // **Release, always.** A debug build of this app is 300 MB and links the debug libmpv paths, and
    // a bundle is the thing you hand to somebody.
    run(Command::new("cargo").current_dir(root).args(["build", "--release", "-p", BINARY]))?;

    let built = root.join("target/release").join(BINARY);
    let app = root.join("target").join(format!("{APP}.app"));
    let contents = app.join("Contents");
    let (macos, resources, frameworks) =
        (contents.join("MacOS"), contents.join("Resources"), contents.join("Frameworks"));
    for dir in [&macos, &resources, &frameworks] {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    let executable = macos.join(BINARY);
    std::fs::copy(&built, &executable).map_err(|e| format!("{}: {e}", executable.display()))?;
    write_plist(&contents.join("Info.plist"))?;

    let icns = root.join("assets/icon/AppIcon.icns");
    if !icns.exists() {
        crate::icon().map_err(|e| format!("icon: {e}"))?;
    }
    copy_into(&icns, &resources.join("AppIcon.icns"))?;
    copy_into(&root.join("LICENSE"), &resources.join("LICENSE"))?;
    copy_into(&root.join("assets/fonts/OFL.txt"), &resources.join("OFL.txt"))?;

    let closure = collect(&executable)?;
    for (name, source) in &closure {
        copy_into(source, &frameworks.join(name))?;
    }

    // Rewrite before signing: every library's own id, then every reference to a library we copied.
    for (name, _) in &closure {
        let target = frameworks.join(name);
        run(Command::new("install_name_tool")
            .arg("-id")
            .arg(format!("@rpath/{name}"))
            .arg(&target))?;
    }
    rewrite(&executable, &closure)?;
    for (name, _) in &closure {
        rewrite(&frameworks.join(name), &closure)?;
    }
    run(Command::new("install_name_tool")
        .args(["-add_rpath", "@executable_path/../Frameworks"])
        .arg(&executable))?;

    // Ad hoc, but a signature nonetheless: an arm64 dylib without one is not loaded at all.
    run(Command::new("codesign").args(["--force", "--sign", "-"]).arg(&executable))?;
    for (name, _) in &closure {
        run(Command::new("codesign").args(["--force", "--sign", "-"]).arg(frameworks.join(name)))?;
    }
    run(Command::new("codesign").args(["--force", "--sign", "-"]).arg(&app))?;

    println!("\n{}", app.display());
    println!("  {} bundled libraries", closure.len());
    Ok(())
}

fn write_plist(path: &Path) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>{APP}</string>
	<key>CFBundleDisplayName</key><string>{APP}</string>
	<key>CFBundleExecutable</key><string>{BINARY}</string>
	<key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
	<key>CFBundleIconFile</key><string>AppIcon</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>{version}</string>
	<key>CFBundleVersion</key><string>{version}</string>
	<key>LSMinimumSystemVersion</key><string>11.0</string>
	<key>LSApplicationCategoryType</key><string>public.app-category.entertainment</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
	<key>NSHumanReadableCopyright</key>
	<string>GPL-3.0-or-later. An unofficial client: not affiliated with DR.</string>
</dict>
</plist>
"#
    );
    std::fs::write(path, plist).map_err(|e| format!("{}: {e}", path.display()))
}

fn copy_into(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{} → {}: {e}", from.display(), to.display()))
}

/// Every non-system library the binary needs, transitively, keyed by file name.
///
/// A name collision would be a bug in the walk, not a choice to make, so the first path wins and the
/// duplicate is left out — Homebrew's own symlink farm resolves several `/opt/homebrew/opt/…` paths
/// to the same file.
fn collect(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut found = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    let mut seen = BTreeSet::new();
    while let Some(file) = queue.pop() {
        for dep in dependencies(&file)? {
            if is_system(&dep) {
                continue;
            }
            let Some(name) = dep.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !seen.insert(name.to_string()) {
                continue;
            }
            found.insert(name.to_string(), dep.clone());
            queue.push(dep);
        }
    }
    Ok(found)
}

/// `otool -L` output, minus the header line and the compatibility notes.
fn dependencies(file: &Path) -> Result<Vec<PathBuf>, String> {
    let out = Command::new("otool")
        .arg("-L")
        .arg(file)
        .output()
        .map_err(|e| format!("otool {}: {e}", file.display()))?;
    if !out.status.success() {
        return Err(format!("otool {}: {}", file.display(), String::from_utf8_lossy(&out.stderr)));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .filter(|path| path.starts_with('/'))
        .map(PathBuf::from)
        .collect())
}

/// Point every reference to a bundled library at `@rpath/<name>`.
fn rewrite(file: &Path, closure: &BTreeMap<String, PathBuf>) -> Result<(), String> {
    for dep in dependencies(file)? {
        let Some(name) = dep.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !closure.contains_key(name) {
            continue;
        }
        run(Command::new("install_name_tool")
            .arg("-change")
            .arg(&dep)
            .arg(format!("@rpath/{name}"))
            .arg(file))?;
    }
    Ok(())
}

fn is_system(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with("/usr/lib/") || text.starts_with("/System/")
}
