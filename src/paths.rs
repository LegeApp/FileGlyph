use crate::model::{is_valid_extension, normalize_extension, Scope};
use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "FileGlyph";

pub fn user_root() -> Result<PathBuf> {
    dirs::data_local_dir()
        .map(|path| path.join(APP_DIR))
        .ok_or_else(|| anyhow::anyhow!("could not determine the local application-data directory"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(user_root()?.join("config.json"))
}

pub fn state_path(scope: Scope) -> Result<PathBuf> {
    Ok(user_root()?.join(format!("state-{scope}.json")))
}

/// Root under which applied icons live.
///
/// On Windows this is a private FileGlyph directory; the registry points at the
/// files by absolute path. On Linux an icon is only found if it sits inside an
/// XDG icon theme, so the root is the directory that holds themes and the
/// platform layer appends the theme name it is overriding.
#[cfg(windows)]
pub fn icon_root(scope: Scope) -> Result<PathBuf> {
    match scope {
        Scope::User => Ok(user_root()?.join("icons")),
        Scope::Machine => env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .map(|path| path.join(APP_DIR).join("icons"))
            .ok_or_else(|| anyhow::anyhow!("PROGRAMDATA is not set")),
    }
}

#[cfg(not(windows))]
pub fn icon_root(scope: Scope) -> Result<PathBuf> {
    match scope {
        Scope::User => Ok(xdg_data_home()?.join("icons")),
        Scope::Machine => Ok(PathBuf::from("/usr/share/icons")),
    }
}

/// Directory holding icon files displaced by an apply, so restore can put the
/// originals back byte for byte.
#[cfg(not(windows))]
pub fn icon_backup_root(scope: Scope) -> Result<PathBuf> {
    Ok(user_root()?.join("backup").join(scope.to_string()))
}

/// `$XDG_DATA_HOME`, falling back to the specified `~/.local/share`.
#[cfg(not(windows))]
pub fn xdg_data_home() -> Result<PathBuf> {
    if let Some(value) = env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(value);
        // The specification requires an absolute path; a relative one is ignored.
        if path.is_absolute() {
            return Ok(path);
        }
    }
    dirs::home_dir()
        .map(|home| home.join(".local").join("share"))
        .ok_or_else(|| anyhow::anyhow!("could not determine the home directory"))
}

pub fn handler_install_path() -> Result<PathBuf> {
    Ok(user_root()?
        .join("handler")
        .join(env!("CARGO_PKG_VERSION"))
        .join("x86_64")
        .join("fileglyph_icon_handler.dll"))
}

pub fn icon_path(scope: Scope, extension: &str) -> Result<PathBuf> {
    Ok(icon_root(scope)?.join(icon_file_name(extension)?))
}

/// Container format for a standalone rendered icon: ICO carries every size in one
/// file on Windows, while Linux desktops read PNGs out of sized theme directories.
pub const ICON_FILE_SUFFIX: &str = if cfg!(windows) { "ico" } else { "png" };

pub fn icon_file_name(extension: &str) -> Result<String> {
    Ok(format!("{}.{ICON_FILE_SUFFIX}", icon_file_stem(extension)?))
}

pub fn icon_file_stem(extension: &str) -> Result<String> {
    let normalized = normalize_extension(extension);
    if !is_valid_extension(&normalized) {
        bail!("invalid file extension {extension:?}");
    }

    let bare = normalized.trim_start_matches('.');
    // Kept on every platform so a rendered set has the same names everywhere.
    Ok(if is_reserved_windows_name(bare) {
        format!("_{bare}")
    } else {
        bare.to_string()
    })
}

fn is_reserved_windows_name(value: &str) -> bool {
    let base = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(
            base.strip_prefix("COM"),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        )
        || matches!(
            base.strip_prefix("LPT"),
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        )
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_windows_safe_icon_names() {
        assert_eq!(icon_file_stem(".asd").unwrap(), "asd");
        assert_eq!(icon_file_stem(".con").unwrap(), "_con");
        assert!(icon_file_stem("bad/path").is_err());
        assert!(icon_file_stem("bad name").is_err());
    }

    #[test]
    fn names_use_the_platform_icon_container() {
        let name = icon_file_name(".asd").unwrap();
        assert_eq!(name, format!("asd.{ICON_FILE_SUFFIX}"));
        assert!(name.ends_with(if cfg!(windows) { ".ico" } else { ".png" }));
    }
}
