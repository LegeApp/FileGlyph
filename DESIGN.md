# FileGlyph design

## 1. Product boundary

FileGlyph owns the visual identity of a **file type**, not the application association.

On Windows it writes an extension-level icon value:

```text
HKCU or HKLM\Software\Classes\.extension\DefaultIcon
    (Default) = "C:\absolute\path\extension.ico",0
```

On Linux it writes PNGs into the active XDG icon theme, under the name the
desktop looks up for the extension's MIME type:

```text
$XDG_DATA_HOME/icons/<active theme>/<size>x<size>/mimetypes/<type>.png
    e.g. ~/.local/share/icons/Adwaita/256x256/mimetypes/application-pdf.png
```

It does not write `UserChoice`, does not touch `mimeapps.list`, does not select an
application, and does not rewrite open commands. If Windows reports that a
protected effective association still bypasses the extension icon, FileGlyph
registers its per-user `IExtractIconW` handler under that effective ProgID. The
handler varies the icon by filename extension while the existing open command
remains untouched. Linux needs no such fallback: a user-scope theme file shadows
the system one, so the primary mechanism cannot be out-ranked.

The first prototype is a CLI because system discovery and restore semantics need to be observable before they are placed behind a GUI.

### 1.1 Extension versus type

The two systems key icons off different things, and this is the one visible
behavioural difference. Windows associates an icon with an *extension*; Linux
associates it with a *MIME type* shared by every extension that maps to it.

The shared code above `src/platform/` works in extensions throughout, because that
is the unit a user selects and labels. The Linux backend translates to MIME types
at the boundary. Where several selected extensions resolve to one type, the apply
path detects the collision — the platform hands back the same icon path for both —
and gives the type to the first extension, reporting the rest as
`skipped_shared_type`. Ownership is recorded in the state file so it survives
across runs. This is preferred over silently letting the last apply win, and over
refusing the whole operation, which would make `--all-candidates` unusable on a
system where dozens of extensions share `text/plain`.

## 2. Data flow

```text
platform::scan_raw_file_types ---------------------+--> RawFileType
  Windows: HKCR names + Explorer FileExts,            |
           AssocQueryStringW, direct HKCR values      |
  Linux:   shared-mime-info globs, mimeapps.list /    |
           mimeinfo.cache / desktop MimeType=,        |
           icon-theme resolution                      |
                                                      |
classification + icon assessment + safety policy ----+--> FileTypeRecord
                                                      |
explicit selection / candidate mode -----------------+--> apply set
                                                      |
shared-type collision check -------------------------+--> owned apply set
                                                      |
recovery journal --> platform::write_icon_asset -----+
  Windows: fontdue render --> ICO --> registry write  |
  Linux:   fontdue render --> PNG per size into the   |
           active theme, displaced icons backed up    |
                                                      |
                           effective check --+--> direct override
                                            +--> dynamic handler map (Windows)
                                                       |
                             SHChangeNotify | gtk-update-icon-cache
```

The journal is written *before* the system is touched, on both platforms, so a
crash mid-apply leaves a record that guarded restore can act on.

### Why use `AssocQueryStringW`

Windows associations can be indirect, merged, and influenced by per-user choices. Reimplementing all lookup rules by walking ProgID keys would be incomplete. The association API is used for the effective executable, ProgID, names, content type, and default icon; the direct extension `DefaultIcon` value is also read because its existence changes overwrite policy.

### How the Linux fields are filled

There is no association API to call, so the backend reads the specified files
directly. `RawFileType` keeps its Windows field names and the Linux backend fills
each slot with the nearest equivalent:

| Field | Linux source |
|---|---|
| `prog_id` | the MIME type — but only when a program claims it, so `associated` keeps its Windows meaning |
| `executable` | the handling desktop entry's `Exec`, field codes and launcher prefixes stripped |
| `content_type` | the MIME type, always present, since classification depends on it |
| `perceived_type` | the MIME media type mapped onto the Windows vocabulary |
| `friendly_document_name` | the untranslated `<comment>` in the type's shared-mime-info XML |
| `friendly_application_name` | the desktop entry's `Name` |
| `effective_icon` | the file the active theme resolves for the type |
| `extension_icon` | that file, when it comes from the user's own data directory — the direct analogue of an extension-level value |

### Deciding that an icon is generic

Windows can answer this from the resource path alone: an icon out of `shell32.dll`
is a shell default. Linux cannot, because one theme file serves both roles —
`text-html.png` is the dedicated icon of `text/html` and simultaneously the
fallback several other types share.

The exact test needs the type: the desktop looks up the type's own icon name
first, so landing on any *other* name means it fell back. `is_generic_system_icon`
therefore takes the content type, and the Linux backend compares the resolved
file's stem against the name the type would have used. Name-shape heuristics
(`-x-generic` and friends) remain only as the answer when no type is supplied.

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

## 7. Apply transaction and restore

For each extension:

1. read the value in the selected scope only;
2. preserve the original value from the first FileGlyph application;
3. save an unconfirmed recovery record;
4. install the icon;
5. write the scoped icon value;
6. mark the recovery record confirmed;
7. after the batch, send one association-change notification.

The read comes first because on Linux installing the icon *is* the override — a
value read afterwards would already be FileGlyph's own. Saving the recovery record
before anything is installed makes interruption recoverable. A failed write leaves
a pending record, but guarded restore will only modify the system when the current
value matches the intended FileGlyph value unless `--force` is supplied.

Reapplication does not replace the original backup with FileGlyph's own previous value.

Restore is a single platform call rather than a value write followed by a file
delete, because the correct order differs. Windows restores the registry first and
then removes the ICO, so no window exists where the value points at a deleted
file. Linux removes the installed files first and then puts back any it displaced,
since both are the same files. Linux derives the theme, the icon name and every
installed size from the path recorded at apply time, so a theme change between
apply and restore cannot strand files or aim the removal at the wrong theme.

### Displaced icons

Windows records the previous registry string and writes it back verbatim. Linux
has no string to record: the previous value is a set of image files at the same
paths FileGlyph is about to occupy. An apply therefore copies whatever it displaces
into `$XDG_DATA_HOME/FileGlyph/backup/<scope>/<icon name>/` before overwriting,
and takes the name outright by removing sizes it did not itself write. Restore
copies the originals back byte for byte. The backup is written only on the first
apply, so re-applying cannot overwrite the originals with FileGlyph's own icons.

## 8. Safety policy

Automatic application excludes executable, installer, shortcut, control-panel, driver, registration, and closely related extensions. `--include-protected` is needed even for an explicit selection.

All real apply/restore operations require `--yes`. The command prints the same operation as a dry-run when confirmation is omitted.

No registry ACLs are changed. No icon-cache files are deleted. No Explorer or file-manager process is terminated automatically. On Linux nothing outside the icon theme directories and FileGlyph's own data directory is written, and the shared-mime-info database is never modified.

## 9. Scope and paths

Windows user scope:

```text
registry: HKCU\Software\Classes\.ext\DefaultIcon
icons:    %LOCALAPPDATA%\FileGlyph\icons
state:    %LOCALAPPDATA%\FileGlyph\state-user.json
```

Windows machine scope:

```text
registry: HKLM\Software\Classes\.ext\DefaultIcon
icons:    %PROGRAMDATA%\FileGlyph\icons
state:    %LOCALAPPDATA%\FileGlyph\state-machine.json
```

Linux user scope:

```text
icons:    $XDG_DATA_HOME/icons/<active theme>/<size>x<size>/mimetypes/
state:    $XDG_DATA_HOME/FileGlyph/state-user.json
backup:   $XDG_DATA_HOME/FileGlyph/backup/user/
```

Linux machine scope:

```text
icons:    /usr/share/icons/<active theme>/<size>x<size>/mimetypes/
state:    $XDG_DATA_HOME/FileGlyph/state-machine.json
backup:   $XDG_DATA_HOME/FileGlyph/backup/machine/
```

On Linux the icons cannot live in a directory of FileGlyph's own choosing: the
desktop only finds them inside an icon theme. That is why `paths::icon_root`
returns a theme container there and the platform layer appends the theme name.

The machine state is intentionally tied to the operator who made the change. A future installer may instead establish a machine-wide journal with explicit ACLs.

### Choosing the theme

Installing into `hicolor` is not sufficient, because themes are searched before
their fallbacks and most desktops run a theme that defines its own file-type
icons. FileGlyph therefore installs into the active theme, resolved in order from
`$FILEGLYPH_ICON_THEME`, the GTK settings files, `gsettings`, and finally
`hicolor`. The environment variable exists so the smoke test and any automation
can pin the theme instead of depending on desktop state.

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
