use crate::config::Config;
use crate::icon::IconRenderer;
use crate::model::{
    normalize_extension, normalize_path_for_comparison, FileTypeRecord, IconLocation, RenderedIcon,
    Scope,
};
use crate::paths;
use crate::platform;
use crate::state::{AppliedRecord, IconMechanism, ProgIdRecord, StateFile};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ActionReport {
    pub extension: String,
    pub action: String,
    pub registry_value: Option<String>,
    pub icon_path: Option<String>,
    pub note: String,
}

pub fn render_extensions(
    config: &Config,
    records: &[FileTypeRecord],
    output_dir: &Path,
    font_override: Option<&Path>,
) -> Result<Vec<RenderedIcon>> {
    let renderer = IconRenderer::new(config, font_override)?;
    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    let mut rendered = Vec::new();
    for record in records {
        let path = output_dir.join(paths::icon_file_name(&record.extension)?);
        renderer.write_ico(&path, &record.label, record.category)?;
        rendered.push(RenderedIcon {
            extension: record.extension.clone(),
            label: record.label.clone(),
            category: record.category,
            color: config.color_string_for(record.category),
            path: path.display().to_string(),
        });
    }
    Ok(rendered)
}

pub fn apply(
    config: &Config,
    records: &[FileTypeRecord],
    scope: Scope,
    dry_run: bool,
    font_override: Option<&Path>,
) -> Result<Vec<ActionReport>> {
    if records.is_empty() {
        bail!("no file extensions were selected");
    }

    let renderer = if dry_run {
        None
    } else {
        Some(IconRenderer::new(config, font_override)?)
    };
    let mut state = StateFile::load(scope)?;
    let mut reports = Vec::new();

    for record in records {
        if !record.associated {
            bail!("{} has no resolved program association", record.extension);
        }
        if record.excluded {
            bail!("{} is excluded by configuration", record.extension);
        }

        let icon_path = paths::icon_path(scope, &record.extension)?;
        let registry_value = platform::registry_reference(&icon_path);
        if dry_run {
            reports.push(ActionReport {
                extension: record.extension.clone(),
                action: "would_apply".to_string(),
                registry_value: Some(registry_value),
                icon_path: Some(icon_path.display().to_string()),
                note: format!(
                    "{} / {} / current assessment {}",
                    record.category, record.label, record.assessment
                ),
            });
            continue;
        }

        let renderer = renderer.as_ref().expect("renderer exists outside dry-run");
        renderer.write_ico(&icon_path, &record.label, record.category)?;

        let current = platform::read_scoped_icon(scope, &record.extension)?;
        let previous = state
            .records
            .get(&record.extension)
            .map(|existing| existing.previous_registry_value.clone())
            .unwrap_or(current);
        let mut applied = AppliedRecord::new(
            record.extension.clone(),
            scope,
            previous,
            registry_value.clone(),
            icon_path.display().to_string(),
            record.category,
            record.label.clone(),
        );

        // Persist the recovery record before touching the registry. If the registry
        // write fails or the process dies, guarded restore will compare current state
        // with applied_registry_value before changing anything.
        state
            .records
            .insert(record.extension.clone(), applied.clone());
        state.save()?;

        if let Err(error) = platform::write_scoped_icon(scope, &record.extension, &registry_value) {
            applied.last_error = Some(format!("{error:#}"));
            state.records.insert(record.extension.clone(), applied);
            state.save()?;
            return Err(error).with_context(|| {
                format!(
                    "failed to install icon for {} in {} scope",
                    record.extension, scope
                )
            });
        }

        platform::notify_association_changed();
        let effective = platform::query_effective_icon(&record.extension);
        if !icon_values_match(effective.as_deref(), &registry_value) {
            match install_effective_handler(&mut state, record, &icon_path, scope) {
                Ok(Some(prog_id)) => {
                    applied.mechanism = IconMechanism::IconHandler;
                    applied.effective_prog_id = Some(prog_id);
                }
                Ok(None) => {
                    applied.last_error = Some(
                        "extension value was written, but Windows still resolves another icon"
                            .to_string(),
                    );
                }
                Err(error) => {
                    applied.last_error = Some(format!("{error:#}"));
                    applied.confirmed = true;
                    state.records.insert(record.extension.clone(), applied);
                    state.save()?;
                    reports.push(ActionReport {
                        extension: record.extension.clone(),
                        action: "skipped_handler_conflict".to_string(),
                        registry_value: Some(registry_value),
                        icon_path: Some(icon_path.display().to_string()),
                        note: format!("{error:#}"),
                    });
                    continue;
                }
            }
        }

        applied.confirmed = true;
        state.records.insert(record.extension.clone(), applied);
        state.save()?;
        let mechanism = state
            .records
            .get(&record.extension)
            .map(|item| item.mechanism)
            .unwrap_or_default();
        reports.push(ActionReport {
            extension: record.extension.clone(),
            action: match mechanism {
                IconMechanism::ExtensionOverride => "applied_extension",
                IconMechanism::IconHandler => "applied_handler",
            }
            .to_string(),
            registry_value: Some(registry_value),
            icon_path: Some(icon_path.display().to_string()),
            note: format!("{} / {} / {:?}", record.category, record.label, mechanism),
        });
    }

    if !dry_run {
        platform::notify_association_changed();
    }
    Ok(reports)
}

pub fn restore(
    scope: Scope,
    requested_extensions: &[String],
    restore_all: bool,
    force: bool,
    dry_run: bool,
) -> Result<Vec<ActionReport>> {
    let mut state = StateFile::load(scope)?;
    if state.records.is_empty() {
        return Ok(Vec::new());
    }

    let requested: BTreeSet<String> = requested_extensions
        .iter()
        .map(|value| normalize_extension(value))
        .filter(|value| !value.is_empty())
        .collect();
    if !restore_all && requested.is_empty() {
        bail!("select --all or pass --extensions");
    }

    let selected: Vec<String> = state
        .records
        .keys()
        .filter(|extension| restore_all || requested.contains(extension.as_str()))
        .cloned()
        .collect();
    let mut reports = Vec::new();

    for extension in selected {
        let record = state
            .records
            .get(&extension)
            .cloned()
            .expect("selected state record exists");
        let current = platform::read_scoped_icon(scope, &extension)?;
        let current_matches = current.as_deref() == Some(record.applied_registry_value.as_str());
        if !current_matches && !force {
            reports.push(ActionReport {
                extension: extension.clone(),
                action: "skipped".to_string(),
                registry_value: current,
                icon_path: Some(record.icon_path.clone()),
                note: "registry value changed after FileGlyph applied it; use --force to restore anyway"
                    .to_string(),
            });
            continue;
        }

        if let Some(prog_id) = record.effective_prog_id.as_deref() {
            let Some(prog_record) = state.prog_ids.get(prog_id).cloned() else {
                bail!("state is missing the handler record for {prog_id}");
            };
            let last_extension = prog_record.extensions.len() <= 1;
            if last_extension {
                let current_default = platform::read_prog_id_default_icon(prog_id)?;
                let current_handler = platform::read_prog_id_icon_handler(prog_id)?;
                let handler_matches = current_default.as_deref() == Some("%1")
                    && current_handler.as_deref() == Some(platform::FILEGLYPH_HANDLER_CLSID);
                if !handler_matches && !force {
                    reports.push(ActionReport {
                        extension: extension.clone(),
                        action: "skipped".to_string(),
                        registry_value: current_handler,
                        icon_path: Some(record.icon_path.clone()),
                        note: "ProgID handler changed after FileGlyph applied it; use --force to restore anyway".to_string(),
                    });
                    continue;
                }
            }
            if dry_run {
                reports.push(ActionReport {
                    extension: extension.clone(),
                    action: "would_restore".to_string(),
                    registry_value: record.previous_registry_value.clone(),
                    icon_path: Some(record.icon_path.clone()),
                    note: format!("would remove dynamic mapping from {prog_id}"),
                });
                continue;
            }

            platform::remove_icon_mapping(&extension)?;
            if last_extension {
                platform::restore_prog_id_handler(
                    prog_id,
                    prog_record.previous_default_icon.as_deref(),
                    prog_record.previous_icon_handler.as_deref(),
                )?;
                platform::remove_prog_id_fallback(prog_id)?;
                state.prog_ids.remove(prog_id);
            } else if let Some(managed) = state.prog_ids.get_mut(prog_id) {
                managed.extensions.remove(&extension);
            }
            if state.prog_ids.is_empty() {
                platform::unregister_handler_clsid()?;
                state.handler_dll_path = None;
            }
            state.save()?;
        }

        if dry_run {
            reports.push(ActionReport {
                extension: extension.clone(),
                action: "would_restore".to_string(),
                registry_value: record.previous_registry_value.clone(),
                icon_path: Some(record.icon_path.clone()),
                note: if current_matches {
                    "current value matches the FileGlyph record".to_string()
                } else {
                    "forced restore would overwrite a later registry change".to_string()
                },
            });
            continue;
        }

        match record.previous_registry_value.as_deref() {
            Some(previous) => platform::write_scoped_icon(scope, &extension, previous)?,
            None => platform::remove_scoped_icon(scope, &extension)?,
        }
        state.records.remove(&extension);
        state.save()?;
        let _ = fs::remove_file(&record.icon_path);
        reports.push(ActionReport {
            extension,
            action: "restored".to_string(),
            registry_value: record.previous_registry_value,
            icon_path: Some(record.icon_path),
            note: "prior extension-level icon value restored".to_string(),
        });
    }

    if !dry_run {
        platform::notify_association_changed();
    }
    Ok(reports)
}

pub fn state_records(scope: Scope) -> Result<Vec<AppliedRecord>> {
    Ok(StateFile::load(scope)?.records.into_values().collect())
}

pub fn default_render_directory() -> Result<PathBuf> {
    Ok(paths::user_root()?.join("rendered"))
}

fn install_effective_handler(
    state: &mut StateFile,
    record: &FileTypeRecord,
    icon_path: &Path,
    scope: Scope,
) -> Result<Option<String>> {
    if scope != Scope::User {
        return Ok(None);
    }
    let Some(prog_id) = record.prog_id.clone() else {
        return Ok(None);
    };

    if let Some(existing) = state.prog_ids.get_mut(&prog_id) {
        existing.extensions.insert(record.extension.clone());
        platform::set_icon_mapping(&record.extension, icon_path)?;
        state.save()?;
        return Ok(Some(prog_id));
    }

    let previous_handler = platform::read_prog_id_icon_handler(&prog_id)?;
    let effective_handler = platform::read_effective_prog_id_icon_handler(&prog_id);
    if let Some(handler) = effective_handler.as_deref() {
        if !handler.eq_ignore_ascii_case(platform::FILEGLYPH_HANDLER_CLSID) {
            bail!(
                "{} already has an icon handler ({handler}); preserved it",
                prog_id
            );
        }
        bail!(
            "{} has a stale FileGlyph handler without recovery state; refusing to overwrite it",
            prog_id
        );
    }
    let previous_default = platform::read_prog_id_default_icon(&prog_id)?;
    let fallback = platform::read_effective_prog_id_default_icon(&prog_id)
        .or_else(|| record.executable.clone())
        .unwrap_or_default();
    if fallback.is_empty() {
        return Ok(None);
    }

    let source = std::env::current_exe()?.with_file_name("fileglyph_icon_handler.dll");
    let destination = paths::handler_install_path()?;
    let installed = platform::install_handler_dll(&source, &destination)?;
    state.handler_dll_path = Some(installed.display().to_string());

    let mut extensions = BTreeSet::new();
    extensions.insert(record.extension.clone());
    state.prog_ids.insert(
        prog_id.clone(),
        ProgIdRecord {
            prog_id: prog_id.clone(),
            previous_default_icon: previous_default,
            previous_icon_handler: previous_handler,
            applied_default_icon: "%1".to_string(),
            applied_icon_handler: platform::FILEGLYPH_HANDLER_CLSID.to_string(),
            fallback_icon: fallback.clone(),
            extensions,
        },
    );
    state.save()?;

    platform::set_prog_id_fallback(&prog_id, &fallback)?;
    platform::set_icon_mapping(&record.extension, icon_path)?;
    platform::write_prog_id_handler(&prog_id)?;
    platform::notify_association_changed();
    Ok(Some(prog_id))
}

fn icon_values_match(actual: Option<&str>, expected: &str) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    let actual = IconLocation::parse(actual);
    let expected = IconLocation::parse(expected);
    actual.index == expected.index
        && normalize_path_for_comparison(&actual.path)
            == normalize_path_for_comparison(&expected.path)
}
