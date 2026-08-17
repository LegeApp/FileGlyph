# Validation record

The original package was assembled in Linux, then continued and validated on
**2026-07-25** on 64-bit Windows 11 using the stable MSVC Rust toolchain.

Linux support was added and validated on **2026-08-17** on Ubuntu 24.04.4 LTS
(x86_64) with Rust 1.94.1 stable. See the Linux gate below.

## Checks performed in the packaging environment

Run from the repository root:

```text
python tools/static_validate.py
```

The validator checks:

- required source, documentation, script, and sample files;
- TOML and JSON parsing;
- Rust module declarations and source-file presence;
- balanced Rust delimiters while ignoring comments, normal strings, raw strings, and character literals;
- balanced PowerShell delimiters while ignoring comments and quoted strings;
- ICO directory structure, payload bounds, and the configured ten image sizes for every sample icon;
- PNG signature and dimensions for the preview sheet;
- absence of bundled font files.

It is a package-integrity and syntax-structure check, not a Rust type checker.

## Windows gate performed

On Windows 11 with the stable 64-bit MSVC Rust toolchain:

```powershell
.\build-windows.ps1
.\smoke-test.ps1
```

`build-windows.ps1` passed all CLI, GUI, integration, and icon-handler tests and
produced the three release artifacts. `smoke-test.ps1` now creates two disposable
extensions sharing one ProgID, verifies distinct generated ICO mappings through
the dynamic handler, restores the ProgID and both extension values, and cleans up.

The real protected `.txt → Applications\trpad.exe` association was also tested:
Shell extraction returned the generated `TXT` icon, the default open executable
remained unchanged, and a full restore/reapply cycle passed.

## Linux gate performed

On Ubuntu 24.04.4 LTS with the stable 64-bit GNU Rust toolchain:

```bash
cargo test --workspace                  # 29 tests, all passing
cargo build --workspace --release       # CLI and GUI
cargo build --release --no-default-features   # headless CLI only
./smoke-test.sh
```

`smoke-test.sh` passed: it registered a temporary MIME type and handler desktop
entry, confirmed the scanner reported an associated icon-less candidate, verified
`--dry-run` created no files and recorded no state, applied ten PNG sizes into the
theme, confirmed the installed icon became the icon the theme resolves, confirmed
a second extension of the same MIME type was reported `skipped_shared_type`,
restored, and removed everything it created.

Checked by hand against the live system beyond the smoke test:

- `scan` enumerated 1097 file types from shared-mime-info; assessments matched the
  system state (`.html` and `.odt` kept their dedicated icons, while `.pdf`,
  `.png`, `.py`, `.svg` and `.txt` were correctly reported as falling back to a
  shared generic icon).
- `apply --yes` installed the configured ten sizes and `gtk-update-icon-cache`
  rebuilt the theme cache; a rescan showed the applied files as the effective
  icons and the types no longer listed as candidates.
- Restoring one extension removed exactly its own ten files and left another
  applied extension untouched.
- Guarded restore skipped an extension whose installed icons had been changed
  externally, and `--force` then completed it — matching the Windows contract.
- An apply over a pre-existing user icon copied it aside, and restore returned it
  byte for byte (verified by SHA-256) while removing every size FileGlyph added.
- Machine scope wrote to and restored from `/usr/share/icons` when run as root.
- `render`, `preview`, `status`, `refresh` and `init-config` all behaved correctly;
  `preview` rendered legible labels using DejaVu Sans Bold.

The GUI was compiled but not run: this environment has no display server.
