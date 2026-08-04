# FileGlyph prototype

FileGlyph is a Windows 11 CLI and GUI that finds registered file extensions with missing or generic icons, generates restrained text-only `.ico` files, and installs per-extension visual identities.

The intended result is not a set of decorative application icons. It is a quiet system convention: a file extension becomes readable at a glance through a short label, while a stable category color distinguishes text, documents, images, video, databases, internal application files, and related groups.

## What this prototype already does

- Enumerates extension keys from the merged Windows class registry and Explorer's per-user `FileExts` history.
- Resolves the effective ProgID, executable, application name, document name, content type, perceived type, and icon using `AssocQueryStringW`.
- Detects:
  - associated extensions with no reported icon;
  - extensions whose icon resolves to index 0 of the associated executable;
  - likely generic icons sourced from `shell32.dll`, `imageres.dll`, or `ddores.dll`;
  - existing extension-level overrides, which are preserved by automatic modes.
- Assigns one of 14 categories using extension lists, MIME/content type, perceived type, and association-name hints.
- Generates a multi-image ICO with 16, 20, 24, 32, 40, 48, 64, 96, 128, and 256-pixel entries.
- Loads a Windows-installed UI font at run time; no font file is bundled.
- Writes `DefaultIcon` at either user or machine scope.
- On Windows builds where a protected `UserChoiceLatest` bypasses that value, installs a per-user dynamic icon handler without changing the default application.
- Saves the prior registry value before writing and provides guarded restoration.
- Calls `SHChangeNotify(SHCNE_ASSOCCHANGED, ...)` after changes.
- Refuses executable and high-risk system extensions unless they are explicitly enabled.

## Important correction about Administrator access

The normal mode is **per-user** and should not require Administrator rights:

```text
HKEY_CURRENT_USER\Software\Classes\.ext\DefaultIcon
```

Machine-wide mode writes here and does require an elevated process:

```text
HKEY_LOCAL_MACHINE\Software\Classes\.ext\DefaultIcon
```

Use machine scope only when a genuinely machine-wide convention is required. Per-user scope is safer, easier to restore, and sufficient for the usual Windows desktop.

FileGlyph does **not** edit Explorer's protected `UserChoice` key and does not change which program opens a file.

## Build on Windows 11

Install the stable Rust MSVC toolchain, open PowerShell in this directory, then run:

```powershell
.\build-windows.ps1
```

Equivalent manual commands:

```powershell
cargo fmt --all
cargo test --all-targets
cargo build --release
```

The executable will be:

```text
target\release\fileglyph.exe
target\release\fileglyph-gui.exe
target\release\fileglyph_icon_handler.dll
```

`fileglyph-gui.exe` provides scan, selection, apply, restore, and Explorer-refresh
controls over the same library operations as the CLI. User scope does not require
Administrator rights. Right-click **Run as administrator** only when selecting
machine scope.

The source declares Rust 1.85 or newer. Build it as a native 64-bit Windows executable using the current stable MSVC toolchain.

## Safe first test

A complete disposable user-scope test is included:

```powershell
.\smoke-test.ps1
```

The script creates a temporary `.fglyphdemo` association to Notepad, verifies that the scanner sees the executable-derived icon, applies a generated icon, restores the prior state, and deletes the test association in `finally` cleanup.

For a real extension, inspect before writing:

```powershell
.\target\release\fileglyph.exe scan --all --extensions asd
.\target\release\fileglyph.exe apply --extensions asd --dry-run
```

Apply in user scope:

```powershell
.\target\release\fileglyph.exe apply --extensions asd --yes
```

Restore exactly what FileGlyph recorded:

```powershell
.\target\release\fileglyph.exe restore --extensions asd --yes
```

## Commands

### Find candidates

```powershell
fileglyph scan
fileglyph scan --mode missing
fileglyph scan --mode conservative
fileglyph scan --mode aggressive
fileglyph scan --all
fileglyph scan --all --format json > scan.json
fileglyph scan --extensions asd,foo,sqlite --all
```

Modes:

- `missing`: only associated types for which Windows reports no icon.
- `conservative`: missing, executable-derived, and likely generic shell icons.
- `aggressive`: conservative plus any associated type that lacks an extension-level override, even when its ProgID supplies a dedicated icon.

The default `scan` output contains candidates only. `--all` exposes the surrounding registrations for review.

### Apply generated icons

```powershell
fileglyph apply --extensions asd,foo --dry-run
fileglyph apply --extensions asd,foo --yes
fileglyph apply --all-candidates --mode conservative --dry-run
fileglyph apply --all-candidates --mode conservative --yes
```

Machine-wide installation:

```powershell
# Run the shell as Administrator first.
fileglyph apply --extensions asd --scope machine --yes
```

A real write always requires `--yes`. Automatic selection never includes the protected extension list unless `--include-protected` is also supplied.

### Render without registry changes

```powershell
fileglyph render --extensions asd,foo,sqlite
fileglyph render --extensions asd --category internal --label ASD
fileglyph preview --output category-preview.png
```

This path is useful for visual iteration before any association work.

### Inspect and restore state

```powershell
fileglyph status
fileglyph status --format json
fileglyph restore --extensions asd --dry-run
fileglyph restore --all --yes
```

Restore is guarded. If another program changed the same scoped registry value after FileGlyph applied it, FileGlyph skips that extension rather than overwriting the later change. `--force` deliberately bypasses that guard.

## Icon convention

The default style is deliberately plain:

- transparent canvas;
- uppercase extension label, normally no more than four characters;
- fixed vertical font size for every extension;
- horizontal compression only, so long labels fit without becoming vertically smaller;
- top-aligned and right-aligned placement;
- category-colored text;
- thin neutral contrast halo for both light and dark Explorer backgrounds;
- no page silhouette, folded corner, picture, glyph, logo, or application branding.

The abbreviation algorithm keeps the first character, then favors digits and consonants. Common exceptions such as `JPEG → JPG` and `SQLITE3 → SQLT` are built in. The JSON configuration can override any label.

Default category colors:

| Category | Color |
|---|---:|
| text | `#4D7C8A` |
| document | `#3F6FB5` |
| spreadsheet | `#3A7D58` |
| presentation | `#B56A32` |
| image | `#9A5CC2` |
| video | `#B04765` |
| audio | `#6A5ACD` |
| archive | `#8A6D3B` |
| database | `#58717A` |
| code | `#2C7A7B` |
| system | `#6B7280` |
| model3d | `#8B5E83` |
| font | `#7B5C48` |
| internal | `#65707E` |

These are intentionally related, medium-dark colors rather than a bright icon palette.

## Configuration

Write the default configuration to the normal location:

```powershell
fileglyph init-config
```

Default path:

```text
%LOCALAPPDATA%\FileGlyph\config.json
```

See `config.example.json` for category, label, color, font, and exclusion examples. A different file can be supplied globally:

```powershell
fileglyph --config D:\Settings\fileglyph.json scan
```

Generated user icons and state are stored below:

```text
%LOCALAPPDATA%\FileGlyph\icons
%LOCALAPPDATA%\FileGlyph\state-user.json
```

Machine-scope icons are stored below `%PROGRAMDATA%\FileGlyph\icons`, while the recovery state remains in the invoking user's local application-data directory.

## Effective icons and protected default applications

FileGlyph first writes an extension-level `DefaultIcon`. It then asks Windows for
the effective icon. New Windows 11 association paths such as `UserChoiceLatest`
can bypass the extension value; in that case user scope installs the bundled
`IExtractIconW` handler under the effective ProgID. The handler chooses the ICO
from the filename extension and never writes or bypasses the protected default-app hash.

It is not an access-control boundary. Another process running as the same user can deliberately write the same `HKCU\Software\Classes\.ext\DefaultIcon` value, and an elevated process can alter machine or user registration. The prototype does not harden registry ACLs because doing so would create brittle ownership and uninstallation problems.

A later tray/service mode could audit and reassert FileGlyph-managed values, but that should be opt-in and should still preserve an explicit restore history.

## Current prototype limitations

- The scanner identifies an executable-derived icon by comparing the resolved icon path and index with the associated executable. It does not yet render and pixel-hash both icons. A program that returns a visually generic icon through a different resource path can evade the conservative heuristic.
- Generic shell DLL detection is intentionally labeled “likely”; those DLLs contain many legitimate Windows resources. Review the list before using `--all-candidates`.
- Windows Explorer caches icons aggressively. FileGlyph sends and flushes the documented association-change notification, but an already-open folder may still need to be reopened.
- Explorer may display a content thumbnail instead of the registered file-type icon for images, videos, and some documents. The generated icon remains the association icon and appears in non-thumbnail views or when no thumbnail is available.
- The dynamic icon handler is currently an unsigned x64 per-user prototype. Existing third-party icon handlers are preserved and reported as conflicts.
- The initial classification tables are broad but not exhaustive. Unknown associated extensions fall into `internal` unless metadata gives a stronger signal.
- Registry backups currently preserve the exact prior string but not its original registry value type. `DefaultIcon` is normally `REG_SZ`, which is also what FileGlyph writes; a later hardening pass should preserve raw value type/data for unusual registrations.
- Apply/restore operations are not process-locked. Do not run two FileGlyph instances against the same scope at once.
- This source package was assembled in a Linux environment without a Rust toolchain, so the included Windows build and smoke-test scripts are the authoritative compile/run gate. See `VALIDATION.md`.

## Repository map

```text
src/platform/windows.rs  Windows association and registry integration
src/scan.rs              candidate assessment
src/classify.rs          extension/category and label rules
src/icon.rs              fontdue renderer and multi-size ICO writer
src/operations.rs        apply/render/restore workflows
src/state.rs             recovery journal
src/cli.rs               command-line contract
src/bin/fileglyph-gui.rs egui front end
icon-handler/            minimal Explorer-loaded COM DLL
DESIGN.md                 architecture and design rationale
AGENT-HANDOFF.md          continuation checklist for an implementing agent
tools/static_validate.py   offline package and sample-icon verifier
VALIDATION.md              checks run in the packaging environment
```

The project is intentionally dependency-light and keeps Windows FFI confined to one module.
