//! Fallback backend for hosts with no FileGlyph implementation.
//!
//! Windows and Linux have real backends. Everything else compiles so the library
//! and its tests still build, but every operation that would touch the system
//! reports that the host is unsupported rather than doing something approximate.

use super::RawFileType;
use crate::config::Config;
use crate::icon::IconRenderer;
use crate::model::{FileTypeRecord, Scope};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub const FILEGLYPH_HANDLER_CLSID: &str = "{5F7A3B34-EA94-4A97-B08F-8D7DEA8CDF11}";
pub const SUPPORTS_ICON_HANDLER_FALLBACK: bool = false;

fn unsupported<T>(operation: &str) -> Result<T> {
    bail!("{operation} is only available on Windows and Linux")
}

pub fn registry_reference(icon_path: &Path) -> String {
    icon_path.display().to_string()
}

pub fn scan_raw_file_types() -> Result<Vec<RawFileType>> {
    unsupported("scanning file-type associations")
}

pub fn is_generic_system_icon(_icon_path: &str, _content_type: Option<&str>) -> bool {
    false
}

pub fn read_scoped_icon(_scope: Scope, _extension: &str) -> Result<Option<String>> {
    unsupported("reading an icon override")
}

pub fn write_scoped_icon(_scope: Scope, _extension: &str, _value: &str) -> Result<()> {
    unsupported("writing an icon override")
}

pub fn remove_scoped_icon(_scope: Scope, _extension: &str) -> Result<()> {
    unsupported("removing an icon override")
}

pub fn icon_asset_path(
    _config: &Config,
    _scope: Scope,
    _record: &FileTypeRecord,
) -> Result<PathBuf> {
    unsupported("locating an installed icon")
}

pub fn write_icon_asset(
    _renderer: &IconRenderer,
    _scope: Scope,
    _record: &FileTypeRecord,
) -> Result<()> {
    unsupported("installing an icon")
}

pub fn restore_scoped_icon(
    _scope: Scope,
    _extension: &str,
    _previous: Option<&str>,
    _recorded_icon_path: &str,
) -> Result<()> {
    unsupported("restoring an icon override")
}

pub fn notify_association_changed() {}

pub fn query_effective_icon(_extension: &str) -> Option<String> {
    None
}

pub fn has_fileglyph_icon_handler(_prog_id: Option<&str>, _extension: &str) -> bool {
    false
}

pub fn read_prog_id_default_icon(_prog_id: &str) -> Result<Option<String>> {
    unsupported("reading a ProgID default icon")
}
pub fn read_prog_id_icon_handler(_prog_id: &str) -> Result<Option<String>> {
    unsupported("reading a ProgID icon handler")
}
pub fn read_effective_prog_id_default_icon(_prog_id: &str) -> Option<String> {
    None
}
pub fn read_effective_prog_id_icon_handler(_prog_id: &str) -> Option<String> {
    None
}
pub fn write_prog_id_handler(_prog_id: &str) -> Result<()> {
    unsupported("installing a ProgID icon handler")
}
pub fn restore_prog_id_handler(
    _prog_id: &str,
    _default_icon: Option<&str>,
    _icon_handler: Option<&str>,
) -> Result<()> {
    unsupported("restoring a ProgID icon handler")
}
pub fn set_icon_mapping(_extension: &str, _icon_path: &Path) -> Result<()> {
    unsupported("writing an icon-handler mapping")
}
pub fn remove_icon_mapping(_extension: &str) -> Result<()> {
    unsupported("removing an icon-handler mapping")
}
pub fn set_prog_id_fallback(_prog_id: &str, _value: &str) -> Result<()> {
    unsupported("writing a ProgID fallback icon")
}
pub fn remove_prog_id_fallback(_prog_id: &str) -> Result<()> {
    unsupported("removing a ProgID fallback icon")
}
pub fn install_handler_dll(_source: &Path, _destination: &Path) -> Result<PathBuf> {
    unsupported("installing the icon-handler library")
}
pub fn unregister_handler_clsid() -> Result<()> {
    unsupported("unregistering the icon-handler class")
}
