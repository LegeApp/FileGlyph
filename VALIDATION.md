# Validation record

The original package was assembled in Linux, then continued and validated on
**2026-07-25** on 64-bit Windows 11 using the stable MSVC Rust toolchain.

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
