use crate::model::{FileTypeRecord, OutputFormat, RenderedIcon};
use crate::operations::ActionReport;
use crate::state::AppliedRecord;
use anyhow::Result;
use serde::Serialize;

pub fn print_scan(records: &[FileTypeRecord], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => print_json(records),
        OutputFormat::Tsv => {
            println!(
                "extension\tcandidate\tcategory\tlabel\tassessment\tapplication\teffective_icon"
            );
            for record in records {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    clean(&record.extension),
                    record.candidate,
                    record.category,
                    clean(&record.label),
                    record.assessment,
                    clean(record.friendly_application_name.as_deref().unwrap_or("")),
                    clean(record.effective_icon.as_deref().unwrap_or("")),
                );
            }
            Ok(())
        }
        OutputFormat::Table => {
            println!(
                "{:<9} {:<4} {:<13} {:<5} {:<22} {:<24} {}",
                "EXT", "USE", "CATEGORY", "TEXT", "ASSESSMENT", "APPLICATION", "ICON"
            );
            println!("{}", "-".repeat(112));
            for record in records {
                println!(
                    "{:<9} {:<4} {:<13} {:<5} {:<22} {:<24} {}",
                    truncate(&record.extension, 9),
                    if record.candidate { "yes" } else { "" },
                    truncate(record.category.as_str(), 13),
                    truncate(&record.label, 5),
                    truncate(record.assessment.as_str(), 22),
                    truncate(
                        record
                            .friendly_application_name
                            .as_deref()
                            .or(record.prog_id.as_deref())
                            .unwrap_or(""),
                        24
                    ),
                    truncate(record.effective_icon.as_deref().unwrap_or(""), 38),
                );
            }
            println!("\n{} record(s)", records.len());
            Ok(())
        }
    }
}

pub fn print_actions(reports: &[ActionReport], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => print_json(reports),
        OutputFormat::Tsv => {
            println!("extension\taction\tregistry_value\ticon_path\tnote");
            for report in reports {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    clean(&report.extension),
                    clean(&report.action),
                    clean(report.registry_value.as_deref().unwrap_or("")),
                    clean(report.icon_path.as_deref().unwrap_or("")),
                    clean(&report.note),
                );
            }
            Ok(())
        }
        OutputFormat::Table => {
            println!(
                "{:<9} {:<15} {:<42} {}",
                "EXT", "ACTION", "ICON PATH", "NOTE"
            );
            println!("{}", "-".repeat(112));
            for report in reports {
                println!(
                    "{:<9} {:<15} {:<42} {}",
                    truncate(&report.extension, 9),
                    truncate(&report.action, 15),
                    truncate(report.icon_path.as_deref().unwrap_or(""), 42),
                    truncate(&report.note, 42),
                );
            }
            println!("\n{} action(s)", reports.len());
            Ok(())
        }
    }
}

pub fn print_rendered(records: &[RenderedIcon], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => print_json(records),
        OutputFormat::Tsv => {
            println!("extension\tlabel\tcategory\tcolor\tpath");
            for record in records {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    clean(&record.extension),
                    clean(&record.label),
                    record.category,
                    clean(&record.color),
                    clean(&record.path),
                );
            }
            Ok(())
        }
        OutputFormat::Table => {
            println!(
                "{:<9} {:<6} {:<14} {:<9} {}",
                "EXT", "TEXT", "CATEGORY", "COLOR", "PATH"
            );
            println!("{}", "-".repeat(100));
            for record in records {
                println!(
                    "{:<9} {:<6} {:<14} {:<9} {}",
                    truncate(&record.extension, 9),
                    truncate(&record.label, 6),
                    truncate(record.category.as_str(), 14),
                    truncate(&record.color, 9),
                    record.path,
                );
            }
            Ok(())
        }
    }
}

pub fn print_status(records: &[AppliedRecord], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => print_json(records),
        OutputFormat::Tsv => {
            println!("extension\tconfirmed\tcategory\tlabel\ticon_path\tapplied_value\tprevious_value\tlast_error");
            for record in records {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    clean(&record.extension),
                    record.confirmed,
                    record.category,
                    clean(&record.label),
                    clean(&record.icon_path),
                    clean(&record.applied_registry_value),
                    clean(record.previous_registry_value.as_deref().unwrap_or("")),
                    clean(record.last_error.as_deref().unwrap_or("")),
                );
            }
            Ok(())
        }
        OutputFormat::Table => {
            println!(
                "{:<9} {:<10} {:<14} {:<6} {:<45} {}",
                "EXT", "STATE", "CATEGORY", "TEXT", "ICON", "ERROR"
            );
            println!("{}", "-".repeat(112));
            for record in records {
                println!(
                    "{:<9} {:<10} {:<14} {:<6} {:<45} {}",
                    truncate(&record.extension, 9),
                    if record.confirmed {
                        "applied"
                    } else {
                        "pending"
                    },
                    truncate(record.category.as_str(), 14),
                    truncate(&record.label, 6),
                    truncate(&record.icon_path, 45),
                    truncate(record.last_error.as_deref().unwrap_or(""), 22),
                );
            }
            println!("\n{} recorded change(s)", records.len());
            Ok(())
        }
    }
}

fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn truncate(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        return value.to_string();
    }
    if width <= 1 {
        return "…".to_string();
    }
    let mut output: String = value.chars().take(width - 1).collect();
    output.push('…');
    output
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if matches!(ch, '\t' | '\r' | '\n') {
                ' '
            } else {
                ch
            }
        })
        .collect()
}
