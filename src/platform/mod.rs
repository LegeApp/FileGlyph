//! Host-specific file-type and icon plumbing.
//!
//! Every backend exposes the same surface, but the systems underneath differ in
//! kind rather than in detail. Windows keys icons off a file *extension* in the
//! registry; Linux keys them off a *MIME type* in an XDG icon theme. The shared
//! code above this module works in extensions and treats the rest as opaque
//! strings, which is what lets one scanner and one apply/restore path serve both.

/// One file type as the host system describes it, before FileGlyph classifies it.
///
/// Field names come from the Windows association API. The Linux backend fills the
/// same slots with the nearest XDG equivalent: `prog_id` carries the MIME type,
/// `executable` the default application's binary, and the icon fields hold
/// absolute paths to files in an icon theme rather than `resource,index` strings.
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

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(any(windows, target_os = "linux")))]
mod unsupported;
#[cfg(not(any(windows, target_os = "linux")))]
pub use unsupported::*;
