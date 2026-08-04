use super::RawFileType;
use crate::model::Scope;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub const FILEGLYPH_HANDLER_CLSID: &str = "{5F7A3B34-EA94-4A97-B08F-8D7DEA8CDF11}";

pub fn scan_raw_file_types() -> Result<Vec<RawFileType>> {
    bail!("registry scanning is only available on Windows")
}

pub fn read_scoped_icon(_scope: Scope, _extension: &str) -> Result<Option<String>> {
    bail!("registry access is only available on Windows")
}

pub fn write_scoped_icon(_scope: Scope, _extension: &str, _value: &str) -> Result<()> {
    bail!("registry access is only available on Windows")
}

pub fn remove_scoped_icon(_scope: Scope, _extension: &str) -> Result<()> {
    bail!("registry access is only available on Windows")
}

pub fn notify_association_changed() {}

pub fn query_effective_icon(_extension: &str) -> Option<String> {
    None
}
pub fn read_prog_id_default_icon(_prog_id: &str) -> Result<Option<String>> {
    bail!("registry access is only available on Windows")
}
pub fn read_prog_id_icon_handler(_prog_id: &str) -> Result<Option<String>> {
    bail!("registry access is only available on Windows")
}
pub fn read_effective_prog_id_default_icon(_prog_id: &str) -> Option<String> {
    None
}
pub fn read_effective_prog_id_icon_handler(_prog_id: &str) -> Option<String> {
    None
}
pub fn has_fileglyph_icon_handler(_prog_id: Option<&str>, _extension: &str) -> bool {
    false
}
pub fn write_prog_id_handler(_prog_id: &str) -> Result<()> {
    bail!("registry access is only available on Windows")
}
pub fn restore_prog_id_handler(
    _prog_id: &str,
    _default_icon: Option<&str>,
    _icon_handler: Option<&str>,
) -> Result<()> {
    bail!("registry access is only available on Windows")
}
pub fn set_icon_mapping(_extension: &str, _icon_path: &Path) -> Result<()> {
    bail!("registry access is only available on Windows")
}
pub fn remove_icon_mapping(_extension: &str) -> Result<()> {
    bail!("registry access is only available on Windows")
}
pub fn set_prog_id_fallback(_prog_id: &str, _value: &str) -> Result<()> {
    bail!("registry access is only available on Windows")
}
pub fn remove_prog_id_fallback(_prog_id: &str) -> Result<()> {
    bail!("registry access is only available on Windows")
}
pub fn install_handler_dll(_source: &Path, _destination: &Path) -> Result<PathBuf> {
    bail!("registry access is only available on Windows")
}
pub fn unregister_handler_clsid() -> Result<()> {
    bail!("registry access is only available on Windows")
}
