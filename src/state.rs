use crate::model::{is_valid_extension, normalize_extension, Category, Scope};
use crate::paths;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub schema_version: u32,
    pub scope: Scope,
    #[serde(default)]
    pub records: BTreeMap<String, AppliedRecord>,
    #[serde(default)]
    pub prog_ids: BTreeMap<String, ProgIdRecord>,
    #[serde(default)]
    pub handler_dll_path: Option<String>,
}

impl StateFile {
    pub fn empty(scope: Scope) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            scope,
            records: BTreeMap::new(),
            prog_ids: BTreeMap::new(),
            handler_dll_path: None,
        }
    }

    pub fn load(scope: Scope) -> Result<Self> {
        let path = paths::state_path(scope)?;
        if !path.exists() {
            return Ok(Self::empty(scope));
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read state file {}", path.display()))?;
        let mut state: StateFile = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse state file {}", path.display()))?;
        if state.schema_version == 1 {
            state.schema_version = STATE_SCHEMA_VERSION;
        } else if state.schema_version != STATE_SCHEMA_VERSION {
            bail!(
                "unsupported state schema {} in {}; expected {}",
                state.schema_version,
                path.display(),
                STATE_SCHEMA_VERSION
            );
        }
        if state.scope != scope {
            bail!(
                "state file {} belongs to {} scope, not {}",
                path.display(),
                state.scope,
                scope
            );
        }
        for (extension, record) in &state.records {
            let normalized = normalize_extension(extension);
            if !is_valid_extension(extension) || normalized.as_str() != extension.as_str() {
                bail!(
                    "state file {} contains an invalid extension key {:?}",
                    path.display(),
                    extension
                );
            }
            if record.extension.as_str() != extension.as_str() || record.scope != scope {
                bail!(
                    "state file {} contains an inconsistent record for {}",
                    path.display(),
                    extension
                );
            }
        }
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::state_path(self.scope)?;
        paths::ensure_parent(&path)?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("failed to write state file {}", temporary.display()))?;
        replace_file(&temporary, &path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedRecord {
    pub extension: String,
    pub scope: Scope,
    pub previous_registry_value: Option<String>,
    pub applied_registry_value: String,
    pub icon_path: String,
    pub category: Category,
    pub label: String,
    pub applied_at_unix: u64,
    pub confirmed: bool,
    pub last_error: Option<String>,
    #[serde(default)]
    pub mechanism: IconMechanism,
    #[serde(default)]
    pub effective_prog_id: Option<String>,
}

impl AppliedRecord {
    pub fn new(
        extension: String,
        scope: Scope,
        previous_registry_value: Option<String>,
        applied_registry_value: String,
        icon_path: String,
        category: Category,
        label: String,
    ) -> Self {
        Self {
            extension,
            scope,
            previous_registry_value,
            applied_registry_value,
            icon_path,
            category,
            label,
            applied_at_unix: now_unix(),
            confirmed: false,
            last_error: None,
            mechanism: IconMechanism::ExtensionOverride,
            effective_prog_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IconMechanism {
    #[default]
    ExtensionOverride,
    IconHandler,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgIdRecord {
    pub prog_id: String,
    pub previous_default_icon: Option<String>,
    pub previous_icon_handler: Option<String>,
    pub applied_default_icon: String,
    pub applied_icon_handler: String,
    pub fallback_icon: String,
    #[serde(default)]
    pub extensions: BTreeSet<String>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn replace_file(temporary: &Path, output: &Path) -> Result<()> {
    if output.exists() {
        fs::remove_file(output)
            .with_context(|| format!("failed to replace existing {}", output.display()))?;
    }
    fs::rename(temporary, output).with_context(|| {
        format!(
            "failed to move temporary state {} to {}",
            temporary.display(),
            output.display()
        )
    })
}
