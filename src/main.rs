use anyhow::{bail, Context, Result};
use clap::Parser;
use fileglyph::cli::{Cli, Command};
use fileglyph::config::Config;
use fileglyph::icon::IconRenderer;
use fileglyph::model::{
    is_valid_extension, normalize_extension, Category, FileTypeRecord, IconAssessment,
};
use fileglyph::{operations, output, platform, scan};
use std::collections::{BTreeMap, BTreeSet};

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Command::InitConfig(args) = &cli.command {
        let path = Config::write_default(args.output.clone(), args.force)?;
        println!("wrote {}", path.display());
        return Ok(());
    }

    let config = Config::load(cli.config.as_deref())?;
    match cli.command {
        Command::Scan(args) => {
            let records = scan::scan_file_types(&config, args.mode, args.include_protected)?;
            let extension_filter = normalized_set(&args.extensions)?;
            let selected: Vec<_> = records
                .into_iter()
                .filter(|record| {
                    extension_filter.is_empty() || extension_filter.contains(&record.extension)
                })
                .filter(|record| args.include_unassociated || record.associated)
                .filter(|record| args.all || record.candidate)
                .collect();
            output::print_scan(&selected, args.format)?;
        }
        Command::Apply(args) => {
            if args.all_candidates && !args.extensions.is_empty() {
                bail!("choose either --all-candidates or --extensions, not both");
            }
            if !args.all_candidates && args.extensions.is_empty() {
                bail!("pass --all-candidates or --extensions");
            }

            let records = scan::scan_file_types(&config, args.mode, args.include_protected)?;
            let selected = if args.all_candidates {
                records
                    .into_iter()
                    .filter(|record| record.candidate)
                    .collect::<Vec<_>>()
            } else {
                select_explicit(&records, &args.extensions, args.include_protected)?
            };
            if selected.is_empty() {
                bail!("selection produced no applicable associated extensions");
            }

            if args.dry_run {
                let reports =
                    operations::apply(&config, &selected, args.scope, true, args.font.as_deref())?;
                output::print_actions(&reports, args.format)?;
            } else if !args.yes {
                let reports =
                    operations::apply(&config, &selected, args.scope, true, args.font.as_deref())?;
                output::print_actions(&reports, args.format)?;
                bail!("no changes made; review the plan and re-run with --yes");
            } else {
                let reports =
                    operations::apply(&config, &selected, args.scope, false, args.font.as_deref())?;
                output::print_actions(&reports, args.format)?;
            }
        }
        Command::Restore(args) => {
            if args.all && !args.extensions.is_empty() {
                bail!("choose either --all or --extensions, not both");
            }
            if !args.all && args.extensions.is_empty() {
                bail!("pass --all or --extensions");
            }

            if args.dry_run {
                let reports =
                    operations::restore(args.scope, &args.extensions, args.all, args.force, true)?;
                output::print_actions(&reports, args.format)?;
            } else if !args.yes {
                let reports =
                    operations::restore(args.scope, &args.extensions, args.all, args.force, true)?;
                output::print_actions(&reports, args.format)?;
                bail!("no changes made; review the plan and re-run with --yes");
            } else {
                let reports =
                    operations::restore(args.scope, &args.extensions, args.all, args.force, false)?;
                output::print_actions(&reports, args.format)?;
            }
        }
        Command::Render(args) => {
            let mut extensions: Vec<String> =
                normalized_set(&args.extensions)?.into_iter().collect();
            if extensions.is_empty() {
                bail!("no valid extensions were supplied");
            }
            if args.label.is_some() && extensions.len() != 1 {
                bail!("--label can only be used with one extension");
            }
            let records: Vec<FileTypeRecord> = extensions
                .drain(..)
                .map(|extension| {
                    standalone_record(&config, extension, args.category, args.label.as_deref())
                })
                .collect();
            let output_dir = args
                .output_dir
                .unwrap_or(operations::default_render_directory()?);
            let rendered = operations::render_extensions(
                &config,
                &records,
                &output_dir,
                args.font.as_deref(),
            )?;
            output::print_rendered(&rendered, args.format)?;
        }
        Command::Preview(args) => {
            let renderer = IconRenderer::new(&config, args.font.as_deref())?;
            renderer.write_category_preview(&args.output, args.cell_size)?;
            println!(
                "wrote {} using {}",
                args.output.display(),
                renderer.font_path().display()
            );
        }
        Command::Status(args) => {
            let records = operations::state_records(args.scope)?;
            output::print_status(&records, args.format)?;
        }
        Command::Refresh => {
            platform::notify_association_changed();
            println!("Explorer association-change notification sent");
        }
        Command::InitConfig(_) => unreachable!(),
    }

    Ok(())
}

fn normalized_set(values: &[String]) -> Result<BTreeSet<String>> {
    let mut extensions = BTreeSet::new();
    for value in values {
        let extension = normalize_extension(value);
        if !is_valid_extension(&extension) {
            bail!("invalid file extension {value:?}");
        }
        extensions.insert(extension);
    }
    Ok(extensions)
}

fn select_explicit(
    records: &[FileTypeRecord],
    requested: &[String],
    include_protected: bool,
) -> Result<Vec<FileTypeRecord>> {
    let requested = normalized_set(requested)?;
    let by_extension: BTreeMap<&str, &FileTypeRecord> = records
        .iter()
        .map(|record| (record.extension.as_str(), record))
        .collect();
    let mut selected = Vec::new();

    for extension in requested {
        let record = by_extension.get(extension.as_str()).with_context(|| {
            format!("{extension} was not found in the Windows file-type registry")
        })?;
        if !record.associated {
            bail!("{extension} has no resolved program association");
        }
        if record.excluded {
            bail!("{extension} is excluded in the configuration");
        }
        if record.protected && !include_protected {
            bail!(
                "{extension} is a protected executable/system type; pass --include-protected to select it explicitly"
            );
        }
        selected.push((**record).clone());
    }
    Ok(selected)
}

fn standalone_record(
    config: &Config,
    extension: String,
    category: Option<Category>,
    label: Option<&str>,
) -> FileTypeRecord {
    let category =
        category.unwrap_or_else(|| config.category_for(&extension, None, None, None, None));
    let label = label
        .map(|value| {
            value
                .trim()
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .flat_map(|ch| ch.to_uppercase())
                .take(config.style.max_label_chars)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| config.label_for(&extension));

    FileTypeRecord {
        extension,
        associated: true,
        prog_id: None,
        executable: None,
        friendly_document_name: None,
        friendly_application_name: None,
        content_type: None,
        perceived_type: None,
        extension_icon: None,
        effective_icon: None,
        category,
        label,
        assessment: IconAssessment::MissingIcon,
        protected: false,
        excluded: false,
        candidate: true,
    }
}
