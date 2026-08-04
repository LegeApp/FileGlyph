#[derive(Debug, Clone)]
pub struct RawFileType {
    pub extension: String,
    pub prog_id: Option<String>,
    pub executable: Option<String>,
    pub friendly_document_name: Option<String>,
    pub friendly_application_name: Option<String>,
    pub content_type: Option<String>,
    pub perceived_type: Option<String>,
    pub extension_icon: Option<String>,
    pub effective_icon: Option<String>,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod non_windows;
#[cfg(not(windows))]
pub use non_windows::*;

pub fn registry_reference(icon_path: &std::path::Path) -> String {
    format!("\"{}\",0", icon_path.display())
}
