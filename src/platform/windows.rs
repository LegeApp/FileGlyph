use super::RawFileType;
use crate::config::Config;
use crate::icon::IconRenderer;
use crate::model::{
    is_valid_extension, normalize_extension, normalized_expanded_path, FileTypeRecord,
    IconLocation, Scope,
};
use crate::paths;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::ffi::c_void;
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use winreg::enums::{
    HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE,
};
use winreg::RegKey;

const ASSOCF_NOTRUNCATE: u32 = 0x0000_0020;
const ASSOCF_INIT_IGNOREUNKNOWN: u32 = 0x0000_0400;
const ASSOC_FLAGS: u32 = ASSOCF_NOTRUNCATE | ASSOCF_INIT_IGNOREUNKNOWN;

const ASSOCSTR_EXECUTABLE: u32 = 2;
const ASSOCSTR_FRIENDLYDOCNAME: u32 = 3;
const ASSOCSTR_FRIENDLYAPPNAME: u32 = 4;
const ASSOCSTR_CONTENTTYPE: u32 = 14;
const ASSOCSTR_DEFAULTICON: u32 = 15;
const ASSOCSTR_PROGID: u32 = 20;

const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
const SHCNF_IDLIST: u32 = 0x0000;
const SHCNF_FLUSH: u32 = 0x1000;
pub const FILEGLYPH_HANDLER_CLSID: &str = "{5F7A3B34-EA94-4A97-B08F-8D7DEA8CDF11}";

/// A ProgID-level icon can out-rank the extension-level value, so the shell
/// extension exists as a second mechanism when the first one does not take.
pub const SUPPORTS_ICON_HANDLER_FALLBACK: bool = true;

/// A registry icon value names a resource and an index within it.
pub fn registry_reference(icon_path: &Path) -> String {
    format!("\"{}\",0", icon_path.display())
}

/// Whether a registered icon is just the opening program's own executable.
///
/// Only index 0 counts: a non-zero index selects a specific resource inside the
/// binary, which is a deliberate choice rather than an inherited default.
pub fn icon_belongs_to_application(icon: &str, executable: Option<&str>) -> bool {
    let Some(executable) = executable else {
        return false;
    };
    let location = IconLocation::parse(icon);
    if location.index != 0 {
        return false;
    }
    normalized_expanded_path(&location.path) == normalized_expanded_path(executable)
}

/// Icons Explorer hands to file types with nothing more specific registered.
/// The registered resource is enough to tell; the content type adds nothing.
pub fn is_generic_system_icon(normalized_path: &str, _content_type: Option<&str>) -> bool {
    ["\\shell32.dll", "\\imageres.dll", "\\ddores.dll"]
        .iter()
        .any(|suffix| normalized_path.ends_with(suffix))
}

/// One ICO per extension in FileGlyph's own directory; the registry points at it.
pub fn icon_asset_path(_config: &Config, scope: Scope, record: &FileTypeRecord) -> Result<PathBuf> {
    paths::icon_path(scope, &record.extension)
}

pub fn write_icon_asset(
    renderer: &IconRenderer,
    scope: Scope,
    record: &FileTypeRecord,
) -> Result<()> {
    let path = paths::icon_path(scope, &record.extension)?;
    renderer.write_ico(&path, &record.label, record.category)
}

/// Undo one applied extension: put the registry back, then drop the ICO it
/// referenced. In that order, no window exists where the value points at a
/// file that has already been deleted.
pub fn restore_scoped_icon(
    scope: Scope,
    extension: &str,
    previous: Option<&str>,
    recorded_icon_path: &str,
) -> Result<()> {
    match previous {
        Some(value) => write_scoped_icon(scope, extension, value)?,
        None => remove_scoped_icon(scope, extension)?,
    }
    // Absence is the desired end state, so a missing file is not a failure.
    let _ = fs::remove_file(recorded_icon_path);
    Ok(())
}

#[link(name = "Shlwapi")]
extern "system" {
    fn AssocQueryStringW(
        flags: u32,
        association_string: u32,
        association: *const u16,
        extra: *const u16,
        output: *mut u16,
        output_chars: *mut u32,
    ) -> i32;
}

#[link(name = "Shell32")]
extern "system" {
    fn SHChangeNotify(event_id: i32, flags: u32, item1: *const c_void, item2: *const c_void);
}

pub fn scan_raw_file_types() -> Result<Vec<RawFileType>> {
    let mut extensions = BTreeSet::new();

    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    for key in hkcr.enum_keys().flatten() {
        if is_extension_key(&key) {
            extensions.insert(normalize_extension(&key));
        }
    }

    // Explorer may retain UserChoice/OpenWith metadata for extensions that do not
    // currently appear as direct HKCR keys. Include those names as scan inputs.
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(file_exts) = hkcu.open_subkey_with_flags(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts",
        KEY_READ,
    ) {
        for key in file_exts.enum_keys().flatten() {
            if is_extension_key(&key) {
                extensions.insert(normalize_extension(&key));
            }
        }
    }

    let mut records = Vec::with_capacity(extensions.len());
    for extension in extensions {
        let extension_icon = read_hkcr_default(&format!("{extension}\\DefaultIcon"));
        let perceived_type = read_hkcr_value(&extension, "PerceivedType");
        let prog_id = assoc_query(&extension, ASSOCSTR_PROGID);
        // Some valid per-user ProgID registrations expose their icon through the
        // association API but do not return ASSOCSTR_EXECUTABLE. Fall back to the
        // effective open command so executable-derived icons remain detectable.
        let executable = assoc_query(&extension, ASSOCSTR_EXECUTABLE)
            .or_else(|| executable_from_open_command(&extension, prog_id.as_deref()));
        records.push(RawFileType {
            prog_id,
            executable,
            friendly_document_name: assoc_query(&extension, ASSOCSTR_FRIENDLYDOCNAME),
            friendly_application_name: assoc_query(&extension, ASSOCSTR_FRIENDLYAPPNAME),
            content_type: assoc_query(&extension, ASSOCSTR_CONTENTTYPE),
            effective_icon: assoc_query(&extension, ASSOCSTR_DEFAULTICON),
            extension,
            perceived_type,
            extension_icon,
        });
    }

    Ok(records)
}

pub fn read_scoped_icon(scope: Scope, extension: &str) -> Result<Option<String>> {
    let root = scoped_root(scope);
    let path = scoped_default_icon_path(extension);
    match root.open_subkey_with_flags(&path, KEY_READ) {
        Ok(key) => Ok(key.get_value::<String, _>("").ok()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read registry key {path}")),
    }
}

pub fn write_scoped_icon(scope: Scope, extension: &str, value: &str) -> Result<()> {
    let root = scoped_root(scope);
    let path = scoped_default_icon_path(extension);
    let (key, _) = root
        .create_subkey(&path)
        .with_context(|| format!("failed to create registry key {path}"))?;
    key.set_value("", &value)
        .with_context(|| format!("failed to set the default value of {path}"))?;
    Ok(())
}

pub fn remove_scoped_icon(scope: Scope, extension: &str) -> Result<()> {
    let root = scoped_root(scope);
    let extension_path = scoped_extension_path(extension);
    let default_icon_path = format!("{extension_path}\\DefaultIcon");

    match root.open_subkey_with_flags(&default_icon_path, KEY_READ | KEY_WRITE) {
        Ok(key) => {
            match key.delete_value("") {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to delete the default value of {default_icon_path}")
                    })
                }
            }
            drop(key);
            // Delete only the now-empty leaf. If another value exists, leave it intact.
            let _ = root.delete_subkey(&default_icon_path);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to open registry key {default_icon_path}"))
        }
    }
}

pub fn notify_association_changed() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED,
            SHCNF_IDLIST | SHCNF_FLUSH,
            ptr::null(),
            ptr::null(),
        );
    }
}

pub fn query_effective_icon(extension: &str) -> Option<String> {
    assoc_query(&normalize_extension(extension), ASSOCSTR_DEFAULTICON)
}

pub fn read_prog_id_default_icon(prog_id: &str) -> Result<Option<String>> {
    read_user_classes_default(&format!(r"{prog_id}\DefaultIcon"))
}

pub fn read_prog_id_icon_handler(prog_id: &str) -> Result<Option<String>> {
    read_user_classes_default(&format!(r"{prog_id}\ShellEx\IconHandler"))
}

pub fn read_effective_prog_id_default_icon(prog_id: &str) -> Option<String> {
    read_hkcr_default(&format!(r"{prog_id}\DefaultIcon"))
}

pub fn read_effective_prog_id_icon_handler(prog_id: &str) -> Option<String> {
    read_hkcr_default(&format!(r"{prog_id}\ShellEx\IconHandler"))
}

pub fn has_fileglyph_icon_handler(prog_id: Option<&str>, extension: &str) -> bool {
    let handler_matches = prog_id
        .and_then(read_effective_prog_id_icon_handler)
        .is_some_and(|value| value.eq_ignore_ascii_case(FILEGLYPH_HANDLER_CLSID));
    if !handler_matches {
        return false;
    }
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\FileGlyph\IconMap", KEY_READ)
        .ok()
        .and_then(|key| {
            key.get_value::<String, _>(normalize_extension(extension))
                .ok()
        })
        .is_some()
}

pub fn write_prog_id_handler(prog_id: &str) -> Result<()> {
    write_user_classes_default(&format!(r"{prog_id}\DefaultIcon"), "%1")?;
    write_user_classes_default(
        &format!(r"{prog_id}\ShellEx\IconHandler"),
        FILEGLYPH_HANDLER_CLSID,
    )
}

pub fn restore_prog_id_handler(
    prog_id: &str,
    default_icon: Option<&str>,
    icon_handler: Option<&str>,
) -> Result<()> {
    restore_user_classes_default(&format!(r"{prog_id}\DefaultIcon"), default_icon)?;
    restore_user_classes_default(&format!(r"{prog_id}\ShellEx\IconHandler"), icon_handler)
}

pub fn set_icon_mapping(extension: &str, icon_path: &Path) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\FileGlyph\IconMap")
        .context("failed to create FileGlyph icon map")?;
    key.set_value(
        normalize_extension(extension),
        &icon_path.display().to_string(),
    )
    .context("failed to write FileGlyph icon mapping")
}

pub fn remove_icon_mapping(extension: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(r"Software\FileGlyph\IconMap", KEY_WRITE) {
        let _ = key.delete_value(normalize_extension(extension));
    }
    Ok(())
}

pub fn set_prog_id_fallback(prog_id: &str, value: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\FileGlyph\ProgIdFallbacks")
        .context("failed to create FileGlyph ProgID fallback map")?;
    key.set_value(prog_id, &value)
        .context("failed to write FileGlyph ProgID fallback")
}

pub fn remove_prog_id_fallback(prog_id: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(r"Software\FileGlyph\ProgIdFallbacks", KEY_WRITE) {
        let _ = key.delete_value(prog_id);
    }
    Ok(())
}

pub fn install_handler_dll(source: &Path, destination: &Path) -> Result<PathBuf> {
    if !source.exists() {
        bail!(
            "icon handler DLL was not found beside the executable: {}",
            source.display()
        );
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if !destination.exists() {
        fs::copy(source, destination).with_context(|| {
            format!(
                "failed to install handler {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }
    register_handler_clsid(destination)?;
    Ok(destination.to_path_buf())
}

pub fn unregister_handler_clsid() -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!(
        r"Software\Classes\CLSID\{}\InprocServer32",
        FILEGLYPH_HANDLER_CLSID
    );
    let _ = hkcu.delete_subkey(&path);
    let clsid_path = format!(r"Software\Classes\CLSID\{}", FILEGLYPH_HANDLER_CLSID);
    let _ = hkcu.delete_subkey(&clsid_path);
    Ok(())
}

fn register_handler_clsid(path: &Path) -> Result<()> {
    let key_path = format!(
        r"Software\Classes\CLSID\{}\InprocServer32",
        FILEGLYPH_HANDLER_CLSID
    );
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(&key_path)
        .with_context(|| format!("failed to create registry key {key_path}"))?;
    key.set_value("", &path.display().to_string())?;
    key.set_value("ThreadingModel", &"Apartment")?;
    Ok(())
}

fn read_user_classes_default(relative: &str) -> Result<Option<String>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!(r"Software\Classes\{relative}");
    match hkcu.open_subkey_with_flags(&path, KEY_READ) {
        Ok(key) => Ok(key.get_value::<String, _>("").ok()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {path}")),
    }
}

fn write_user_classes_default(relative: &str, value: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!(r"Software\Classes\{relative}");
    let (key, _) = hkcu.create_subkey(&path)?;
    key.set_value("", &value)?;
    Ok(())
}

fn restore_user_classes_default(relative: &str, value: Option<&str>) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = format!(r"Software\Classes\{relative}");
    match value {
        Some(value) => write_user_classes_default(relative, value),
        None => {
            if let Ok(key) = hkcu.open_subkey_with_flags(&path, KEY_WRITE) {
                let _ = key.delete_value("");
            }
            let _ = hkcu.delete_subkey(&path);
            Ok(())
        }
    }
}

fn scoped_root(scope: Scope) -> RegKey {
    match scope {
        Scope::User => RegKey::predef(HKEY_CURRENT_USER),
        Scope::Machine => RegKey::predef(HKEY_LOCAL_MACHINE),
    }
}

fn scoped_extension_path(extension: &str) -> String {
    format!("Software\\Classes\\{}", normalize_extension(extension))
}

fn scoped_default_icon_path(extension: &str) -> String {
    format!("{}\\DefaultIcon", scoped_extension_path(extension))
}

fn is_extension_key(value: &str) -> bool {
    value.starts_with('.') && is_valid_extension(value)
}

fn read_hkcr_default(path: &str) -> Option<String> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    hkcr.open_subkey_with_flags(path, KEY_READ)
        .ok()?
        .get_value::<String, _>("")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn read_hkcr_value(path: &str, name: &str) -> Option<String> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    hkcr.open_subkey_with_flags(path, KEY_READ)
        .ok()?
        .get_value::<String, _>(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn assoc_query(extension: &str, kind: u32) -> Option<String> {
    let association: Vec<u16> = extension.encode_utf16().chain(Some(0)).collect();
    let mut required_chars = 0_u32;

    unsafe {
        // The sizing call commonly reports an insufficient-buffer HRESULT. The
        // length is the useful output; a zero length means the association was absent.
        let _ = AssocQueryStringW(
            ASSOC_FLAGS,
            kind,
            association.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            &mut required_chars,
        );
    }
    if required_chars == 0 {
        return None;
    }

    let mut buffer = vec![0_u16; required_chars as usize];
    let result = unsafe {
        AssocQueryStringW(
            ASSOC_FLAGS,
            kind,
            association.as_ptr(),
            ptr::null(),
            buffer.as_mut_ptr(),
            &mut required_chars,
        )
    };
    if result < 0 {
        return None;
    }

    let used = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    let value = String::from_utf16_lossy(&buffer[..used]);
    (!value.trim().is_empty()).then_some(value)
}

fn executable_from_open_command(extension: &str, prog_id: Option<&str>) -> Option<String> {
    let prog_id = prog_id
        .map(str::to_owned)
        .or_else(|| read_hkcr_default(extension))?;
    let command = read_hkcr_default(&format!(r"{prog_id}\shell\open\command"))?;
    executable_from_command(&command)
}

fn executable_from_command(command: &str) -> Option<String> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    if let Some(rest) = command.strip_prefix('"') {
        return rest
            .find('"')
            .map(|end| rest[..end].trim().to_string())
            .filter(|path| !path.is_empty());
    }

    command
        .split_whitespace()
        .next()
        .map(str::to_string)
        .filter(|path| !path.is_empty())
}

#[cfg(test)]
mod tests {
    use super::executable_from_command;

    #[test]
    fn extracts_quoted_open_command_executable() {
        assert_eq!(
            executable_from_command(r#""C:\Program Files\App\app.exe" "%1""#).as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
    }
}
