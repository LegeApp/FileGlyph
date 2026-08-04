# Implementing-agent handoff

The source is intended to be taken directly to a Windows 11 machine for the first compile and behavioral pass.

## First pass: compile, do not redesign

1. Install stable Rust MSVC and run `build-windows.ps1`.
2. Fix only concrete compiler/API issues. Preserve the module boundaries and CLI contract unless a Windows API requires a change.
3. Run all unit and integration tests.
4. Run `smoke-test.ps1` from a non-elevated PowerShell session.
5. Confirm the generated `.fglyphdemo` icon contains all configured sizes by reopening it with the `ico` crate or a trusted icon inspector.
6. Repeat the smoke test in an Explorer folder containing several `.fglyphdemo` files at small, medium, large, and extra-large icon views.

## Windows behavior checklist

- `scan --all --extensions fglyphdemo` resolves Notepad as the executable.
- The initial icon is assessed as `inherited_executable`.
- `apply --dry-run` creates no files, state, or registry keys.
- `apply --yes` creates one ICO and an HKCU extension-level `DefaultIcon` value.
- The default open command remains Notepad.
- `status` reports a confirmed record and the original value.
- `restore --yes` removes only the value FileGlyph added and removes its icon file.
- A second application followed by an external registry edit causes ordinary restore to skip; `--force` restores.
- Machine scope fails clearly without elevation and succeeds in an elevated shell.

## Candidate-quality pass

Export these two scans on a representative Windows installation:

```powershell
fileglyph scan --mode conservative --format json > conservative.json
fileglyph scan --all --format json > all.json
```

Review at least 100 associated extensions. Record:

- true missing icons;
- true executable matches;
- generic-shell false positives;
- custom icons incorrectly marked generic;
- extensions that should receive a different category;
- labels that abbreviate poorly.

Do not simply expand hard-coded lists after every odd result. Add evidence from content type, perceived type, ProgID, or executable identity where a stable rule exists. Put one-off preferences in configuration examples.

## Highest-value technical improvement

Implement optional pixel-level icon equivalence without discarding the current string evidence:

- obtain system-rendered icons for the extension and opening executable;
- compare multiple sizes;
- expose hashes/similarity in JSON;
- add a distinct assessment or evidence flag;
- test DPI and light/dark backgrounds;
- keep the operation deterministic and cache results during one scan.

This is the strongest route to finding programs that expose their generic executable icon through a resource indirection.

## Renderer tuning pass

Use the supplied `preview` command and real Explorer views. Tune only through `IconStyle` defaults:

- font-height ratio;
- horizontal scale;
- top/right padding;
- halo radius/alpha;
- maximum label width.

The invariant is fixed vertical label size. Do not solve fit by shrinking the font per extension.

Test labels of length 1, 2, 3, and 4, including narrow and wide letters: `I`, `W`, `DB`, `PDF`, `DOCX`, `SQLT`.

## Restore-hardening pass

Add tests for:

- interrupted apply with an unconfirmed state record;
- reapply preserving the first pre-FileGlyph value;
- current scoped value already absent;
- current value differing only in path quoting/case;
- icon file removed independently;
- corrupt or future-version state files;
- partial batch failure.

Consider storing a normalized value alongside the exact string, but always restore the exact original string.

## Packaging after behavior is stable

Only after the scanner and restore path are proven:

- add a signed installer or portable release archive;
- add a GUI that calls the same library operations;
- request elevation only when machine scope is chosen;
- optionally add an opt-in audit mode that reports overwritten managed values;
- do not add registry ACL hardening by default.
