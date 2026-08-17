#!/usr/bin/env bash
# Release build for Linux. The Windows counterpart is build-windows.ps1.
set -euo pipefail

skip_tests=0
skip_format=0
headless=0

usage() {
    cat <<'USAGE'
Usage: ./build-linux.sh [options]

  --skip-tests   Do not run the test suite.
  --skip-format  Do not run cargo fmt.
  --headless     Build only the CLI, without the eframe/winit GUI.
                 Use this on servers and build agents with no desktop libraries.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --skip-tests) skip_tests=1 ;;
        --skip-format) skip_format=1 ;;
        --headless) headless=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$root"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo was not found. Install the stable Rust toolchain from https://rustup.rs." >&2
    exit 1
fi

cargo --version
rustc --version

features=()
if [ "$headless" -eq 1 ]; then
    features+=(--no-default-features)
fi

if [ "$skip_format" -eq 0 ]; then
    cargo fmt --all
fi
if [ "$skip_tests" -eq 0 ]; then
    cargo test --workspace --all-targets "${features[@]}"
fi

cargo build --workspace --release "${features[@]}"

dist="$root/dist"
mkdir -p "$dist"
install -m 0755 "$root/target/release/fileglyph" "$dist/"
if [ "$headless" -eq 0 ]; then
    install -m 0755 "$root/target/release/fileglyph-gui" "$dist/"
fi
install -m 0644 \
    "$root/README.md" \
    "$root/LICENSE" \
    "$root/VALIDATION.md" \
    "$root/config.example.json" \
    "$dist/"

echo "Built: $dist/fileglyph"
if [ "$headless" -eq 0 ]; then
    echo "Built: $dist/fileglyph-gui"
fi
