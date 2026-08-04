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

pub fn icon_root(scope: Scope) -> Result<PathBuf> {
    match scope {
        Scope::User => Ok(user_root()?.join("icons")),
        Scope::Machine => env::var_os("PROGRAMDATA")
            .map(PathBuf::from)
            .map(|path| path.join(APP_DIR).join("icons"))
            .ok_or_else(|| anyhow::anyhow!("PROGRAMDATA is not set")),
    }
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

pub fn icon_file_name(extension: &str) -> Result<String> {
    let normalized = normalize_extension(extension);
    if !is_valid_extension(&normalized) {
        bail!("invalid file extension {extension:?}");
    }

    let bare = normalized.trim_start_matches('.');
    let stem = if is_reserved_windows_name(bare) {
        format!("_{bare}")
    } else {
        bare.to_string()
    };
    Ok(format!("{stem}.ico"))
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
        assert_eq!(icon_file_name(".asd").unwrap(), "asd.ico");
        assert_eq!(icon_file_name(".con").unwrap(), "_con.ico");
        assert!(icon_file_name("bad/path").is_err());
        assert!(icon_file_name("bad name").is_err());
    }
}
