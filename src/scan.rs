use crate::classify::is_protected_extension;
use crate::config::Config;
use crate::model::{
    normalize_path_for_comparison, CandidateMode, FileTypeRecord, IconAssessment, IconLocation,
};
use crate::platform;
use anyhow::Result;
use std::env;

pub fn scan_file_types(
    config: &Config,
    mode: CandidateMode,
    include_protected: bool,
) -> Result<Vec<FileTypeRecord>> {
    let mut output = Vec::new();

    for raw in platform::scan_raw_file_types()? {
        let associated = raw.prog_id.is_some() || raw.executable.is_some();
        let category = config.category_for(
            &raw.extension,
            raw.content_type.as_deref(),
            raw.perceived_type.as_deref(),
            raw.prog_id.as_deref(),
            raw.friendly_document_name.as_deref(),
        );
        let label = config.label_for(&raw.extension);
        let assessment =
            if platform::has_fileglyph_icon_handler(raw.prog_id.as_deref(), &raw.extension) {
                IconAssessment::ExistingExtensionOverride
            } else {
                assess_icon(
                    associated,
                    raw.extension_icon.as_deref(),
                    raw.effective_icon.as_deref(),
                    raw.executable.as_deref(),
                )
            };
        let protected = is_protected_extension(&raw.extension);
        let excluded = config.is_excluded(&raw.extension);
        let candidate = associated
            && assessment.is_candidate(mode)
            && (!protected || include_protected)
            && !excluded;

        output.push(FileTypeRecord {
            extension: raw.extension,
            associated,
            prog_id: raw.prog_id,
            executable: raw.executable,
            friendly_document_name: raw.friendly_document_name,
            friendly_application_name: raw.friendly_application_name,
            content_type: raw.content_type,
            perceived_type: raw.perceived_type,
            extension_icon: raw.extension_icon,
            effective_icon: raw.effective_icon,
            category,
            label,
            assessment,
            protected,
            excluded,
            candidate,
        });
    }

    output.sort_by(|left, right| left.extension.cmp(&right.extension));
    Ok(output)
}

fn assess_icon(
    associated: bool,
    extension_icon: Option<&str>,
    effective_icon: Option<&str>,
    executable: Option<&str>,
) -> IconAssessment {
    if !associated {
        return IconAssessment::Unassociated;
    }

    if let Some(icon) = non_empty(extension_icon) {
        if icon_is_executable(icon, executable) {
            return IconAssessment::InheritedExecutableIcon;
        }
        if is_likely_generic_shell_icon(icon) {
            return IconAssessment::LikelyGenericShellIcon;
        }
        if let Some(effective) = non_empty(effective_icon) {
            if !same_icon_location(icon, effective) {
                return IconAssessment::IneffectiveExtensionOverride;
            }
        }
        return IconAssessment::ExistingExtensionOverride;
    }

    match non_empty(effective_icon) {
        None => IconAssessment::MissingIcon,
        Some(icon) if icon_is_executable(icon, executable) => {
            IconAssessment::InheritedExecutableIcon
        }
        Some(icon) if is_likely_generic_shell_icon(icon) => IconAssessment::LikelyGenericShellIcon,
        Some(_) => IconAssessment::DedicatedProgramIcon,
    }
}

fn same_icon_location(left: &str, right: &str) -> bool {
    let left = IconLocation::parse(left);
    let right = IconLocation::parse(right);
    left.index == right.index
        && normalized_expanded_path(&left.path) == normalized_expanded_path(&right.path)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn icon_is_executable(icon: &str, executable: Option<&str>) -> bool {
    let Some(executable) = executable else {
        return false;
    };
    let location = IconLocation::parse(icon);
    if location.index != 0 {
        return false;
    }
    normalized_expanded_path(&location.path) == normalized_expanded_path(executable)
}

fn is_likely_generic_shell_icon(icon: &str) -> bool {
    let path = normalized_expanded_path(&IconLocation::parse(icon).path);
    ["\\shell32.dll", "\\imageres.dll", "\\ddores.dll"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn normalized_expanded_path(value: &str) -> String {
    let value = value.trim().trim_start_matches('@');
    normalize_path_for_comparison(&expand_environment(value))
}

fn expand_environment(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '%' {
            if let Some(relative_end) = chars[index + 1..].iter().position(|ch| *ch == '%') {
                let end = index + 1 + relative_end;
                let name: String = chars[index + 1..end].iter().collect();
                if let Some(replacement) = env::var_os(&name) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_custom_icon_is_preserved() {
        assert_eq!(
            assess_icon(
                true,
                Some(r#"C:\Icons\abc.ico,0"#),
                Some(r#"C:\Icons\abc.ico,0"#),
                Some(r#"C:\Program Files\ABC\abc.exe"#),
            ),
            IconAssessment::ExistingExtensionOverride
        );
    }

    #[test]
    fn executable_icon_is_detected() {
        assert_eq!(
            assess_icon(
                true,
                None,
                Some(r#""C:\Program Files\ABC\abc.exe",0"#),
                Some(r#"C:\Program Files\ABC\abc.exe"#),
            ),
            IconAssessment::InheritedExecutableIcon
        );
    }

    #[test]
    fn ignored_extension_override_is_detected() {
        assert_eq!(
            assess_icon(
                true,
                Some(r#"C:\Icons\txt.ico,0"#),
                Some(r#"C:\Apps\viewer.exe,0"#),
                Some(r#"C:\Apps\viewer.exe"#),
            ),
            IconAssessment::IneffectiveExtensionOverride
        );
    }
}
