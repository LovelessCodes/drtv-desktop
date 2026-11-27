# DRTV desktop

A native desktop client for DRTV — DR's streaming service — for macOS, Linux and Windows, written
in Rust with `egui` and `libmpv`.

It is a dense, keyboard-and-mouse window rather than a television interface: a six-column poster
grid on a 1440×900 window, a hero on each title, and a player that is just the video and a HUD that
gets out of the way.

**This is an unofficial client.** It is not made, endorsed or supported by DR, and it ships no DR
logos, artwork or trademarks of its own. Everything you watch comes from DR's own service under
DR's own terms; if you are not in Denmark, most of the catalogue will not be available to you,
because that is DR's decision, not this app's.

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

You need Rust 1.95 or newer and a system libmpv.

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

`bundle` builds in release, writes an `Info.plist`, and copies libmpv **with its entire dylib
closure** — Homebrew's libmpv alone links against sixty-odd libraries — into `Contents/Frameworks`,
rewriting every install name to `@rpath/…` and re-signing ad hoc. The result runs on a machine that
has never had Homebrew installed. Windows and Linux packaging is not written yet; the app builds and
runs there against a system or vendored libmpv.

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
