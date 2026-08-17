# FileGlyph prototype

FileGlyph is a CLI and GUI for **Windows 11 and Linux** that finds registered file
types with missing or generic icons, generates restrained text-only icons, and
installs them as per-file-type visual identities.

The intended result is not a set of decorative application icons. It is a quiet system convention: a file extension becomes readable at a glance through a short label, while a stable category color distinguishes text, documents, images, video, databases, internal application files, and related groups.

## What this prototype already does

Shared across both platforms:

- Assigns one of 14 categories using extension lists, MIME/content type, perceived type, and association-name hints.
- Renders at 16, 20, 24, 32, 40, 48, 64, 96, 128, and 256 pixels.
- Loads a system-installed UI font at run time; no font file is bundled.
- Detects file types with no icon, types showing the icon of the program that
  opens them, types falling back to a shared generic icon, and types that already
  carry an override — which automatic modes preserve.
- Saves the prior setting before writing and provides guarded restoration.
- Refuses executable and high-risk system extensions unless they are explicitly enabled.

On Windows:

- Enumerates extension keys from the merged Windows class registry and Explorer's per-user `FileExts` history.
- Resolves the effective ProgID, executable, application name, document name, content type, perceived type, and icon using `AssocQueryStringW`.
- Flags likely generic icons sourced from `shell32.dll`, `imageres.dll`, or `ddores.dll`.
- Generates a multi-image `.ico` and writes `DefaultIcon` at user or machine scope.
- On Windows builds where a protected `UserChoiceLatest` bypasses that value, installs a per-user dynamic icon handler without changing the default application.
- Calls `SHChangeNotify(SHCNE_ASSOCCHANGED, ...)` after changes.

On Linux:

- Enumerates file types from the freedesktop shared-mime-info database.
- Resolves the handling application from `mimeapps.list`, `mimeinfo.cache`, and the
  `MimeType=` declarations in installed desktop entries.
- Resolves the icon a type actually displays by walking the active icon theme and
  its inheritance chain, and reports a type as generic when the desktop fell back
  from the type's own icon name to a shared one.
- Installs PNGs at every configured size into the active XDG icon theme, backing
  up any icons it displaces.
- Refreshes the icon caches with `gtk-update-icon-cache` and `xdg-icon-resource`.

See [How Linux support works](#how-linux-support-works) for the model and its one
real difference in behaviour.

## Important correction about elevated access

The normal mode is **per-user** and should not require Administrator or root
rights. It writes to:

```text
Windows   HKEY_CURRENT_USER\Software\Classes\.ext\DefaultIcon
Linux     $XDG_DATA_HOME/icons/<active theme>/<size>/mimetypes/<type>.png
```

Machine-wide mode does require an elevated process:

```text
Windows   HKEY_LOCAL_MACHINE\Software\Classes\.ext\DefaultIcon
Linux     /usr/share/icons/<active theme>/<size>/mimetypes/<type>.png
```

Use machine scope only when a genuinely machine-wide convention is required. Per-user scope is safer, easier to restore, and sufficient for the usual desktop. On Linux it is also the only scope that keeps out of directories owned by the distribution's packages.

FileGlyph does **not** edit Explorer's protected `UserChoice` key, does not touch
`mimeapps.list`, and on neither platform does it change which program opens a file.

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

## Build on Linux

Install the stable Rust toolchain, then run:

```bash
./build-linux.sh
```

Equivalent manual commands:

```bash
cargo fmt --all
cargo test --workspace --all-targets
cargo build --workspace --release
```

The executables will be:

```text
target/release/fileglyph
target/release/fileglyph-gui
```

Build agents and servers with no desktop libraries can skip the GUI entirely:

```bash
./build-linux.sh --headless      # or: cargo build --release --no-default-features
```

Runtime dependencies are the ones a desktop already has: `shared-mime-info` for
the type database, an icon theme, and a TrueType font. `gtk-update-icon-cache`
and `xdg-icon-resource` are used when present and skipped when not.

## Safe first test

A complete disposable user-scope test is included for each platform:

```powershell
.\smoke-test.ps1
```

```bash
./smoke-test.sh
```

The Windows script creates a temporary `.fglyphdemo` association to Notepad, verifies that the scanner sees the executable-derived icon, applies a generated icon, restores the prior state, and deletes the test association in `finally` cleanup.

The Linux script registers a temporary MIME type and a handler desktop entry of
its own, checks that the scanner sees an icon-less candidate, applies, confirms
the installed icon becomes the one the desktop resolves, confirms a second
extension of the same type defers rather than fighting over it, restores, and
removes everything it created from an `EXIT` trap. Both refuse to start if any of
their artifacts already exist.

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

- `missing`: only associated types for which the system reports no icon.
- `conservative`: missing, executable-derived, and likely generic shell icons.
- `aggressive`: conservative plus any associated type that lacks an extension-level override, even when its ProgID supplies a dedicated icon.

The default `scan` output contains candidates only. `--all` exposes the surrounding registrations for review, and `--include-unassociated` adds types no installed program claims.

The command examples in this section are written for PowerShell. The same
invocations work unchanged in a Linux shell as `./fileglyph …`.

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

### Render without changing system settings

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

Restore is guarded. If something else changed the same scoped setting after FileGlyph applied it — a registry value on Windows, the installed icon files on Linux — FileGlyph skips that extension rather than overwriting the later change. `--force` deliberately bypasses that guard.

Where an apply displaced icons that were already present, restore puts the
originals back byte for byte from the copies it made.

## Icon convention

The default style is deliberately plain:

- transparent canvas;
- uppercase extension label, normally no more than four characters;
- fixed vertical font size for every extension;
- horizontal compression only, so long labels fit without becoming vertically smaller;
- top-aligned and right-aligned placement;
- category-colored text;
- thin neutral contrast halo for both light and dark file-manager backgrounds;
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
Windows   %LOCALAPPDATA%\FileGlyph\config.json
Linux     $XDG_DATA_HOME/FileGlyph/config.json    (usually ~/.local/share/FileGlyph)
```

`style.font_paths` is searched in order and may hold entries for both platforms;
the first path that exists is used, so one configuration file can serve both.

See `config.example.json` for category, label, color, font, and exclusion examples. A different file can be supplied globally:

```powershell
fileglyph --config D:\Settings\fileglyph.json scan
```

Generated user icons and state are stored below:

```text
Windows   %LOCALAPPDATA%\FileGlyph\icons
          %LOCALAPPDATA%\FileGlyph\state-user.json

Linux     $XDG_DATA_HOME/icons/<active theme>/    (icons must live in a theme)
          $XDG_DATA_HOME/FileGlyph/state-user.json
          $XDG_DATA_HOME/FileGlyph/backup/        (icons displaced by an apply)
```

Machine-scope icons are stored below `%PROGRAMDATA%\FileGlyph\icons` on Windows and `/usr/share/icons` on Linux, while the recovery state remains in the invoking user's data directory on both.

## How Linux support works

Linux desktops do not give an icon to a file *extension*. They give it to a *MIME
type*, which the shared-mime-info database derives from the extension. `.pdf` has
no icon of its own — `application/pdf` does.

An override is a PNG placed in an XDG icon theme under the name the desktop looks
up, so `application/pdf` becomes `application-pdf.png`. Because `$XDG_DATA_HOME`
precedes the system data directories in the theme search path, a user-scope copy
of a theme directory shadows the distribution's copy without modifying it.

FileGlyph installs into the **active** icon theme rather than into `hicolor`.
Themes are searched before their fallbacks, so a theme that defines its own
`application-pdf` would otherwise win over anything dropped into `hicolor`. The
theme is read from `$FILEGLYPH_ICON_THEME`, then the GTK settings files, then
`gsettings`, and finally defaults to `hicolor`. Restore works from the path
recorded at apply time, so changing themes in between cannot strand the files.

### One extension per file type

This is the one place Linux behaves differently from Windows, and it is inherent
rather than a limitation of the implementation. Several extensions commonly share
one type — `.txt` and `.asc` are both `text/plain`; `.cpp`, `.cc`, `.cxx` and
`.c++` are all `text/x-c++src` — and a type has exactly one icon, so it can carry
exactly one label.

FileGlyph makes that explicit instead of letting the last apply silently relabel
the earlier ones. The first extension applied owns the type's icon; the rest are
reported as `skipped_shared_type`, naming the owner and the shared type:

```text
.c     applied_extension     text-x-c++src.png   code / C
.cpp   skipped_shared_type   text-x-c++src.png   shares the text/x-c++src file type with .c
```

Ownership is recorded in the state file, so it holds across runs until that
extension is restored. To choose which label a shared type gets, apply only the
extension you want, or set a `label_overrides` entry for it.

### What counts as associated

As on Windows, FileGlyph only offers to change types some installed program
claims. Having a MIME type is not enough — a handler has to exist in
`mimeapps.list`, `mimeinfo.cache`, or an installed desktop entry's `MimeType=`.
On a minimal system with few applications installed, very few types qualify;
`scan --all --include-unassociated` shows the rest.

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
- The Linux backend approximates the desktop's icon lookup by scanning theme
  directories rather than reading each theme's `index.theme` directory list. It
  picks the largest raster available and prefers PNG over SVG, which is what
  FileGlyph installs and compares against; an exotic theme layout could resolve
  differently from the real file manager.
- Linux desktops cache icons at least as aggressively as Explorer. FileGlyph runs
  `gtk-update-icon-cache` and `xdg-icon-resource` where available, but some file
  managers still need a restart before old icons disappear.
- Machine scope on Linux writes into `/usr/share/icons/<theme>`, which belongs to
  a distribution package. Those files survive until the theme package is updated
  or reinstalled. Prefer user scope.
- Icons are installed into the active icon theme, so switching themes leaves them
  behind for the old theme. Restore still removes them correctly, because it uses
  the path recorded at apply time, but they will not be in effect meanwhile.
- Type descriptions are read from the untranslated `<comment>` in shared-mime-info
  XML; the locale-specific names are ignored.
- The Windows build and smoke-test scripts remain the authoritative compile/run
  gate for the Windows backend. See `VALIDATION.md`.

## Repository map

```text
src/platform/windows.rs  Windows association and registry integration
src/platform/linux.rs    XDG shared-mime-info and icon-theme integration
src/scan.rs              candidate assessment
src/classify.rs          extension/category and label rules
src/icon.rs              fontdue renderer, multi-size ICO and PNG writers
src/operations.rs        apply/render/restore workflows
src/state.rs             recovery journal
src/cli.rs               command-line contract
src/bin/fileglyph-gui.rs egui front end
src/paths.rs             per-platform data, icon and backup locations
icon-handler/            minimal Explorer-loaded COM DLL (Windows only)
DESIGN.md                 architecture and design rationale
AGENT-HANDOFF.md          continuation checklist for an implementing agent
build-windows.ps1 / smoke-test.ps1   Windows build and end-to-end check
build-linux.sh    / smoke-test.sh     Linux build and end-to-end check
tools/static_validate.py   offline package and sample-icon verifier
VALIDATION.md              checks run in the packaging environment
```

The project is intentionally dependency-light and keeps all host-specific code confined to `src/platform/`. Everything above that layer works in extensions and treats the rest as opaque strings, which is what lets one scanner and one apply/restore path serve both systems.
