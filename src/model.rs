use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Text,
    Document,
    Spreadsheet,
    Presentation,
    Image,
    Video,
    Audio,
    Archive,
    Database,
    Code,
    System,
    Model3d,
    Font,
    Internal,
}

impl Category {
    pub const ALL: [Category; 14] = [
        Category::Text,
        Category::Document,
        Category::Spreadsheet,
        Category::Presentation,
        Category::Image,
        Category::Video,
        Category::Audio,
        Category::Archive,
        Category::Database,
        Category::Code,
        Category::System,
        Category::Model3d,
        Category::Font,
        Category::Internal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Text => "text",
            Category::Document => "document",
            Category::Spreadsheet => "spreadsheet",
            Category::Presentation => "presentation",
            Category::Image => "image",
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Archive => "archive",
            Category::Database => "database",
            Category::Code => "code",
            Category::System => "system",
            Category::Model3d => "model3d",
            Category::Font => "font",
            Category::Internal => "internal",
        }
    }

    pub fn default_color(self) -> &'static str {
        match self {
            Category::Text => "#4D7C8A",
            Category::Document => "#3F6FB5",
            Category::Spreadsheet => "#3A7D58",
            Category::Presentation => "#B56A32",
            Category::Image => "#9A5CC2",
            Category::Video => "#B04765",
            Category::Audio => "#6A5ACD",
            Category::Archive => "#8A6D3B",
            Category::Database => "#58717A",
            Category::Code => "#2C7A7B",
            Category::System => "#6B7280",
            Category::Model3d => "#8B5E83",
            Category::Font => "#7B5C48",
            Category::Internal => "#65707E",
        }
    }
}

impl Display for Category {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum CandidateMode {
    /// Only associated extensions for which the system reports no icon.
    Missing,
    /// Missing icons, executable-inherited icons, and likely generic shell icons.
    Conservative,
    /// Any associated extension without an extension-level icon override.
    Aggressive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Per-user settings: HKCU\\Software\\Classes on Windows, the user's XDG
    /// icon theme on Linux. Normally requires no elevation.
    User,
    /// System-wide settings: HKLM\\Software\\Classes on Windows, /usr/share/icons
    /// on Linux. Requires Administrator or root.
    Machine,
}

impl Display for Scope {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Scope::User => "user",
            Scope::Machine => "machine",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Table,
    Json,
    Tsv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IconAssessment {
    #[serde(rename = "unassociated")]
    Unassociated,
    #[serde(rename = "extension_override")]
    ExistingExtensionOverride,
    #[serde(rename = "ineffective_extension_override")]
    IneffectiveExtensionOverride,
    #[serde(rename = "missing")]
    MissingIcon,
    #[serde(rename = "inherited_executable")]
    InheritedExecutableIcon,
    #[serde(rename = "likely_generic_shell")]
    LikelyGenericShellIcon,
    #[serde(rename = "program_icon")]
    DedicatedProgramIcon,
}

impl IconAssessment {
    pub fn as_str(self) -> &'static str {
        match self {
            IconAssessment::Unassociated => "unassociated",
            IconAssessment::ExistingExtensionOverride => "extension_override",
            IconAssessment::IneffectiveExtensionOverride => "ineffective_extension_override",
            IconAssessment::MissingIcon => "missing",
            IconAssessment::InheritedExecutableIcon => "inherited_executable",
            IconAssessment::LikelyGenericShellIcon => "likely_generic_shell",
            IconAssessment::DedicatedProgramIcon => "program_icon",
        }
    }

    pub fn is_candidate(self, mode: CandidateMode) -> bool {
        match mode {
            CandidateMode::Missing => matches!(self, IconAssessment::MissingIcon),
            CandidateMode::Conservative => matches!(
                self,
                IconAssessment::MissingIcon
                    | IconAssessment::IneffectiveExtensionOverride
                    | IconAssessment::InheritedExecutableIcon
                    | IconAssessment::LikelyGenericShellIcon
            ),
            CandidateMode::Aggressive => matches!(
                self,
                IconAssessment::MissingIcon
                    | IconAssessment::IneffectiveExtensionOverride
                    | IconAssessment::InheritedExecutableIcon
                    | IconAssessment::LikelyGenericShellIcon
                    | IconAssessment::DedicatedProgramIcon
            ),
        }
    }
}

impl Display for IconAssessment {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTypeRecord {
    pub extension: String,
    pub associated: bool,
    pub prog_id: Option<String>,
    pub executable: Option<String>,
    pub friendly_document_name: Option<String>,
    pub friendly_application_name: Option<String>,
    pub content_type: Option<String>,
    pub perceived_type: Option<String>,
    pub extension_icon: Option<String>,
    pub effective_icon: Option<String>,
    pub category: Category,
    pub label: String,
    pub assessment: IconAssessment,
    pub protected: bool,
    pub excluded: bool,
    pub candidate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconLocation {
    pub path: String,
    pub index: i32,
}

impl IconLocation {
    pub fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        let (path, index) = match trimmed.rsplit_once(',') {
            Some((candidate_path, candidate_index)) => {
                match candidate_index.trim().parse::<i32>() {
                    Ok(index) => (candidate_path, index),
                    Err(_) => (trimmed, 0),
                }
            }
            None => (trimmed, 0),
        };

        Self {
            path: path.trim().trim_matches('"').to_string(),
            index,
        }
    }

    pub fn normalized_path(&self) -> String {
        normalize_path_for_comparison(&self.path)
    }
}

pub fn normalize_extension(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let without_dot = trimmed.trim_start_matches('.');
    format!(".{}", without_dot.to_ascii_lowercase())
}

pub fn is_valid_extension(value: &str) -> bool {
    let normalized = normalize_extension(value);
    let bare = normalized.trim_start_matches('.');
    if bare.is_empty() || bare.len() > 128 || bare.ends_with('.') {
        return false;
    }

    !bare.chars().any(|ch| {
        ch.is_control()
            || ch.is_whitespace()
            || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
    })
}

/// Reduce a system path to a form safe to compare against another path.
///
/// Windows paths are case-insensitive and accept either separator, so both are
/// folded away. Linux paths are case-sensitive and only `/` separates
/// components, so folding case there would equate genuinely different files.
#[cfg(windows)]
pub fn normalize_path_for_comparison(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

#[cfg(not(windows))]
pub fn normalize_path_for_comparison(value: &str) -> String {
    let trimmed = value.trim().trim_matches('"');
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed.is_empty() && value.contains('/') {
        return "/".to_string();
    }
    trimmed.to_string()
}

/// Fully resolve a system path so two spellings of the same file compare equal.
pub fn normalized_expanded_path(value: &str) -> String {
    // A leading '@' marks an indirect resource string on Windows.
    let value = value.trim().trim_start_matches('@');
    normalize_path_for_comparison(&expand_environment(value))
}

/// Substitute `%NAME%` references from the environment, leaving unknown names as
/// written so a failed lookup stays visible instead of silently emptying a path.
pub fn expand_environment(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '%' {
            if let Some(relative_end) = chars[index + 1..].iter().position(|ch| *ch == '%') {
                let end = index + 1 + relative_end;
                let name: String = chars[index + 1..end].iter().collect();
                if let Some(replacement) = std::env::var_os(&name) {
                    result.push_str(&replacement.to_string_lossy());
                    index = end + 1;
                    continue;
                }
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedIcon {
    pub extension: String,
    pub label: String,
    pub category: Category,
    pub color: String,
    pub path: String,
}
