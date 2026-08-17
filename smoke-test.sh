#!/usr/bin/env bash
# End-to-end check of the Linux backend against a throwaway file type.
#
# It registers a MIME type and a handler application of its own, drives the whole
# scan -> dry-run -> apply -> status -> restore cycle against them, and removes
# everything it created. Nothing pre-existing is touched: the script refuses to
# start if any of its artifacts already exist. The Windows counterpart is
# smoke-test.ps1.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exe="${1:-$root/target/release/fileglyph}"

extension="fglyphdemo"
second_extension="fglyphdemo2"
mime="application/x-fileglyph-demo"
icon_name="application-x-fileglyph-demo"

data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
mime_package="$data_home/mime/packages/fileglyph-smoke-test.xml"
desktop_entry="$data_home/applications/fileglyph-smoke-test.desktop"

# Pin the theme so the test is independent of the desktop's current setting.
export FILEGLYPH_ICON_THEME="${FILEGLYPH_ICON_THEME:-hicolor}"
theme_dir="$data_home/icons/$FILEGLYPH_ICON_THEME"

fail() { echo "FAIL: $*" >&2; exit 1; }
step() { echo; echo "--- $* ---"; }

[ -x "$exe" ] || fail "FileGlyph executable not found at $exe. Run ./build-linux.sh first."
command -v update-mime-database >/dev/null 2>&1 ||
    fail "update-mime-database not found. Install shared-mime-info."
[ -e "$mime_package" ] && fail "$mime_package already exists. Refusing to overwrite it."
[ -e "$desktop_entry" ] && fail "$desktop_entry already exists. Refusing to overwrite it."

# Count the override files installed for the test type. The theme directory does
# not exist before the first apply, and under `pipefail` a failing find would
# abort the script, so the missing case is answered directly.
installed_icons() {
    if [ ! -d "$theme_dir" ]; then
        echo 0
        return 0
    fi
    find "$theme_dir" -name "$icon_name.png" | wc -l
}

# True when FileGlyph has no recorded state at all.
state_is_empty() {
    [ -z "$("$exe" status --format tsv | tail -n +2)" ]
}

cleanup() {
    "$exe" restore --all --yes >/dev/null 2>&1 || true
    rm -f "$mime_package" "$desktop_entry"
    update-mime-database "$data_home/mime" >/dev/null 2>&1 || true
    echo
    echo "Cleaned up the temporary file type."
}
trap cleanup EXIT

step "Registering a temporary file type"
mkdir -p "$(dirname "$mime_package")" "$(dirname "$desktop_entry")"
cat > "$mime_package" <<XML
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="$mime">
    <comment>FileGlyph smoke-test document</comment>
    <glob pattern="*.$extension"/>
    <glob pattern="*.$second_extension"/>
  </mime-type>
</mime-info>
XML
cat > "$desktop_entry" <<DESKTOP
[Desktop Entry]
Type=Application
Name=FileGlyph Smoke Test Viewer
Exec=/usr/bin/vi %f
Terminal=true
NoDisplay=true
MimeType=$mime;
DESKTOP
update-mime-database "$data_home/mime"

step "Scan"
record="$("$exe" scan --all --extensions "$extension" --format json |
    python3 -c 'import json,sys; print(json.dumps((json.load(sys.stdin) or [None])[0]))')"
[ "$record" = "null" ] && fail "the temporary extension was not returned by scan"
read -r associated candidate assessment prog_id <<EOF
$(printf '%s' "$record" | python3 -c '
import json, sys
r = json.load(sys.stdin)
# json.dumps keeps booleans lowercase, matching the tool own JSON output.
print(json.dumps(r["associated"]), json.dumps(r["candidate"]), r["assessment"], r["prog_id"])')
EOF
echo "associated=$associated candidate=$candidate assessment=$assessment prog_id=$prog_id"
[ "$associated" = "true" ] || fail "the desktop entry was not recognised as a handler"
[ "$prog_id" = "$mime" ] || fail "expected prog_id $mime, got $prog_id"
# A brand-new type has no icon of its own, so it is either missing entirely or
# resolved to a shared fallback. Both are states FileGlyph offers to replace.
case "$assessment" in
    missing|likely_generic_shell) ;;
    *) fail "expected missing or likely_generic_shell, got $assessment" ;;
esac
[ "$candidate" = "true" ] || fail "the type should be a candidate"

step "Dry run"
before="$(installed_icons)"
"$exe" apply --extensions "$extension" --dry-run
[ "$(installed_icons)" -eq "$before" ] || fail "--dry-run installed icon files"
state_is_empty || fail "--dry-run recorded state"

step "Apply"
"$exe" apply --extensions "$extension" --yes
count="$(installed_icons)"
echo "installed $count icon files under $theme_dir"
[ "$count" -gt 0 ] || fail "apply installed no icon files"
"$exe" status --format tsv | tail -n +2 | grep -q "^\.$extension" ||
    fail "apply recorded no state for .$extension"


step "The override now resolves"
resolved="$("$exe" scan --all --extensions "$extension" --format json |
    python3 -c 'import json,sys; r=json.load(sys.stdin)[0]; print(r["assessment"], r["effective_icon"])')"
echo "$resolved"
case "$resolved" in
    extension_override*"$data_home"*) ;;
    *) fail "the applied icon did not become the effective icon: $resolved" ;;
esac

step "A second extension of the same type does not fight over the icon"
"$exe" apply --extensions "$second_extension" --dry-run --format tsv |
    tail -n +2 | grep -q "skipped_shared_type" ||
    fail ".$second_extension should defer to .$extension for the shared type"

step "Restore"
"$exe" restore --extensions "$extension" --yes
[ "$(installed_icons)" -eq 0 ] || fail "restore left icon files behind"
state_is_empty || fail "restore left state behind"

echo
echo "PASS: scan, dry-run, apply, status and restore all behaved as expected."
