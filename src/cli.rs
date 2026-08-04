use crate::model::{CandidateMode, Category, OutputFormat, Scope};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "fileglyph",
    version,
    about = "Generate and install standardized text icons for Windows file extensions",
    long_about = None
)]
pub struct Cli {
    /// JSON configuration file. Defaults to %LOCALAPPDATA%\\FileGlyph\\config.json if present.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Find associated file extensions whose current icon is missing or generic.
    Scan(ScanArgs),
    /// Generate ICO files and install extension-level DefaultIcon overrides.
    Apply(ApplyArgs),
    /// Restore registry values recorded before apply.
    Restore(RestoreArgs),
    /// Render ICO files without touching the registry.
    Render(RenderArgs),
    /// Render a PNG contact sheet for all category styles.
    Preview(PreviewArgs),
    /// Show FileGlyph's recorded registry changes.
    Status(StatusArgs),
    /// Tell Explorer that file associations/icons changed.
    Refresh,
    /// Write a default editable JSON configuration.
    InitConfig(InitConfigArgs),
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Candidate threshold used by automatic scanning.
    #[arg(long, value_enum, default_value_t = CandidateMode::Conservative)]
    pub mode: CandidateMode,

    /// Show all associated file types instead of candidates only.
    #[arg(long)]
    pub all: bool,

    /// Include extensions with no resolved program association (only meaningful with --all).
    #[arg(long)]
    pub include_unassociated: bool,

    /// Include executable/system extensions in candidate output.
    #[arg(long)]
    pub include_protected: bool,

    /// Limit output to these comma-separated extensions.
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub extensions: Vec<String>,

    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// Apply to these comma-separated extensions. Explicit selection may replace an existing icon.
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub extensions: Vec<String>,

    /// Apply to every candidate found using --mode.
    #[arg(long)]
    pub all_candidates: bool,

    #[arg(long, value_enum, default_value_t = CandidateMode::Conservative)]
    pub mode: CandidateMode,

    /// User scope normally needs no elevation; machine scope requires Administrator.
    #[arg(long, value_enum, default_value_t = Scope::User)]
    pub scope: Scope,

    /// Permit high-risk executable/system extensions.
    #[arg(long)]
    pub include_protected: bool,

    /// Override the font search with a specific TTF/OTF file.
    #[arg(long)]
    pub font: Option<PathBuf>,

    /// Print intended changes without generating icons or writing the registry.
    #[arg(long)]
    pub dry_run: bool,

    /// Required for a real registry write.
    #[arg(long)]
    pub yes: bool,

    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    #[arg(long, value_enum, default_value_t = Scope::User)]
    pub scope: Scope,

    /// Restore these comma-separated extensions.
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub extensions: Vec<String>,

    /// Restore every recorded extension in the selected scope.
    #[arg(long)]
    pub all: bool,

    /// Restore even if another program changed the registry after FileGlyph applied its value.
    #[arg(long)]
    pub force: bool,

    #[arg(long)]
    pub dry_run: bool,

    /// Required for a real registry write.
    #[arg(long)]
    pub yes: bool,

    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// Extensions to render, for example: --extensions asd,foo,sqlite
    #[arg(long, value_delimiter = ',', num_args = 1.., required = true)]
    pub extensions: Vec<String>,

    /// Force one category for every rendered extension.
    #[arg(long, value_enum)]
    pub category: Option<Category>,

    /// Force a label. Valid only when rendering one extension.
    #[arg(long)]
    pub label: Option<String>,

    /// Output directory. Defaults to %LOCALAPPDATA%\\FileGlyph\\rendered.
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    #[arg(long)]
    pub font: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
pub struct PreviewArgs {
    /// Destination PNG.
    #[arg(long, default_value = "fileglyph-category-preview.png")]
    pub output: PathBuf,

    /// Size of each category sample in the contact sheet.
    #[arg(long, default_value_t = 128)]
    pub cell_size: u32,

    #[arg(long)]
    pub font: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(long, value_enum, default_value_t = Scope::User)]
    pub scope: Scope,

    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,
}

#[derive(Debug, Args)]
pub struct InitConfigArgs {
    #[arg(long)]
    pub output: Option<PathBuf>,

    #[arg(long)]
    pub force: bool,
}
