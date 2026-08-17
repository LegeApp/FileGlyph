//! Linux backend built on the freedesktop.org shared-mime-info and icon-theme
//! specifications.
//!
//! The shape of the problem differs from Windows in one way that drives the whole
//! module: Linux desktops do not associate an icon with a file *extension*. They
//! associate it with a *MIME type*, which the shared-mime-info database derives
//! from the extension. So `.pdf` has no icon of its own — `application/pdf` does,
//! and every extension mapping to that type shares it.
//!
//! An override is therefore a file dropped into an XDG icon theme under the name
//! the desktop looks up (`application/pdf` becomes `application-pdf.png`). Because
//! `$XDG_DATA_HOME` precedes the system data directories in the theme search path,
//! a user-scope copy of a theme directory shadows the distribution's copy — that
//! is the mechanism FileGlyph uses, and it is why icons are installed into the
//! *active* theme rather than into `hicolor`: a theme that defines its own
//! `application-pdf` would otherwise win over anything placed in the fallback.

use super::RawFileType;
use crate::config::Config;
use crate::icon::IconRenderer;
use crate::model::{
    is_valid_extension, normalize_extension, normalized_expanded_path, FileTypeRecord, Scope,
};
use crate::paths;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

/// Retained so state files written by the Windows build stay readable; the
/// in-process icon handler it identifies has no Linux counterpart.
pub const FILEGLYPH_HANDLER_CLSID: &str = "{5F7A3B34-EA94-4A97-B08F-8D7DEA8CDF11}";

/// Windows needs a shell extension when a ProgID out-ranks the extension-level
/// icon. On Linux a user-scope theme file always shadows the system one, so the
/// primary mechanism cannot be out-ranked and no fallback exists.
pub const SUPPORTS_ICON_HANDLER_FALLBACK: bool = false;

/// Theme subdirectories holding file-type and application icons, per the icon
/// theme specification.
const MIMETYPES_CONTEXT: &str = "mimetypes";
const APPLICATIONS_CONTEXT: &str = "apps";

/// Icons a desktop shows for types it has nothing specific for.
const GENERIC_ICON_NAMES: [&str; 5] = [
    "unknown",
    "empty",
    "application-octet-stream",
    "application-default-icon",
    "gtk-file",
];

// ---------------------------------------------------------------------------
// XDG base directories
// ---------------------------------------------------------------------------

fn data_home() -> PathBuf {
    paths::xdg_data_home().unwrap_or_else(|_| PathBuf::from("/usr/share"))
}

/// Data directories in search order: `$XDG_DATA_HOME` first, then `$XDG_DATA_DIRS`.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![data_home()];
    push_path_list(
        &mut dirs,
        &env::var("XDG_DATA_DIRS").unwrap_or_default(),
        "/usr/local/share:/usr/share",
    );
    dirs
}

/// Config directories in search order: `$XDG_CONFIG_HOME`, then `$XDG_CONFIG_DIRS`.
fn config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        Some(path) if path.is_absolute() => dirs.push(path),
        _ => {
            if let Some(home) = dirs::home_dir() {
                dirs.push(home.join(".config"));
            }
        }
    }
    push_path_list(
        &mut dirs,
        &env::var("XDG_CONFIG_DIRS").unwrap_or_default(),
        "/etc/xdg",
    );
    dirs
}

fn push_path_list(dirs: &mut Vec<PathBuf>, raw: &str, fallback: &str) {
    let value = if raw.trim().is_empty() { fallback } else { raw };
    for entry in value.split(':').filter(|entry| !entry.is_empty()) {
        let path = PathBuf::from(entry);
        // The specification says relative entries are invalid and must be ignored.
        if path.is_absolute() && !dirs.contains(&path) {
            dirs.push(path);
        }
    }
}

/// Directories that may contain icon themes, highest priority first.
fn icon_base_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // `~/.icons` predates the base-directory spec but is still honoured by GTK.
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".icons"));
    }
    for base in data_dirs() {
        dirs.push(base.join("icons"));
    }
    dirs
}

// ---------------------------------------------------------------------------
// shared-mime-info database
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct MimeDatabase {
    /// ".pdf" -> "application/pdf"
    extension_to_mime: BTreeMap<String, String>,
    /// Deprecated type name -> canonical type name.
    aliases: HashMap<String, String>,
    /// Explicit icon name from the `icons` file, when a type declares one.
    explicit_icons: HashMap<String, String>,
    /// Fallback icon name shared by a family of types, from `generic-icons`.
    generic_icons: HashMap<String, String>,
}

impl MimeDatabase {
    fn load() -> Self {
        let mut database = Self::default();
        // Highest-priority directory first; ties resolve to whatever is already in.
        let mut weights: HashMap<String, u32> = HashMap::new();
        for base in data_dirs() {
            let mime_dir = base.join("mime");
            database.load_globs(&mime_dir, &mut weights);
            database.load_aliases(&mime_dir);
            database.load_icon_map(&mime_dir.join("icons"), false);
            database.load_icon_map(&mime_dir.join("generic-icons"), true);
        }
        database
    }

    fn load_globs(&mut self, mime_dir: &Path, weights: &mut HashMap<String, u32>) {
        // globs2 carries explicit weights; globs is the older unweighted form.
        let (path, weighted) = match mime_dir.join("globs2") {
            path if path.is_file() => (path, true),
            _ => (mime_dir.join("globs"), false),
        };
        let Ok(contents) = fs::read_to_string(&path) else {
            return;
        };

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((weight, mime, glob)) = parse_glob_line(line, weighted) else {
                continue;
            };
            let Some(extension) = extension_from_glob(&glob) else {
                continue;
            };

            // A higher weight wins outright. Equal weights keep the entry already
            // recorded, which preserves the data-directory precedence above.
            match weights.get(&extension) {
                Some(existing) if *existing >= weight => continue,
                _ => {}
            }
            weights.insert(extension.clone(), weight);
            self.extension_to_mime.insert(extension, mime);
        }
    }

    fn load_aliases(&mut self, mime_dir: &Path) {
        let Ok(contents) = fs::read_to_string(mime_dir.join("aliases")) else {
            return;
        };
        for line in contents.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(alias), Some(canonical)) = (parts.next(), parts.next()) {
                self.aliases
                    .entry(alias.to_ascii_lowercase())
                    .or_insert_with(|| canonical.to_ascii_lowercase());
            }
        }
    }

    fn load_icon_map(&mut self, path: &Path, generic: bool) {
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };
        for line in contents.lines() {
            let Some((mime, icon)) = line.trim().split_once(':') else {
                continue;
            };
            if mime.is_empty() || icon.is_empty() {
                continue;
            }
            let target = if generic {
                &mut self.generic_icons
            } else {
                &mut self.explicit_icons
            };
            target
                .entry(mime.to_ascii_lowercase())
                .or_insert_with(|| icon.to_string());
        }
    }

    fn canonical(&self, mime: &str) -> String {
        let lowered = mime.to_ascii_lowercase();
        self.aliases.get(&lowered).cloned().unwrap_or(lowered)
    }

    /// Icon names the desktop tries, in order, for a MIME type.
    fn icon_names(&self, mime: &str) -> Vec<String> {
        let mime = self.canonical(mime);
        let mut names = Vec::new();
        if let Some(explicit) = self.explicit_icons.get(&mime) {
            names.push(explicit.clone());
        }
        names.push(mime.replace('/', "-"));
        if let Some(generic) = self.generic_icons.get(&mime) {
            names.push(generic.clone());
        }
        if let Some((media, _)) = mime.split_once('/') {
            names.push(format!("{media}-x-generic"));
        }
        names.dedup();
        names
    }

    /// The name an override must use to be found first.
    fn primary_icon_name(&self, mime: &str) -> String {
        let mime = self.canonical(mime);
        self.explicit_icons
            .get(&mime)
            .cloned()
            .unwrap_or_else(|| mime.replace('/', "-"))
    }
}

/// Split `weight:mime:glob` (globs2) or `mime:glob` (globs).
fn parse_glob_line(line: &str, weighted: bool) -> Option<(u32, String, String)> {
    if weighted {
        let mut parts = line.splitn(3, ':');
        let weight = parts.next()?.trim().parse::<u32>().ok()?;
        let mime = parts.next()?.trim().to_ascii_lowercase();
        let rest = parts.next()?;
        // A trailing ":cs" marks the pattern case-sensitive; it is not part of it.
        let glob = rest.strip_suffix(":cs").unwrap_or(rest).trim().to_string();
        (!mime.is_empty() && !glob.is_empty()).then_some((weight, mime, glob))
    } else {
        let (mime, glob) = line.split_once(':')?;
        let mime = mime.trim().to_ascii_lowercase();
        let glob = glob.trim().to_string();
        (!mime.is_empty() && !glob.is_empty()).then_some((50, mime, glob))
    }
}

/// Reduce a glob to a plain extension, ignoring patterns that are not `*.ext`.
///
/// Whole-name patterns like `Makefile` and multi-part ones like `*.tar.gz` have no
/// single-extension equivalent, so they are skipped rather than approximated.
fn extension_from_glob(glob: &str) -> Option<String> {
    let rest = glob.strip_prefix("*.")?;
    if rest.is_empty() || rest.contains(['*', '?', '[', ']', '/']) {
        return None;
    }
    let normalized = normalize_extension(rest);
    is_valid_extension(&normalized).then_some(normalized)
}

fn mime_database() -> &'static MimeDatabase {
    static DATABASE: OnceLock<MimeDatabase> = OnceLock::new();
    DATABASE.get_or_init(MimeDatabase::load)
}

/// Human-readable type name from `<data>/mime/<media>/<subtype>.xml`.
fn mime_comment(mime: &str) -> Option<String> {
    let (media, subtype) = mime.split_once('/')?;
    for base in data_dirs() {
        let path = base.join("mime").join(media).join(format!("{subtype}.xml"));
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(comment) = first_untranslated_comment(&contents) {
            return Some(comment);
        }
    }
    None
}

/// Pull the C-locale `<comment>` out of a shared-mime-info type description.
///
/// Translations carry an `xml:lang` attribute; the untranslated element does not,
/// so it is the one that opens with a bare `>`.
fn first_untranslated_comment(xml: &str) -> Option<String> {
    let mut rest = xml;
    while let Some(start) = rest.find("<comment>") {
        let after = &rest[start + "<comment>".len()..];
        let end = after.find("</comment>")?;
        let value = decode_xml_entities(&after[..end]).trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
        rest = &after[end..];
    }
    None
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

// ---------------------------------------------------------------------------
// Desktop entries and default applications
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct DesktopEntry {
    name: Option<String>,
    executable: Option<String>,
    icon: Option<String>,
}

#[derive(Debug, Default)]
struct ApplicationDatabase {
    /// MIME type -> desktop file id of the chosen handler.
    default_for_mime: HashMap<String, String>,
    entries: HashMap<String, DesktopEntry>,
}

impl ApplicationDatabase {
    fn load() -> Self {
        let mut database = Self::default();
        let mut removed: HashMap<String, HashSet<String>> = HashMap::new();

        for path in mimeapps_files() {
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            let sections = parse_desktop_file(&contents);
            if let Some(entries) = sections.get("Removed Associations") {
                for (mime, ids) in entries {
                    removed
                        .entry(mime.to_ascii_lowercase())
                        .or_default()
                        .extend(split_desktop_list(ids));
                }
            }
            if let Some(entries) = sections.get("Default Applications") {
                database.record_defaults(entries, &removed);
            }
        }

        // Types with no explicit default still have registered handlers; the cache
        // built by update-desktop-database lists them.
        for base in data_dirs() {
            let path = base.join("applications").join("mimeinfo.cache");
            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(entries) = parse_desktop_file(&contents).get("MIME Cache") {
                database.record_defaults(entries, &removed);
            }
        }

        database.load_declared_associations(&removed);
        database
    }

    /// Read `MimeType=` straight out of the installed desktop entries.
    ///
    /// The cache above is generated by `update-desktop-database`, which is not
    /// guaranteed to have been run — on a container or a hand-built image it is
    /// often missing entirely. The declarations in the entries themselves are the
    /// source that cache is built from, so they fill the same role.
    fn load_declared_associations(&mut self, removed: &HashMap<String, HashSet<String>>) {
        for base in data_dirs() {
            let directory = base.join("applications");
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            let mut ids: Vec<String> = entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    name.ends_with(".desktop").then_some(name)
                })
                .collect();
            // Sorted so the handler chosen for a type does not depend on readdir order.
            ids.sort();

            for id in ids {
                let Some(path) = find_desktop_file(&id) else {
                    continue;
                };
                let Ok(contents) = fs::read_to_string(&path) else {
                    continue;
                };
                let sections = parse_desktop_file(&contents);
                let Some(section) = sections.get("Desktop Entry") else {
                    continue;
                };
                // `Hidden` means the entry is to be treated as though it did not
                // exist. `NoDisplay` only keeps it out of menus — the spec names
                // MIME association as the reason to set it, so those entries are
                // exactly the handlers this lookup is for.
                if is_true(section.get("Hidden")) {
                    continue;
                }
                let Some(declared) = section.get("MimeType") else {
                    continue;
                };
                for mime in split_desktop_list(declared) {
                    let mime = mime.to_ascii_lowercase();
                    if self.default_for_mime.contains_key(&mime)
                        || removed.get(&mime).is_some_and(|set| set.contains(&id))
                    {
                        continue;
                    }
                    if self.load_entry(&id).is_some() {
                        self.default_for_mime.insert(mime, id.clone());
                    }
                }
            }
        }
    }

    fn record_defaults(
        &mut self,
        entries: &BTreeMap<String, String>,
        removed: &HashMap<String, HashSet<String>>,
    ) {
        for (mime, ids) in entries {
            let mime = mime.to_ascii_lowercase();
            if self.default_for_mime.contains_key(&mime) {
                continue;
            }
            let blocked = removed.get(&mime);
            let chosen = split_desktop_list(ids).into_iter().find(|id| {
                !blocked.is_some_and(|set| set.contains(id)) && self.load_entry(id).is_some()
            });
            if let Some(id) = chosen {
                self.default_for_mime.insert(mime, id);
            }
        }
    }

    /// Read and cache a desktop entry, returning None when the file is absent.
    fn load_entry(&mut self, id: &str) -> Option<&DesktopEntry> {
        if !self.entries.contains_key(id) {
            let entry = find_desktop_file(id).and_then(|path| read_desktop_entry(&path))?;
            self.entries.insert(id.to_string(), entry);
        }
        self.entries.get(id)
    }

    fn entry_for_mime(&self, mime: &str) -> Option<&DesktopEntry> {
        let id = self.default_for_mime.get(&mime.to_ascii_lowercase())?;
        self.entries.get(id)
    }
}

fn application_database() -> &'static ApplicationDatabase {
    static DATABASE: OnceLock<ApplicationDatabase> = OnceLock::new();
    DATABASE.get_or_init(ApplicationDatabase::load)
}

/// mimeapps.list locations in the precedence order given by the association spec.
fn mimeapps_files() -> Vec<PathBuf> {
    let desktops: Vec<String> = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_ascii_lowercase())
        .collect();

    let mut files = Vec::new();
    let push_dir = |dir: PathBuf, files: &mut Vec<PathBuf>| {
        for desktop in &desktops {
            files.push(dir.join(format!("{desktop}-mimeapps.list")));
        }
        files.push(dir.join("mimeapps.list"));
    };

    for dir in config_dirs() {
        push_dir(dir, &mut files);
    }
    for base in data_dirs() {
        push_dir(base.join("applications"), &mut files);
    }
    files
}

/// Parse the INI-like desktop entry format into `section -> key -> value`.
fn parse_desktop_file(contents: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current = String::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.to_string();
            sections.entry(current.clone()).or_default();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(current.clone())
                .or_default()
                .insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    sections
}

/// Desktop entry booleans are the literal strings `true` and `false`.
fn is_true(value: Option<&String>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn split_desktop_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// Locate a desktop file by id across the application directories.
fn find_desktop_file(id: &str) -> Option<PathBuf> {
    // Ids are file names; reject anything that could escape the directory.
    if id.contains('/') || id.contains("..") || !id.ends_with(".desktop") {
        return None;
    }
    for base in data_dirs() {
        let applications = base.join("applications");
        let direct = applications.join(id);
        if direct.is_file() {
            return Some(direct);
        }
        // Ids from subdirectories encode the separator as '-', per the menu spec.
        if let Some((head, tail)) = id.split_once('-') {
            let nested = applications.join(head).join(tail);
            if nested.is_file() {
                return Some(nested);
            }
        }
    }
    None
}

fn read_desktop_entry(path: &Path) -> Option<DesktopEntry> {
    let contents = fs::read_to_string(path).ok()?;
    let sections = parse_desktop_file(&contents);
    let entry = sections.get("Desktop Entry")?;
    Some(DesktopEntry {
        name: entry.get("Name").cloned().filter(|v| !v.is_empty()),
        executable: entry
            .get("TryExec")
            .or_else(|| entry.get("Exec"))
            .and_then(|value| executable_from_exec(value)),
        icon: entry.get("Icon").cloned().filter(|v| !v.is_empty()),
    })
}

/// Extract the program from an `Exec=` line and resolve it to an absolute path.
///
/// Exec values carry field codes (`%f`, `%U`) and may be prefixed with `env` or a
/// sandbox launcher; only the leading program name is of interest here.
fn executable_from_exec(value: &str) -> Option<String> {
    let mut tokens = split_exec_tokens(value).into_iter();
    let mut program = tokens.next()?;
    if program == "env" || program.ends_with("/env") {
        // Skip VAR=value assignments to reach the real program.
        program = tokens.find(|token| !token.contains('='))?;
    }
    if program.is_empty() || program.starts_with('%') {
        return None;
    }
    Some(resolve_program(&program))
}

fn split_exec_tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in value.chars() {
        match (quote, ch) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"') | (None, '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (None, c) => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Turn a bare program name into an absolute path using `$PATH`.
fn resolve_program(program: &str) -> String {
    if program.contains('/') {
        return program.to_string();
    }
    let path = env::var("PATH").unwrap_or_default();
    for dir in path.split(':').filter(|dir| !dir.is_empty()) {
        let candidate = Path::new(dir).join(program);
        if candidate.is_file() {
            return candidate.display().to_string();
        }
    }
    program.to_string()
}

// ---------------------------------------------------------------------------
// Icon themes
// ---------------------------------------------------------------------------

/// Which kind of icon a theme subdirectory holds. File types and applications are
/// kept apart so a program named like a MIME type cannot answer a type lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum IconContext {
    MimeTypes,
    Applications,
}

fn icon_context(directory: &str) -> Option<IconContext> {
    match directory {
        MIMETYPES_CONTEXT => Some(IconContext::MimeTypes),
        APPLICATIONS_CONTEXT => Some(IconContext::Applications),
        _ => None,
    }
}

/// Icons available in one theme directory, keyed by context and icon name.
#[derive(Debug, Default)]
struct ThemeIndex {
    icons: HashMap<(IconContext, String), PathBuf>,
}

/// The active icon theme, its inheritance chain, and a cache of built indexes.
struct IconThemes {
    chain: Vec<String>,
    indexes: Mutex<HashMap<(PathBuf, String), Arc<ThemeIndex>>>,
}

fn icon_themes() -> &'static IconThemes {
    static THEMES: OnceLock<IconThemes> = OnceLock::new();
    THEMES.get_or_init(|| IconThemes {
        chain: build_theme_chain(),
        indexes: Mutex::new(HashMap::new()),
    })
}

/// Drop cached theme contents after FileGlyph adds or removes icon files.
fn invalidate_icon_index() {
    if let Ok(mut indexes) = icon_themes().indexes.lock() {
        indexes.clear();
    }
}

/// Name of the theme the desktop is currently displaying.
pub fn active_icon_theme() -> String {
    static THEME: OnceLock<String> = OnceLock::new();
    THEME
        .get_or_init(|| {
            // An explicit override first: it is also how the test suite pins a theme.
            if let Ok(value) = env::var("FILEGLYPH_ICON_THEME") {
                if !value.trim().is_empty() {
                    return value.trim().to_string();
                }
            }
            gtk_settings_icon_theme()
                .or_else(gsettings_icon_theme)
                .unwrap_or_else(|| "hicolor".to_string())
        })
        .clone()
}

fn gtk_settings_icon_theme() -> Option<String> {
    let mut candidates = Vec::new();
    for dir in config_dirs() {
        candidates.push(dir.join("gtk-4.0").join("settings.ini"));
        candidates.push(dir.join("gtk-3.0").join("settings.ini"));
    }
    for path in candidates {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(settings) = parse_desktop_file(&contents).get("Settings") {
            if let Some(theme) = settings.get("gtk-icon-theme-name") {
                let theme = theme.trim().trim_matches('"');
                if !theme.is_empty() {
                    return Some(theme.to_string());
                }
            }
        }
    }
    None
}

fn gsettings_icon_theme() -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string();
    (!value.is_empty()).then_some(value)
}

/// Active theme followed by its parents, always ending at `hicolor`.
fn build_theme_chain() -> Vec<String> {
    let mut chain = Vec::new();
    let mut pending = vec![active_icon_theme()];

    while let Some(theme) = pending.pop() {
        if chain.contains(&theme) {
            continue;
        }
        // Guard against a malformed index.theme cycling forever.
        if chain.len() > 16 {
            break;
        }
        chain.push(theme.clone());
        let mut parents = theme_parents(&theme);
        parents.reverse();
        pending.extend(parents);
    }

    if !chain.iter().any(|theme| theme == "hicolor") {
        chain.push("hicolor".to_string());
    }
    chain
}

fn theme_parents(theme: &str) -> Vec<String> {
    for base in icon_base_dirs() {
        let path = base.join(theme).join("index.theme");
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(section) = parse_desktop_file(&contents).get("Icon Theme") {
            if let Some(inherits) = section.get("Inherits") {
                return inherits
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();
            }
        }
    }
    Vec::new()
}

impl IconThemes {
    fn index(&self, base: &Path, theme: &str) -> Arc<ThemeIndex> {
        let key = (base.to_path_buf(), theme.to_string());
        if let Ok(indexes) = self.indexes.lock() {
            if let Some(index) = indexes.get(&key) {
                return Arc::clone(index);
            }
        }
        let index = Arc::new(index_theme_dir(&base.join(theme)));
        if let Ok(mut indexes) = self.indexes.lock() {
            indexes.insert(key, Arc::clone(&index));
        }
        index
    }

    /// Resolve an icon name the way the desktop would: the best candidate name in
    /// the highest-priority theme that provides it.
    fn lookup(&self, names: &[String], bases: &[PathBuf], context: IconContext) -> Option<PathBuf> {
        for name in names {
            let key = (context, name.clone());
            for theme in &self.chain {
                for base in bases {
                    if let Some(path) = self.index(base, theme).icons.get(&key) {
                        return Some(path.clone());
                    }
                }
            }
        }
        None
    }
}

/// Catalogue the file-type and application icons in one theme directory.
///
/// Themes nest as `<theme>/<size>/<context>/` or `<theme>/<context>/<size>/`, so
/// both orderings are accepted. The largest raster wins; SVG is a last resort
/// because FileGlyph compares against the PNGs it installs.
fn index_theme_dir(theme_dir: &Path) -> ThemeIndex {
    let mut index = ThemeIndex::default();
    let mut best_rank: HashMap<(IconContext, String), (u8, u32)> = HashMap::new();

    let Ok(outer) = fs::read_dir(theme_dir) else {
        return index;
    };
    for outer_entry in outer.flatten() {
        let outer_name = outer_entry.file_name().to_string_lossy().to_string();
        // Monochrome variants are never the icon a file manager shows for a type.
        if outer_name.starts_with("symbolic") || !outer_entry.path().is_dir() {
            continue;
        }
        let Ok(inner) = fs::read_dir(outer_entry.path()) else {
            continue;
        };
        for inner_entry in inner.flatten() {
            let inner_name = inner_entry.file_name().to_string_lossy().to_string();
            if !inner_entry.path().is_dir() {
                continue;
            }
            let (context, size) = if let Some(context) = icon_context(&inner_name) {
                (context, parse_theme_size(&outer_name))
            } else if let Some(context) = icon_context(&outer_name) {
                (context, parse_theme_size(&inner_name))
            } else {
                continue;
            };
            collect_icon_files(
                &inner_entry.path(),
                context,
                size,
                &mut index,
                &mut best_rank,
            );
        }
    }
    index
}

/// Pixel size encoded in a theme subdirectory name (`48x48`, `48`, `scalable`).
fn parse_theme_size(name: &str) -> u32 {
    if name == "scalable" {
        return 0;
    }
    let head = name.split(['x', '@']).next().unwrap_or(name);
    head.parse::<u32>().unwrap_or(0)
}

fn collect_icon_files(
    dir: &Path,
    context: IconContext,
    size: u32,
    index: &mut ThemeIndex,
    best_rank: &mut HashMap<(IconContext, String), (u8, u32)>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        // Rank raster above vector so comparisons see the same kind of file
        // FileGlyph writes.
        let format_rank = match extension.to_ascii_lowercase().as_str() {
            "png" => 2_u8,
            "xpm" => 1,
            "svg" | "svgz" => 0,
            _ => continue,
        };
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if stem.ends_with("-symbolic") {
            continue;
        }

        let rank = (format_rank, size);
        let key = (context, stem.to_string());
        match best_rank.get(&key) {
            Some(existing) if *existing >= rank => continue,
            _ => {}
        }
        best_rank.insert(key.clone(), rank);
        index.icons.insert(key, path);
    }
}

// ---------------------------------------------------------------------------
// Icon locations for a scope
// ---------------------------------------------------------------------------

/// Theme directory FileGlyph installs into for a scope.
fn scope_theme_dir(scope: Scope) -> Result<PathBuf> {
    Ok(paths::icon_root(scope)?.join(active_icon_theme()))
}

fn icon_file_for_size(theme_dir: &Path, size: u32, icon_name: &str) -> PathBuf {
    theme_dir
        .join(format!("{size}x{size}"))
        .join(MIMETYPES_CONTEXT)
        .join(format!("{icon_name}.png"))
}

fn icon_name_for_extension(extension: &str) -> Result<String> {
    let extension = normalize_extension(extension);
    let mime = mime_for_extension(&extension).with_context(|| {
        format!("{extension} has no MIME type in the shared-mime-info database")
    })?;
    Ok(mime_database().primary_icon_name(&mime))
}

fn mime_for_extension(extension: &str) -> Option<String> {
    mime_database()
        .extension_to_mime
        .get(&normalize_extension(extension))
        .cloned()
}

/// Every installed override file for an icon name inside one theme directory,
/// largest size first.
fn installed_icon_files(theme_dir: &Path, icon_name: &str) -> Vec<(u32, PathBuf)> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(theme_dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry
            .path()
            .join(MIMETYPES_CONTEXT)
            .join(format!("{icon_name}.png"));
        if path.is_file() {
            found.push((parse_theme_size(&name), path));
        }
    }
    found.sort_by(|left, right| right.0.cmp(&left.0));
    found
}

fn backup_dir(scope: Scope, icon_name: &str) -> Result<PathBuf> {
    Ok(paths::icon_backup_root(scope)?.join(icon_name))
}

// ---------------------------------------------------------------------------
// Public platform API
// ---------------------------------------------------------------------------

/// Value recorded as the applied override. Unlike Windows there is no
/// `resource,index` form: the desktop resolves a theme name to a single file.
pub fn registry_reference(icon_path: &Path) -> String {
    icon_path.display().to_string()
}

pub fn scan_raw_file_types() -> Result<Vec<RawFileType>> {
    let mimes = mime_database();
    let applications = application_database();
    let bases = icon_base_dirs();
    let user_bases = vec![data_home().join("icons")];
    let mut comments: HashMap<String, Option<String>> = HashMap::new();
    let mut records = Vec::with_capacity(mimes.extension_to_mime.len());

    for (extension, mime) in &mimes.extension_to_mime {
        let mime = mimes.canonical(mime);
        let names = mimes.icon_names(&mime);
        let entry = applications.entry_for_mime(&mime);

        let effective_icon = icon_themes()
            .lookup(&names, &bases, IconContext::MimeTypes)
            .map(|path| path.display().to_string());
        // An override is simply an icon supplied from the user's own data
        // directory; that is the direct analogue of an extension-level value.
        let extension_icon = icon_themes()
            .lookup(&names, &user_bases, IconContext::MimeTypes)
            .map(|path| path.display().to_string());

        let friendly_document_name = comments
            .entry(mime.clone())
            .or_insert_with(|| mime_comment(&mime))
            .clone();

        records.push(RawFileType {
            // Having a MIME type is not an association: the shared database
            // describes types the system can *name*, while `prog_id` has to mean
            // the same thing it does on Windows — some installed program claims
            // this type. Only then does the shared code treat it as associated.
            prog_id: entry.is_some().then(|| mime.clone()),
            executable: entry.and_then(|entry| entry.executable.clone()),
            friendly_document_name,
            friendly_application_name: entry.and_then(|entry| entry.name.clone()),
            perceived_type: perceived_type_for(&mime),
            // The type itself is always known, and classification depends on it.
            content_type: Some(mime),
            extension: extension.clone(),
            extension_icon,
            effective_icon,
        });
    }

    Ok(records)
}

/// Map a MIME media type onto the vocabulary the classifier shares with Windows.
fn perceived_type_for(mime: &str) -> Option<String> {
    let media = mime.split('/').next()?;
    Some(match media {
        "text" | "image" | "video" | "audio" => media.to_string(),
        "inode" => "system".to_string(),
        _ => return None,
    })
}

/// Whether a file type is merely showing the icon of the application that opens it.
///
/// Windows points such a type straight at the `.exe`. The Linux equivalent is
/// indirect: the type resolves to the very same theme icon the application's
/// desktop entry declares, which is what a launcher shows for the program itself.
pub fn icon_belongs_to_application(icon: &str, executable: Option<&str>) -> bool {
    let Some(executable) = executable else {
        return false;
    };
    let Some(application_icon) = application_icon_path(executable) else {
        return false;
    };
    normalized_expanded_path(icon) == normalized_expanded_path(&application_icon)
}

/// Resolve the icon of the desktop entry whose program is `executable`.
fn application_icon_path(executable: &str) -> Option<String> {
    static ICONS: OnceLock<HashMap<String, String>> = OnceLock::new();
    let by_executable = ICONS.get_or_init(|| {
        let bases = icon_base_dirs();
        let mut resolved = HashMap::new();
        for entry in application_database().entries.values() {
            let (Some(program), Some(icon)) = (&entry.executable, &entry.icon) else {
                continue;
            };
            // An absolute Icon= is used verbatim; a bare name is a theme lookup.
            let path = if icon.contains('/') {
                Some(PathBuf::from(icon))
            } else {
                icon_themes().lookup(
                    std::slice::from_ref(icon),
                    &bases,
                    IconContext::Applications,
                )
            };
            if let Some(path) = path {
                resolved
                    .entry(program.clone())
                    .or_insert_with(|| path.display().to_string());
            }
        }
        resolved
    });
    by_executable.get(executable).cloned()
}

/// Whether the resolved icon is a shared fallback rather than this type's own.
///
/// Given the MIME type the answer is exact: the desktop looks up the type's own
/// icon name first, so landing on any other name means it fell back. That
/// distinction cannot be made from the file name alone — `text-html` is the
/// dedicated icon of `text/html` and simultaneously the fallback that several
/// other types share.
pub fn is_generic_system_icon(icon_path: &str, content_type: Option<&str>) -> bool {
    let stem = Path::new(icon_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();

    match content_type {
        Some(mime) => mime_database().primary_icon_name(mime) != stem,
        // Without a type to compare against, fall back to the naming convention
        // every freedesktop theme uses for its shared icons.
        None => stem.ends_with("-x-generic") || GENERIC_ICON_NAMES.contains(&stem),
    }
}

pub fn read_scoped_icon(scope: Scope, extension: &str) -> Result<Option<String>> {
    let icon_name = icon_name_for_extension(extension)?;
    let theme_dir = scope_theme_dir(scope)?;
    Ok(installed_icon_files(&theme_dir, &icon_name)
        .first()
        .map(|(_, path)| path.display().to_string()))
}

/// No-op: on Linux the icon files written by `write_icon_asset` *are* the
/// override, so by the time apply reaches this step the value is already in place.
/// The Windows backend needs a separate registry write here, hence the shared step.
pub fn write_scoped_icon(_scope: Scope, _extension: &str, _value: &str) -> Result<()> {
    Ok(())
}

/// Drop the override for an extension in the active theme and put back whatever
/// the apply displaced.
pub fn remove_scoped_icon(scope: Scope, extension: &str) -> Result<()> {
    let icon_name = icon_name_for_extension(extension)?;
    let theme_dir = scope_theme_dir(scope)?;
    remove_installed_icons(&theme_dir, &icon_name)?;
    restore_displaced_icons(scope, &theme_dir, &icon_name)?;
    invalidate_icon_index();
    Ok(())
}

/// Undo one applied extension.
///
/// Everything is derived from the path recorded at apply time rather than from
/// the current environment, so switching desktop themes in between cannot strand
/// the installed files or aim the removal at the wrong theme.
pub fn restore_scoped_icon(
    scope: Scope,
    _extension: &str,
    _previous: Option<&str>,
    recorded_icon_path: &str,
) -> Result<()> {
    let path = Path::new(recorded_icon_path);
    // <theme>/<size>/mimetypes/<name>.png
    let Some(theme_dir) = path.parent().and_then(Path::parent).and_then(Path::parent) else {
        return Ok(());
    };
    let Some(icon_name) = path.file_stem().and_then(|value| value.to_str()) else {
        return Ok(());
    };

    remove_installed_icons(theme_dir, icon_name)?;
    restore_displaced_icons(scope, theme_dir, icon_name)?;
    invalidate_icon_index();
    Ok(())
}

fn remove_installed_icons(theme_dir: &Path, icon_name: &str) -> Result<()> {
    for (_, path) in installed_icon_files(theme_dir, icon_name) {
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove icon {}", path.display()))?;
    }
    Ok(())
}

fn restore_displaced_icons(scope: Scope, theme_dir: &Path, icon_name: &str) -> Result<()> {
    let backup = backup_dir(scope, icon_name)?;
    if !backup.is_dir() {
        // Nothing was displaced, so removing FileGlyph's files was the whole undo.
        return Ok(());
    }
    restore_backup(&backup, theme_dir, icon_name)?;
    fs::remove_dir_all(&backup).ok();
    Ok(())
}

fn restore_backup(backup: &Path, theme_dir: &Path, icon_name: &str) -> Result<()> {
    let entries = fs::read_dir(backup)
        .with_context(|| format!("failed to read backup {}", backup.display()))?;
    for entry in entries.flatten() {
        let source = entry.path();
        let Some(size_dir) = source.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let destination = theme_dir
            .join(size_dir)
            .join(MIMETYPES_CONTEXT)
            .join(format!("{icon_name}.png"));
        paths::ensure_parent(&destination)?;
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "failed to restore {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }
    Ok(())
}

/// Path recorded for an applied icon: the largest size FileGlyph writes.
pub fn icon_asset_path(config: &Config, scope: Scope, record: &FileTypeRecord) -> Result<PathBuf> {
    let icon_name = icon_name_for_extension(&record.extension)?;
    Ok(icon_file_for_size(
        &scope_theme_dir(scope)?,
        config.largest_icon_size(),
        &icon_name,
    ))
}

/// Render the icon into the scope's theme at every configured size.
///
/// Any icons already occupying those names are copied aside first, so FileGlyph
/// owns the name outright while applied and restore can return the originals.
pub fn write_icon_asset(
    renderer: &IconRenderer,
    scope: Scope,
    record: &FileTypeRecord,
) -> Result<()> {
    let icon_name = icon_name_for_extension(&record.extension)?;
    let theme_dir = scope_theme_dir(scope)?;
    ensure_scope_writable(scope, &theme_dir)?;

    let existing = installed_icon_files(&theme_dir, &icon_name);
    let backup = backup_dir(scope, &icon_name)?;
    // Only the first apply displaces anything; a re-apply must not overwrite the
    // backup with FileGlyph's own icons.
    if !existing.is_empty() && !backup.exists() {
        for (size, path) in &existing {
            let destination = backup.join(format!("{size}x{size}.png"));
            paths::ensure_parent(&destination)?;
            fs::copy(path, &destination).with_context(|| {
                format!(
                    "failed to back up {} to {}",
                    path.display(),
                    destination.display()
                )
            })?;
        }
    }
    for (_, path) in &existing {
        fs::remove_file(path)
            .with_context(|| format!("failed to replace icon {}", path.display()))?;
    }

    for size in renderer.sizes() {
        let path = icon_file_for_size(&theme_dir, size, &icon_name);
        renderer.write_png(&path, size, &record.label, record.category)?;
    }

    write_theme_index(&theme_dir)?;
    invalidate_icon_index();
    Ok(())
}

fn ensure_scope_writable(scope: Scope, theme_dir: &Path) -> Result<()> {
    if let Err(error) = fs::create_dir_all(theme_dir) {
        if scope == Scope::Machine {
            bail!(
                "machine scope writes to {} and needs root: {error}",
                theme_dir.display()
            );
        }
        return Err(error).with_context(|| format!("failed to create {}", theme_dir.display()));
    }
    Ok(())
}

/// Give a FileGlyph-created theme directory the index.theme every icon theme
/// needs, so the desktop and `gtk-update-icon-cache` treat it as a real theme.
fn write_theme_index(theme_dir: &Path) -> Result<()> {
    let path = theme_dir.join("index.theme");
    if path.exists() {
        return Ok(());
    }
    let theme = active_icon_theme();
    let contents = format!(
        "[Icon Theme]\n\
         Name={theme}\n\
         Comment=User icon overrides\n\
         Directories=\n\
         Hidden=true\n"
    );
    fs::write(&path, contents).with_context(|| format!("failed to write {}", path.display()))
}

pub fn query_effective_icon(extension: &str) -> Option<String> {
    let mime = mime_for_extension(extension)?;
    let names = mime_database().icon_names(&mime);
    icon_themes()
        .lookup(&names, &icon_base_dirs(), IconContext::MimeTypes)
        .map(|path| path.display().to_string())
}

/// Rebuild the desktop's icon caches so a file manager picks the change up.
pub fn notify_association_changed() {
    invalidate_icon_index();

    for scope in [Scope::User, Scope::Machine] {
        let Ok(theme_dir) = scope_theme_dir(scope) else {
            continue;
        };
        if !theme_dir.is_dir() {
            continue;
        }
        // Best effort throughout: a headless or minimal host has neither tool, and
        // desktops that read the directory directly do not need either.
        let _ = Command::new("gtk-update-icon-cache")
            .args(["--force", "--quiet", "--ignore-theme-index"])
            .arg(&theme_dir)
            .output();
    }

    let _ = Command::new("xdg-icon-resource")
        .arg("forceupdate")
        .output();
}

pub fn has_fileglyph_icon_handler(_prog_id: Option<&str>, _extension: &str) -> bool {
    // A user-scope theme file cannot be out-ranked, so the shell-extension
    // fallback the Windows build needs has no Linux counterpart.
    false
}

// ---------------------------------------------------------------------------
// Windows-only mechanisms
//
// The ProgID icon-handler path is never entered on Linux, because
// SUPPORTS_ICON_HANDLER_FALLBACK gates it off. These keep the platform surface
// uniform and fail loudly rather than silently doing nothing.
// ---------------------------------------------------------------------------

fn no_icon_handler<T>(operation: &str) -> Result<T> {
    bail!("{operation} is a Windows icon-handler operation with no Linux equivalent")
}

pub fn read_prog_id_default_icon(_prog_id: &str) -> Result<Option<String>> {
    no_icon_handler("reading a ProgID default icon")
}
pub fn read_prog_id_icon_handler(_prog_id: &str) -> Result<Option<String>> {
    no_icon_handler("reading a ProgID icon handler")
}
pub fn read_effective_prog_id_default_icon(_prog_id: &str) -> Option<String> {
    None
}
pub fn read_effective_prog_id_icon_handler(_prog_id: &str) -> Option<String> {
    None
}
pub fn write_prog_id_handler(_prog_id: &str) -> Result<()> {
    no_icon_handler("installing a ProgID icon handler")
}
pub fn restore_prog_id_handler(
    _prog_id: &str,
    _default_icon: Option<&str>,
    _icon_handler: Option<&str>,
) -> Result<()> {
    no_icon_handler("restoring a ProgID icon handler")
}
pub fn set_icon_mapping(_extension: &str, _icon_path: &Path) -> Result<()> {
    no_icon_handler("writing an icon-handler mapping")
}
pub fn remove_icon_mapping(_extension: &str) -> Result<()> {
    no_icon_handler("removing an icon-handler mapping")
}
pub fn set_prog_id_fallback(_prog_id: &str, _value: &str) -> Result<()> {
    no_icon_handler("writing a ProgID fallback icon")
}
pub fn remove_prog_id_fallback(_prog_id: &str) -> Result<()> {
    no_icon_handler("removing a ProgID fallback icon")
}
pub fn install_handler_dll(_source: &Path, _destination: &Path) -> Result<PathBuf> {
    no_icon_handler("installing the icon-handler library")
}
pub fn unregister_handler_clsid() -> Result<()> {
    no_icon_handler("unregistering the icon-handler class")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_extensions_from_weighted_globs() {
        assert_eq!(
            parse_glob_line("80:text/html:*.html", true),
            Some((80, "text/html".to_string(), "*.html".to_string()))
        );
        // The case-sensitivity flag is not part of the pattern.
        assert_eq!(
            parse_glob_line("50:text/x-c:*.C:cs", true),
            Some((50, "text/x-c".to_string(), "*.C".to_string()))
        );
        assert_eq!(
            parse_glob_line("text/plain:*.txt", false),
            Some((50, "text/plain".to_string(), "*.txt".to_string()))
        );
    }

    #[test]
    fn keeps_only_single_extension_globs() {
        assert_eq!(extension_from_glob("*.pdf").as_deref(), Some(".pdf"));
        assert_eq!(extension_from_glob("*.TXT").as_deref(), Some(".txt"));
        // Whole-name and multi-part patterns have no single-extension form.
        assert_eq!(extension_from_glob("Makefile"), None);
        assert_eq!(extension_from_glob("*.tar.[gx]z"), None);
        assert_eq!(extension_from_glob("*.[0-9]"), None);
    }

    #[test]
    fn derives_icon_names_in_lookup_order() {
        let mut database = MimeDatabase::default();
        database
            .generic_icons
            .insert("application/x-cd-image".into(), "media-optical".into());
        let names = database.icon_names("application/x-cd-image");
        assert_eq!(
            names,
            vec![
                "application-x-cd-image".to_string(),
                "media-optical".to_string(),
                "application-x-generic".to_string(),
            ]
        );
        assert_eq!(
            database.primary_icon_name("application/x-cd-image"),
            "application-x-cd-image"
        );
    }

    #[test]
    fn explicit_icon_name_takes_precedence() {
        let mut database = MimeDatabase::default();
        database
            .explicit_icons
            .insert("application/pdf".into(), "gnome-mime-pdf".into());
        assert_eq!(
            database.primary_icon_name("application/pdf"),
            "gnome-mime-pdf"
        );
        assert_eq!(database.icon_names("application/pdf")[0], "gnome-mime-pdf");
    }

    #[test]
    fn resolves_aliases_to_canonical_types() {
        let mut database = MimeDatabase::default();
        database
            .aliases
            .insert("application/acrobat".into(), "application/pdf".into());
        assert_eq!(database.canonical("application/ACROBAT"), "application/pdf");
        assert_eq!(
            database.primary_icon_name("application/acrobat"),
            "application-pdf"
        );
    }

    #[test]
    fn strips_field_codes_and_launchers_from_exec() {
        assert_eq!(
            executable_from_exec("/usr/bin/gedit %U").as_deref(),
            Some("/usr/bin/gedit")
        );
        assert_eq!(
            executable_from_exec("\"/opt/my app/bin/viewer\" %f").as_deref(),
            Some("/opt/my app/bin/viewer")
        );
        assert_eq!(
            executable_from_exec("env GDK_BACKEND=x11 /usr/bin/inkscape %f").as_deref(),
            Some("/usr/bin/inkscape")
        );
    }

    #[test]
    fn parses_sections_of_the_desktop_entry_format() {
        let parsed = parse_desktop_file(
            "# comment\n[Desktop Entry]\nName=Text Editor\nExec=gedit %U\n\n[Desktop Action new]\nName=New\n",
        );
        assert_eq!(parsed["Desktop Entry"]["Name"].as_str(), "Text Editor");
        assert_eq!(parsed["Desktop Action new"]["Name"].as_str(), "New");
    }

    #[test]
    fn reads_the_untranslated_type_comment() {
        let xml = concat!(
            "<mime-type type=\"application/pdf\">",
            "<comment>PDF document</comment>",
            "<comment xml:lang=\"de\">PDF-Dokument</comment>",
            "</mime-type>"
        );
        assert_eq!(
            first_untranslated_comment(xml).as_deref(),
            Some("PDF document")
        );
        assert_eq!(
            first_untranslated_comment("<comment>Tom &amp; Jerry</comment>").as_deref(),
            Some("Tom & Jerry")
        );
    }

    #[test]
    fn recognizes_generic_fallback_icons_by_name() {
        let generic = "/usr/share/icons/Adwaita/48x48/mimetypes/text-x-generic.png";
        let unknown = "/usr/share/icons/hicolor/48x48/mimetypes/unknown.png";
        let specific = "/usr/share/icons/Adwaita/48x48/mimetypes/application-pdf.png";
        assert!(is_generic_system_icon(generic, None));
        assert!(is_generic_system_icon(unknown, None));
        assert!(!is_generic_system_icon(specific, None));
    }

    #[test]
    fn a_shared_icon_is_generic_only_for_types_that_fell_back_to_it() {
        let html = "/usr/share/icons/Adwaita/48x48/mimetypes/text-html.png";
        // text/html looks up "text-html" first, so this is its dedicated icon.
        assert!(!is_generic_system_icon(html, Some("text/html")));
        // Any other type reaching the same file got there by falling back.
        assert!(is_generic_system_icon(html, Some("application/xhtml+xml")));
    }

    #[test]
    fn reads_sizes_from_theme_directory_names() {
        assert_eq!(parse_theme_size("48x48"), 48);
        assert_eq!(parse_theme_size("32"), 32);
        assert_eq!(parse_theme_size("64x64@2x"), 64);
        assert_eq!(parse_theme_size("scalable"), 0);
    }

    #[test]
    fn rejects_desktop_ids_that_escape_the_applications_directory() {
        assert_eq!(find_desktop_file("../../etc/passwd.desktop"), None);
        assert_eq!(find_desktop_file("/etc/shadow.desktop"), None);
        assert_eq!(find_desktop_file("not-a-desktop-file"), None);
    }
}
