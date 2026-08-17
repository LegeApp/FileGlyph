use crate::classify::is_protected_extension;
use crate::config::Config;
use crate::model::{
    normalized_expanded_path, CandidateMode, FileTypeRecord, IconAssessment, IconLocation,
};
use crate::platform;
use anyhow::Result;

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
                    raw.content_type.as_deref(),
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
    content_type: Option<&str>,
) -> IconAssessment {
    if !associated {
        return IconAssessment::Unassociated;
    }

    if let Some(icon) = non_empty(extension_icon) {
        if icon_is_executable(icon, executable) {
            return IconAssessment::InheritedExecutableIcon;
        }
        if is_likely_generic_shell_icon(icon, content_type) {
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
        Some(icon) if is_likely_generic_shell_icon(icon, content_type) => {
            IconAssessment::LikelyGenericShellIcon
        }
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

/// Whether the file type is simply showing the icon of the program that opens it.
///
/// What counts as "the program's own icon" is platform-specific: Windows points
/// the type straight at the executable, while a Linux type resolves to the same
/// theme icon the application uses. Both live behind the platform module.
fn icon_is_executable(icon: &str, executable: Option<&str>) -> bool {
    platform::icon_belongs_to_application(icon, executable)
}

/// Whether the resolved icon is one the desktop hands to file types it knows
/// nothing specific about.
///
/// Also entirely platform-specific, and the content type is what makes the answer
/// exact on Linux: a shared icon is only "generic" relative to the type that ended
/// up with it, since one theme icon can be another type's dedicated one.
fn is_likely_generic_shell_icon(icon: &str, content_type: Option<&str>) -> bool {
    platform::is_generic_system_icon(
        &normalized_expanded_path(&IconLocation::parse(icon).path),
        content_type,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Icon strings shaped the way the host actually reports them, so the
    /// platform hooks inside `assess_icon` parse input of the right form.
    #[cfg(windows)]
    mod sample {
        pub const CUSTOM: &str = r"C:\Icons\abc.ico,0";
        pub const PROGRAM: &str = r"C:\Apps\viewer.exe,0";
        pub const GENERIC: &str = r"C:\Windows\System32\shell32.dll,-152";
        pub const EXECUTABLE: &str = r"C:\Apps\viewer.exe";
    }

    #[cfg(not(windows))]
    mod sample {
        pub const CUSTOM: &str = "/usr/share/icons/hicolor/256x256/mimetypes/application-pdf.png";
        pub const PROGRAM: &str = "/usr/share/icons/Adwaita/48x48/mimetypes/x-office-document.png";
        pub const GENERIC: &str = "/usr/share/icons/Adwaita/48x48/mimetypes/text-x-generic.png";
        pub const EXECUTABLE: &str = "/usr/bin/viewer";
    }

    #[test]
    fn direct_custom_icon_is_preserved() {
        assert_eq!(
            assess_icon(
                true,
                Some(sample::CUSTOM),
                Some(sample::CUSTOM),
                Some(sample::EXECUTABLE),
                None,
            ),
            IconAssessment::ExistingExtensionOverride
        );
    }

    #[test]
    fn unassociated_types_are_left_alone() {
        assert_eq!(
            assess_icon(false, None, None, None, None),
            IconAssessment::Unassociated
        );
    }

    #[test]
    fn absent_icon_is_reported_missing() {
        assert_eq!(
            assess_icon(true, None, None, Some(sample::EXECUTABLE), None),
            IconAssessment::MissingIcon
        );
    }

    #[test]
    fn generic_system_icon_is_detected() {
        assert_eq!(
            assess_icon(
                true,
                None,
                Some(sample::GENERIC),
                Some(sample::EXECUTABLE),
                None
            ),
            IconAssessment::LikelyGenericShellIcon
        );
    }

    #[test]
    fn dedicated_program_icon_is_left_alone() {
        assert_eq!(
            assess_icon(
                true,
                None,
                Some(sample::PROGRAM),
                Some(sample::EXECUTABLE),
                None
            ),
            IconAssessment::DedicatedProgramIcon
        );
    }

    #[test]
    fn ignored_extension_override_is_detected() {
        assert_eq!(
            assess_icon(
                true,
                Some(sample::CUSTOM),
                Some(sample::PROGRAM),
                Some(sample::EXECUTABLE),
                None,
            ),
            IconAssessment::IneffectiveExtensionOverride
        );
    }

    /// Windows registers the executable itself as the icon resource. The Linux
    /// equivalent resolves through a desktop entry and is covered by
    /// `platform::linux`, which owns that lookup.
    #[cfg(windows)]
    #[test]
    fn executable_icon_is_detected() {
        assert_eq!(
            assess_icon(
                true,
                None,
                Some(r#""C:\Apps\viewer.exe",0"#),
                Some(sample::EXECUTABLE),
                None,
            ),
            IconAssessment::InheritedExecutableIcon
        );
    }
}
