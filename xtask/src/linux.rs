//! The Linux tarball: the binary, and libmpv's own closure beside it in `lib/`.
//!
//! Linux has no bundle to hang libraries off, so the layout is a directory somebody can unpack and
//! run from: `drtv-desktop` next to `lib/`. The binary's `RPATH` is `$ORIGIN/lib` — relative to the
//! executable, so it works from anywhere, including a directory that has been moved.
//!
//! What is *not* copied matters as much as what is. The C library and the loader are the host's: a
//! bundled glibc breaks on the first machine whose kernel is older than the build container's. The
//! graphics drivers and the display server's client libraries are the host's too, because a bundled
//! libGL cannot reach the kernel driver the host actually has, and a bundled libX11 cannot talk to
//! the host's X server. Everything else — ffmpeg, libass, libplacebo, harfbuzz, and the rest of
//! libmpv's long tail — travels with the app.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{APP, BINARY, copy_into, run};

/// Libraries the running machine owns. Matched by soname prefix, because the version suffix is the
/// part that changes and the part that has to keep matching the host.
///
/// `libstdc++` is here for the same reason as glibc: ffmpeg and libass are the only consumers, they
/// are happy with whatever the host provides, and shipping a C++ runtime alongside a distro's is a
/// well-known way to get two of them loaded.
const HOST_OWNED: &[&str] = &[
    // The loader and the C library.
    "ld-linux",
    "libc.so",
    "libm.so",
    "libdl.so",
    "libpthread.so",
    "librt.so",
    "libresolv.so",
    "libutil.so",
    "libnsl.so",
    "libanl.so",
    "libcrypt.so",
    "libmvec.so",
    "libstdc++.so",
    // Graphics: the driver half of the stack belongs to the kernel and the host's mesa.
    "libGL",
    "libEGL",
    "libGLX",
    "libOpenGL",
    "libGLdispatch",
    "libdrm",
    "libgbm",
    "libvulkan",
    "libglapi",
    "libnvidia",
    "libvdpau",
    // The display server's client libraries, and everything they glue to.
    "libX11",
    "libxcb",
    "libwayland",
    "libXext",
    "libXfixes",
    "libXrender",
    "libXau",
    "libXdmcp",
    "libXi",
    "libXcursor",
    "libXrandr",
    "libXinerama",
    "libxkbcommon",
    "libSM",
    "libICE",
];

pub fn bundle(root: &Path) -> Result<(), String> {
    // **Release, always.** The tree is what somebody downloads, and a debug build of this app is
    // several hundred megabytes of unoptimised egui.
    run(Command::new("cargo").current_dir(root).args(["build", "--release", "-p", BINARY]))?;

    let built = root.join("target/release").join(BINARY);
    let out = root.join("target").join(format!("{BINARY}-linux"));
    let lib = out.join("lib");
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;

    let binary = out.join(BINARY);
    copy_into(&built, &binary)?;

    let closure = collect(&built)?;
    for (name, source) in &closure {
        copy_into(source, &lib.join(name))?;
    }

    // `--set-rpath`, not `--add-rpath`: the binary was linked with the build machine's library
    // directories, and a path that exists on exactly one computer has no business in a tarball.
    patchelf(&["--set-rpath", "$ORIGIN/lib"], &binary)?;
    for name in closure.keys() {
        patchelf(&["--set-rpath", "$ORIGIN"], &lib.join(name))?;
    }

    let icon = root.join("assets/icon/icon_256.png");
    if !icon.exists() {
        crate::icon().map_err(|e| format!("icon: {e}"))?;
    }
    copy_into(&icon, &out.join("drtv-desktop.png"))?;
    copy_into(&root.join("LICENSE"), &out.join("LICENSE"))?;
    copy_into(&root.join("README.md"), &out.join("README.md"))?;
    std::fs::write(out.join("drtv-desktop.desktop"), desktop()).map_err(|e| e.to_string())?;

    let (host, unexplained) = audit(&binary, &closure)?;
    if !unexplained.is_empty() {
        return Err(format!("this tarball is not self-contained:\n  {}", unexplained.join("\n  ")));
    }
    std::fs::write(out.join("HOST-LIBS.txt"), host_libs(&host)).map_err(|e| e.to_string())?;
    std::fs::write(out.join("RUNNING.txt"), running(&closure, &host)).map_err(|e| e.to_string())?;

    println!("\n{}", out.display());
    println!("  {} bundled libraries", closure.len());
    println!("  {} expected from the host", host.len());
    Ok(())
}

/// Everything the binary needs that the host does not own, transitively, by the name the loader
/// asks for.
///
/// `ldd` reports both halves of that: the soname on the left (what the ELF needs) and the file on
/// the right (what it found). The file is often a symlink into a versioned name, `libavcodec.so.60`
/// pointing at `libavcodec.so.60.31.102`, and the loader looks the soname up again at run time — so
/// the copy is stored under the soname, which is the name that has to exist.
fn collect(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut found = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    let mut seen = BTreeSet::new();
    while let Some(file) = queue.pop() {
        for (name, path) in dependencies(&file)? {
            if host_owned(&name) {
                continue;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            queue.push(path.clone());
            found.insert(name, path);
        }
    }
    Ok(found)
}

/// `ldd`'s answer for one file: the soname it asks for and the file that answered.
fn dependencies(file: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let out = Command::new("ldd").arg(file).output().map_err(|e| format!("ldd: {e}"))?;
    if !out.status.success() {
        return Err(format!("ldd {}: {}", file.display(), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            // `libavcodec.so.60 => /usr/lib/x86_64-linux-gnu/libavcodec.so.60 (0x…)`, and for the
            // loader itself `/lib64/ld-linux-x86-64.so.2 (0x…)` with no arrow at all.
            let (name, rest) = match line.split_once(" => ") {
                Some((name, rest)) => (name.trim(), rest),
                None => (line.trim(), ""),
            };
            if name.starts_with('/') {
                return None;
            }
            let path = rest.split_whitespace().next().filter(|p| p.starts_with('/'))?;
            // A symlink is how a distribution ships the real file under its soname.
            let real = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
            Some((name.to_string(), real))
        })
        .collect())
}

/// Whether this library has to come from the machine the app runs on.
fn host_owned(name: &str) -> bool {
    HOST_OWNED.iter().any(|prefix| name.starts_with(prefix))
}

fn patchelf(args: &[&str], file: &Path) -> Result<(), String> {
    let out = Command::new("patchelf")
        .args(args)
        .arg(file)
        .output()
        .map_err(|e| format!("patchelf (is it installed? apt install patchelf): {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "patchelf {} {}: {}",
            args.join(" "),
            file.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Every library this tarball ships must be able to find its own dependencies — in `lib/`, or in
/// the machine it runs on. An under-bundled library links fine on the build machine and dies on the
/// first one without it, so this runs before a release does.
///
/// It asks each object what it declares in `DT_NEEDED` rather than asking `ldd`, because `ldd`
/// answers for the whole load: the host's `libgbm` pulls in the host's `libexpat`, and a check that
/// counted those would refuse a tarball that is perfectly complete.
///
/// Returns what the host is expected to bring, and what nobody was going to bring.
fn audit(
    binary: &Path,
    closure: &BTreeMap<String, PathBuf>,
) -> Result<(BTreeSet<String>, Vec<String>), String> {
    let lib = binary.parent().map(|dir| dir.join("lib")).unwrap_or_default();
    let mut files = vec![binary.to_path_buf()];
    files.extend(closure.keys().map(|name| lib.join(name)));

    let mut host = BTreeSet::new();
    let mut unexplained = Vec::new();
    for file in &files {
        for name in needed(file)? {
            if host_owned(&name) {
                host.insert(name);
            } else if !closure.contains_key(&name) {
                let who = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
                unexplained
                    .push(format!("{who} needs {name}, which is in neither `lib/` nor the host"));
            }
        }
    }
    for name in closure.keys() {
        if host_owned(name) {
            return Err(format!("{name} was bundled, but it belongs to the host"));
        }
    }
    Ok((host, unexplained))
}

/// The list shipped beside the libraries, because the moment the app refuses to start is the moment
/// this file is worth reading.
fn host_libs(host: &BTreeSet<String>) -> String {
    let mut text = String::from(
        "Libraries this build expects the machine to provide.\n\
         \n\
         They are here because they are the machine's: the C library and its loader, the OpenGL\n\
         and Vulkan drivers, and the display server's client libraries. A copy inside `lib/` would\n\
         be a copy that has to match a driver and a kernel it was not built against. Every desktop\n\
         distribution ships all of them; a container that has none of them is not a desktop.\n\
         \n",
    );
    for name in host {
        text.push_str("  ");
        text.push_str(name);
        text.push('\n');
    }
    text
}

/// The sonames an ELF declares it needs, from its own dynamic section.
fn needed(file: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("readelf")
        .args(["-d"])
        .arg(file)
        .output()
        .map_err(|e| format!("readelf: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "readelf {}: {}",
            file.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.contains("(NEEDED)"))
        .filter_map(|line| {
            let open = line.find('[')?;
            let close = line.rfind(']')?;
            Some(line[open + 1..close].to_string())
        })
        .collect())
}

fn desktop() -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={APP}\n\
         Comment=An unofficial DRTV client\n\
         Exec={BINARY}\n\
         Icon=drtv-desktop\n\
         Terminal=false\n\
         Categories=AudioVideo;Player;\n"
    )
}

fn running(closure: &BTreeMap<String, PathBuf>, host: &BTreeSet<String>) -> String {
    format!(
        "{APP} desktop — unofficial, not affiliated with DR.\n\
         \n\
         Run it from wherever this directory is:\n\
         \n\
         \t./{BINARY}\n\
         \n\
         It is a single directory on purpose: the binary, the libraries it needs in `lib/`, and the\n\
         icon and `.desktop` file if you would rather install it. Nothing is written outside your\n\
         data directory (~/.local/share/{BINARY}, or `$XDG_DATA_HOME`).\n\
         \n\
         It draws its window with the desktop's own toolchains, so the machine needs what every\n\
         other desktop application already has: an X11 or Wayland client library (including\n\
         libXfixes, libwayland-cursor and libxkbcommon with its libxkbcommon-x11 half) and a GL\n\
         driver. The C library, the driver and those client libraries are deliberately not inside\n\
         `lib/`: they have to match the host. Ubuntu 24.04 and newer, and distributions of that\n\
         age, are what this archive is built against.\n\
         \n\
         To install it for the current user, unpack it to ~/.local/opt and link the two files:\n\
         \n\
         \tln -s ~/.local/opt/{dir}/{BINARY} ~/.local/bin/{BINARY}\n\
         \tcp ~/.local/opt/{dir}/drtv-desktop.desktop ~/.local/share/applications/\n\
         \tcp ~/.local/opt/{dir}/drtv-desktop.png ~/.local/share/icons/\n\
         \n\
         {n} libraries travel with it (libmpv and its closure); {h} are the machine's own — the C\n\
         library, the graphics driver, and the display server's client libraries. `HOST-LIBS.txt`\n\
         names them, and the loader stops before this does if one is missing.\n",
        dir = format!("{BINARY}-linux"),
        n = closure.len(),
        h = host.len()
    )
}
