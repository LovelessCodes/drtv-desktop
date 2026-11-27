# DRTV desktop

[![CI](https://github.com/LovelessCodes/drtv-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/LovelessCodes/drtv-desktop/actions/workflows/ci.yml)

A native desktop client for DRTV — DR's streaming service — for macOS, Linux and Windows, written
in Rust with `egui` and `libmpv`.

It is a dense, keyboard-and-mouse window rather than a television interface: a six-column poster
grid on a 1440×900 window, a hero on each title, and a player that is just the video and a HUD that
gets out of the way.

**This is an unofficial client.** It is not made, endorsed or supported by DR, and it ships no DR
logos, artwork or trademarks of its own. Everything you watch comes from DR's own service under
DR's own terms; if you are not in Denmark, most of the catalogue will not be available to you,
because that is DR's decision, not this app's.

## Installing

From [the latest release][release], for the Mac, Linux or Windows. libmpv travels with the app in
every case, together with everything libmpv links against that the operating system does not own.

**macOS** — unzip and move `DRTV.app` wherever you keep applications. It is ad-hoc signed rather
than notarised (there is no Apple developer account behind this), so the first launch needs
**System Settings → Privacy & Security → Open Anyway**, or:

```sh
xattr -dr com.apple.quarantine /Applications/DRTV.app
```

**Linux** — unpack the tarball anywhere and run `./drtv-desktop` from inside it. The binary finds its
libraries through an `$ORIGIN/lib` rpath, so the directory can live in `~/.local/opt`, on a USB
stick, or wherever. `RUNNING.txt` inside has the two symlinks that put it on your `PATH` and in your
application menu. It needs what any other desktop application needs: an X11 or Wayland client
library, `libxkbcommon` (with `libxkbcommon-x11`), and a working GL driver.

**Windows** — unzip and run `drtv-desktop.exe`. Keep `libmpv-2.dll` beside it; Windows looks for it
next to the executable.

[release]: https://github.com/LovelessCodes/drtv-desktop/releases/latest

## What it does

- The DRTV front page, in DR's own order, with its own shelves.
- Search across DR's catalogue, grouped the way DR groups it.
- Title pages: hero, description, seasons, episodes, resume points.
- Playback of DR's clear (unencrypted) HLS titles, with audio and subtitle track selection,
  scrubbing, skip markers and resume.
- Device-code sign-in, so the app can read your watch history, your list, and write your progress
  back. Signing in is optional; browsing and playing without it works.

## What it does not do

- **Live television.** The TV endpoints this client speaks do not expose DR's live channels; the
  "Se live tv" rows in DR's front page are help links. Live is out of scope for now rather than
  half-built.
- **PlayReady titles.** Some of DR's catalogue is DRM-protected. Those play in DR's own apps and
  not here: there is no CDM in this client and no plan to add one.
- **Downloads and casting.**

## Building

Instead of a download: you need Rust 1.95 or newer and a system libmpv.

```sh
# macOS
brew install mpv

# Debian/Ubuntu
sudo apt install libmpv-dev

# Fedora
sudo dnf install mpv-libs-devel

# Windows: put libmpv-2.dll on the build's library path, or set DRTV_MPV_LIB_DIR
```

```sh
cargo run --release -p drtv-desktop
```

The build script looks for libmpv in this order: `DRTV_MPV_LIB_DIR`, `vendor/mpv/<target>/lib`,
`pkg-config --libs-only-L mpv`, then the platform's usual places (including `brew --prefix mpv`). If
it cannot find one it warns and the binary will fail to start.

`RUST_LOG` works as usual (`RUST_LOG=drtv_app=debug`).

## Packaging

```sh
cargo xtask bundle   # → target/DRTV.app
cargo xtask icon     # → assets/icon/ (PNG sizes, plus AppIcon.icns on macOS)
```

Release builds are made by tagging: `.github/workflows/release.yml` bundles both Mac architectures
and publishes them, and a run can be repeated from the Actions tab for any existing tag.

`bundle` builds in release and assembles whatever the platform ships: on macOS a `DRTV.app` with
libmpv **and its entire dylib closure** in `Contents/Frameworks` (Homebrew's libmpv alone links
against sixty-odd libraries), every install name rewritten to `@rpath/…` and the bundle re-signed
inside out; on Linux `target/drtv-desktop-linux/`, the binary plus libmpv's closure in `lib/`, joined
by an `$ORIGIN` rpath; on Windows `target/drtv-desktop-windows/`, the exe with `libmpv-2.dll` beside
it.

The libraries the operating system owns are deliberately left out on every platform: glibc and the
loader, the graphics drivers, and the display server's client libraries on Linux — a bundled one of
those cannot work on a machine that is not the build machine. The Linux bundler asks every object it
ships what it declares in `DT_NEEDED` and refuses to finish if an answer is in neither `lib/` nor the
host.

## Where it remembers you

Signing in is optional: browsing, search and playback all work anonymously. DR keeps a watch
position per *profile*, though, which means an anonymous viewer gets no resume at all. So the app
keeps its own: `progress.json` in the data directory, written while a title plays and read when it
opens, and a title watched to the end is forgotten rather than offered its last two minutes again.
The later of the two positions wins, so signing in tomorrow does not undo tonight.

## Keys

| Key | Does |
| --- | --- |
| `/` or `⌘K` | Search, from anywhere |
| `Esc` | Back — leaves the player as well |
| `Space` | Play/pause |
| `←` `→` | Ten seconds back, ten seconds forward |
| `F` | Fullscreen |
| `S` | Subtitle tracks |
| `A` | Audio tracks |
| `M` | Mute |

## Layout

| Crate | What lives there |
| --- | --- |
| `drtv-net` | Blocking HTTP over `ureq`: one agent, one response type, no query strings in logs. |
| `drtv-api` | DR's wire format and the mapping to what a screen needs: cards, shelves, detail pages, markers, auth, session. |
| `drtv-player` | `libmpv` behind a render API, drawn into an offscreen framebuffer for egui to paint. |
| `drtv-ui` | Palette, fonts, primitives-drawn icons, and the widgets tiles and shelves are made of. |
| `drtv-app` | The shell: routing, worker threads, image cache, and the five screens. |
| `drtv-desktop` | The binary: window setup, nothing else. |
| `xtask` | The icon and the bundle. |

The UI thread never blocks: every network read and every image decode happens on a worker and comes
back as a message tagged with the generation of the page that asked for it, so a slow answer for a
page you have already left is dropped rather than drawn.

## Smoke tests

`DRTV_START` opens a route at launch, which is how the screens get checked without a mouse:

```sh
DRTV_START=detail:623436 cargo run -p drtv-desktop   # a title, by DR item id
DRTV_START=play:622108   cargo run -p drtv-desktop   # straight into the player
DRTV_START=search:borgen cargo run -p drtv-desktop
DRTV_START=account       cargo run -p drtv-desktop
```

`cargo run -p drtv-api --example probe` prints the front page the way the app would draw it, which
is the quickest way to tell a wire-format change from a UI bug.

## Licence

GPL-3.0-or-later. The bundled Inter typeface is under the SIL Open Font License; its licence travels
with the font and with the app bundle.
