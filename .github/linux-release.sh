#!/usr/bin/env bash
# Builds the Linux tarball on a pinned image, so the glibc floor of the artifact is a decision
# rather than whatever `ubuntu-latest` happens to be this month. Left to the runner image, a tarball
# keeps working right up until the image moves and then silently stops on last year's distribution.
#
# Run it from the repository root, either in CI (`docker run … ./.github/linux-release.sh`) or by
# hand to see the same artifact locally.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

say() { printf '\n== %s\n' "$*"; }

say "packages"
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
    build-essential ca-certificates curl pkg-config \
    libmpv-dev libasound2-dev libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev \
    libwayland-cursor0 libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxfixes3 \
    libgl1-mesa-dev libgl1-mesa-dri patchelf xvfb > /dev/null

say "rust"
if ! command -v cargo > /dev/null; then
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable > /dev/null
    . "$HOME/.cargo/env"
fi

say "bundle"
cargo xtask bundle

# The tarball has to run for somebody who is not the build machine, so it is started here the way a
# user starts it: unpacked where it lies, on a display that is not ours either.
say "it opens a window and stays open"
cd target/drtv-desktop-linux
log="$(mktemp)"
set +e
xvfb-run -a --server-args="-screen 0 1440x900x24" ./drtv-desktop > "$log" 2>&1 &
pid=$!
sleep 15
kill -0 "$pid" 2>/dev/null
alive=$?
kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
set -e
if [ "$alive" -ne 0 ]; then
    echo "the app is not running; it said:" >&2
    cat "$log" >&2
    exit 1
fi
# A window that opened is a window that logged nothing worse than a warning.
if grep -qiE 'panicked|could not be loaded|error while loading' "$log"; then
    echo "the app started, but complained:" >&2
    cat "$log" >&2
    exit 1
fi

say "done: $(find lib -name '*.so*' | wc -l) libraries"
