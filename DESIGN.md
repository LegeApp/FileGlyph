# FileGlyph design

## 1. Product boundary

FileGlyph owns the visual identity of a **file extension**, not the application association.

It first writes an extension-level icon value:

```text
HKCU or HKLM\Software\Classes\.extension\DefaultIcon
    (Default) = "C:\absolute\path\extension.ico",0
```

It does not write `UserChoice`, does not select an application, and does not
rewrite open commands. If Windows reports that a protected effective association
still bypasses the extension icon, FileGlyph registers its per-user
`IExtractIconW` handler under that effective ProgID. The handler varies the icon
by filename extension while the existing open command remains untouched.

The first prototype is a CLI because registry discovery and restore semantics need to be observable before they are placed behind a GUI.

## 2. Data flow

```text
HKCR extension names ------------------------------+
Explorer FileExts names ---------------------------+--> extension set
                                                      |
AssocQueryStringW + direct HKCR values --------------+--> RawFileType
                                                      |
classification + icon assessment + safety policy ----+--> FileTypeRecord
                                                      |
explicit selection / candidate mode -----------------+--> apply set
                                                      |
fontdue render --> ICO entries --> recovery journal --> registry write
                                                      |
                           effective check --+--> direct override
                                            +--> dynamic handler map
                                                       |
                                            SHChangeNotify + flush
```

### Why use `AssocQueryStringW`

Windows associations can be indirect, merged, and influenced by per-user choices. Reimplementing all lookup rules by walking ProgID keys would be incomplete. The association API is used for the effective executable, ProgID, names, content type, and default icon; the direct extension `DefaultIcon` value is also read because its existence changes overwrite policy.

## 3. Candidate model

Every scanned extension receives one assessment:

1. `unassociated`
2. `extension_override`
3. `missing`
4. `inherited_executable`
5. `likely_generic_shell`
6. `program_icon`

Assessment precedence matters. A direct extension icon that points to index 0 of the opening executable is still classified as `inherited_executable`, because it produces the exact visual failure FileGlyph is meant to correct. A direct extension icon that points elsewhere is treated as an intentional override and is not automatically replaced.

### Modes

- `missing`: assessment 3.
- `conservative`: assessments 3–5.
- `aggressive`: assessments 3–6, but still never assessment 2.

Explicitly named extensions can replace assessment 2, because the user has selected that exact type after inspection.

### Future exact comparison

The string/resource heuristic should eventually be supplemented by an icon-image comparison path:

1. Request the extension icon through `SHGetFileInfoW` or the system image list at 16/32/48/256 sizes.
2. Extract the associated executable's index-0 icon at matching sizes.
3. Normalize to premultiplied RGBA.
4. Compare perceptual hashes and exact alpha-aware hashes.
5. Label identical images `executable_visual_match`, even when resource paths differ.

That should be an additional evidence field, not a silent replacement for the current explainable registry evidence.

## 4. Classification

Classification order:

1. user extension override;
2. ambiguous-extension rule (`.ts` currently uses metadata to choose video or code);
3. known extension table;
4. MIME/content-type prefix or subtype;
5. Windows `PerceivedType`;
6. ProgID and friendly-name keywords;
7. `internal` fallback.

Categories are intentionally broad. The visual system should remain learnable; adding a new color for every technical subtype would defeat the purpose.

Unknown application formats are placed near databases/system stores through the neutral `internal` color, but do not exactly share the database color.

## 5. Label system

Labels are uppercase and normally limited to four characters.

Selection order:

1. JSON override;
2. built-in conventional alias (`JPEG → JPG`, `SQLITE3 → SQLT`);
3. full extension if it fits;
4. deterministic abbreviation: first character, then digits/consonants, then remaining characters.

The vertical font size is invariant. The renderer applies a fixed horizontal scale, with an additional width cap only when necessary. One-character extensions therefore do not become oversized and four-character extensions do not become vertically smaller.

## 6. Rendering

Each size is rendered independently at a configurable supersampling factor and box-downsampled with alpha-aware averaging.

Pipeline:

1. fontdue layout and glyph rasterization;
2. crop to visible text bounds;
3. horizontal resize;
4. top-right placement on a transparent square;
5. separable maximum filter to construct a thin contrast halo;
6. text-over-halo alpha compositing;
7. premultiplied box downsample;
8. BMP encoding for entries up to 48 pixels, PNG encoding above 48 pixels;
9. one ICO directory containing all entries.

The renderer uses a Windows-installed font and never redistributes it.

## 7. Registry transaction and restore

For each extension:

1. render the icon to its final stable path;
2. read the value in the selected scope only;
3. preserve the original value from the first FileGlyph application;
4. save an unconfirmed recovery record;
5. write the registry value;
6. mark the recovery record confirmed;
7. after the batch, send one association-change notification.

Saving the recovery record before the registry write makes interruption recoverable. A failed write leaves a pending record, but guarded restore will only modify the registry when the current value matches the intended FileGlyph value unless `--force` is supplied.

Reapplication does not replace the original backup with FileGlyph's own previous value.

## 8. Safety policy

Automatic application excludes executable, installer, shortcut, control-panel, driver, registration, and closely related extensions. `--include-protected` is needed even for an explicit selection.

All real apply/restore operations require `--yes`. The command prints the same operation as a dry-run when confirmation is omitted.

No registry ACLs are changed. No icon-cache files are deleted. No Explorer process is terminated automatically.

## 9. Scope and paths

User scope:

```text
registry: HKCU\Software\Classes\.ext\DefaultIcon
icons:    %LOCALAPPDATA%\FileGlyph\icons
state:    %LOCALAPPDATA%\FileGlyph\state-user.json
```

Machine scope:

```text
registry: HKLM\Software\Classes\.ext\DefaultIcon
icons:    %PROGRAMDATA%\FileGlyph\icons
state:    %LOCALAPPDATA%\FileGlyph\state-machine.json
```

The machine state is intentionally tied to the operator who made the change. A future installer may instead establish a machine-wide journal with explicit ACLs.

## 10. GUI direction

A GUI should be a thin client over the existing operations, not a second implementation. Suggested layout:

- scan table with current icon preview, generated preview, association, category, and reason;
- filters for missing/executable/generic/dedicated;
- editable category and label cells;
- batch checkbox selection;
- user/machine scope selector with elevation indicator;
- dry-run review page;
- managed-icons page backed by the state file;
- explicit restore and rescan actions.

The GUI should preserve the prototype's evidence fields. “Why was this selected?” must remain inspectable.
