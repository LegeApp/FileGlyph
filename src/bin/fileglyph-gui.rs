#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use fileglyph::config::Config;
use fileglyph::model::{CandidateMode, FileTypeRecord, Scope};
use fileglyph::{operations, platform, scan};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Absolute paths to try for a UI font, most preferred first.
///
/// egui ships no default font here (`FontDefinitions::empty`), so the system has
/// to supply one. Windows keeps its fonts in one directory; Linux spreads them
/// across per-family directories under the standard font roots.
#[cfg(windows)]
fn system_font_candidates(monospace: bool) -> Vec<PathBuf> {
    let root = std::env::var_os("WINDIR")
        .or_else(|| std::env::var_os("SystemRoot"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    let names: &[&str] = if monospace {
        &["consola.ttf", "cour.ttf"]
    } else {
        &["segoeui.ttf", "tahoma.ttf", "arial.ttf"]
    };
    names.iter().map(|name| root.join(name)).collect()
}

#[cfg(not(windows))]
fn system_font_candidates(monospace: bool) -> Vec<PathBuf> {
    let relative: &[&str] = if monospace {
        &[
            "truetype/dejavu/DejaVuSansMono.ttf",
            "truetype/liberation/LiberationMono-Regular.ttf",
            "truetype/ubuntu/UbuntuMono-R.ttf",
            "TTF/DejaVuSansMono.ttf",
        ]
    } else {
        &[
            "truetype/dejavu/DejaVuSans.ttf",
            "truetype/liberation/LiberationSans-Regular.ttf",
            "truetype/ubuntu/Ubuntu-R.ttf",
            "truetype/noto/NotoSans-Regular.ttf",
            "TTF/DejaVuSans.ttf",
        ]
    };
    let roots = [
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    roots
        .iter()
        .flat_map(|root| relative.iter().map(|name| root.join(name)))
        .collect()
}

fn read_first_font(monospace: bool) -> Option<Vec<u8>> {
    system_font_candidates(monospace)
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
}

fn configure_system_fonts(context: &egui::Context) -> Result<(), String> {
    let proportional = read_first_font(false)
        .ok_or_else(|| "Could not read a system proportional font.".to_owned())?;
    let monospace = read_first_font(true).unwrap_or_else(|| proportional.clone());

    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "system-ui".to_owned(),
        egui::FontData::from_owned(proportional).into(),
    );
    fonts.font_data.insert(
        "system-monospace".to_owned(),
        egui::FontData::from_owned(monospace).into(),
    );
    fonts
        .families
        .insert(egui::FontFamily::Proportional, vec!["system-ui".to_owned()]);
    fonts.families.insert(
        egui::FontFamily::Monospace,
        vec!["system-monospace".to_owned(), "system-ui".to_owned()],
    );
    context.set_fonts(fonts);
    Ok(())
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FileGlyph")
            .with_inner_size([980.0, 700.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "FileGlyph",
        options,
        Box::new(|context| {
            context.egui_ctx.set_theme(egui::ThemePreference::System);
            let font_error = configure_system_fonts(&context.egui_ctx).err();
            let mut app = FileGlyphApp::new();
            if let Some(error) = font_error {
                app.set_error(error);
            }
            Ok(Box::new(app))
        }),
    )
}

struct GuiRecord {
    record: FileTypeRecord,
    selected: bool,
    managed: bool,
    mechanism: Option<String>,
}

struct FileGlyphApp {
    config: Option<Config>,
    records: Vec<GuiRecord>,
    mode: CandidateMode,
    scope: Scope,
    filter: String,
    show_all: bool,
    include_protected: bool,
    confirm_apply: bool,
    confirm_restore: bool,
    managed_count: usize,
    message: String,
    is_error: bool,
}

impl FileGlyphApp {
    fn new() -> Self {
        let mut app = Self {
            config: None,
            records: Vec::new(),
            mode: CandidateMode::Conservative,
            scope: Scope::User,
            filter: String::new(),
            show_all: false,
            include_protected: false,
            confirm_apply: false,
            confirm_restore: false,
            managed_count: 0,
            message: String::new(),
            is_error: false,
        };

        match Config::load(None) {
            Ok(config) => {
                app.config = Some(config);
                app.rescan();
            }
            Err(error) => app.set_error(format!("Could not load configuration: {error:#}")),
        }
        app
    }

    fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.is_error = false;
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.is_error = true;
    }

    fn rescan(&mut self) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let prior_selection: BTreeSet<String> = self
            .records
            .iter()
            .filter(|item| item.selected)
            .map(|item| item.record.extension.clone())
            .collect();

        let managed = match operations::state_records(self.scope) {
            Ok(records) => records
                .into_iter()
                .map(|record| (record.extension, format!("{:?}", record.mechanism)))
                .collect::<BTreeMap<_, _>>(),
            Err(error) => {
                self.set_error(format!("Could not load recovery state: {error:#}"));
                return;
            }
        };
        self.managed_count = managed.len();

        match scan::scan_file_types(config, self.mode, self.include_protected) {
            Ok(records) => {
                self.records = records
                    .into_iter()
                    .filter(|record| record.associated)
                    .map(|record| {
                        let mechanism = managed.get(&record.extension).cloned();
                        let is_managed = mechanism.is_some();
                        let selected = prior_selection.contains(&record.extension);
                        GuiRecord {
                            record,
                            selected,
                            managed: is_managed,
                            mechanism,
                        }
                    })
                    .collect();
                self.set_message(format!(
                    "Scan complete: {} associated types; {} managed in {} scope.",
                    self.records.len(),
                    self.managed_count,
                    self.scope
                ));
            }
            Err(error) => self.set_error(format!("Scan failed: {error:#}")),
        }
    }

    fn apply_selected(&mut self) {
        let selected: Vec<FileTypeRecord> = self
            .records
            .iter()
            .filter(|item| item.selected)
            .map(|item| item.record.clone())
            .collect();
        if selected.is_empty() {
            self.set_error("Select at least one file type.");
            return;
        }
        let Some(config) = self.config.as_ref() else {
            return;
        };

        match operations::apply(config, &selected, self.scope, false, None) {
            Ok(reports) => {
                self.confirm_apply = false;
                self.set_message(format!(
                    "Applied {} icon(s) in {} scope. Reopen file manager windows if old icons remain.",
                    reports.len(),
                    self.scope
                ));
                self.rescan();
            }
            Err(error) => self.set_error(format!("Apply failed: {error:#}")),
        }
    }

    fn restore_all(&mut self) {
        match operations::restore(self.scope, &[], true, false, false) {
            Ok(reports) => {
                let skipped = reports
                    .iter()
                    .filter(|report| report.action == "skipped")
                    .count();
                self.confirm_restore = false;
                self.set_message(format!(
                    "Restored {} record(s); {} skipped because their value changed.",
                    reports.len().saturating_sub(skipped),
                    skipped
                ));
                self.rescan();
            }
            Err(error) => self.set_error(format!("Restore failed: {error:#}")),
        }
    }

    fn is_visible(item: &GuiRecord, show_all: bool, filter: &str) -> bool {
        if !show_all && !item.record.candidate && !item.managed {
            return false;
        }
        let filter = filter.trim().to_ascii_lowercase();
        filter.is_empty()
            || item.record.extension.contains(&filter)
            || item.record.label.to_ascii_lowercase().contains(&filter)
            || item
                .record
                .friendly_application_name
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains(&filter)
    }
}

impl eframe::App for FileGlyphApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(root_ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("FileGlyph");
                ui.separator();
                ui.label("Scope:");
                let old_scope = self.scope;
                ui.radio_value(&mut self.scope, Scope::User, "Current user");
                ui.radio_value(&mut self.scope, Scope::Machine, "All users (Administrator)");
                if self.scope != old_scope {
                    self.rescan();
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label("Scan:");
                ui.radio_value(&mut self.mode, CandidateMode::Missing, "Missing");
                ui.radio_value(&mut self.mode, CandidateMode::Conservative, "Conservative");
                ui.radio_value(&mut self.mode, CandidateMode::Aggressive, "Aggressive");
                ui.checkbox(&mut self.show_all, "Show all associated");
                ui.checkbox(&mut self.include_protected, "Include protected");
                if ui.button("Rescan").clicked() {
                    self.rescan();
                }
                if ui.button("Refresh icon caches").clicked() {
                    platform::notify_association_changed();
                    self.set_message("Icon caches refreshed. Reopen affected folders.");
                }
            });

            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(220.0)
                        .hint_text(".txt, application, or label"),
                );
                if ui.button("Select visible").clicked() {
                    let show_all = self.show_all;
                    let filter = self.filter.clone();
                    for item in &mut self.records {
                        if Self::is_visible(item, show_all, &filter) {
                            item.selected = true;
                        }
                    }
                }
                if ui.button("Clear selection").clicked() {
                    for item in &mut self.records {
                        item.selected = false;
                    }
                }
                let selected = self.records.iter().filter(|item| item.selected).count();
                ui.label(format!("{selected} selected"));
            });
            ui.separator();
            let message_color = if self.is_error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            };
            ui.colored_label(message_color, &self.message);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(
                    &mut self.confirm_apply,
                    "I have reviewed the selected extensions",
                );
                let selected = self.records.iter().filter(|item| item.selected).count();
                if ui
                    .add_enabled(
                        self.confirm_apply && selected > 0,
                        egui::Button::new(format!("Apply {selected} selected ({})", self.scope)),
                    )
                    .clicked()
                {
                    self.apply_selected();
                }

                ui.separator();
                ui.checkbox(&mut self.confirm_restore, "Confirm restore");
                if ui
                    .add_enabled(
                        self.confirm_restore && self.managed_count > 0,
                        egui::Button::new(format!(
                            "Restore all {} managed ({})",
                            self.managed_count, self.scope
                        )),
                    )
                    .clicked()
                {
                    self.restore_all();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("Use");
                ui.add_space(14.0);
                ui.strong("Extension");
                ui.add_space(32.0);
                ui.strong("Label");
                ui.add_space(28.0);
                ui.strong("Category");
                ui.add_space(28.0);
                ui.strong("Assessment");
                ui.add_space(38.0);
                ui.strong("Application");
            });
            ui.separator();

            let show_all = self.show_all;
            let filter = self.filter.clone();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for item in &mut self.records {
                        if !Self::is_visible(item, show_all, &filter) {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut item.selected, "");
                            ui.monospace(format!("{:<12}", item.record.extension));
                            ui.monospace(format!("{:<7}", item.record.label));
                            ui.label(format!("{:<14}", item.record.category));
                            ui.label(format!("{:<22}", item.record.assessment));
                            ui.label(
                                item.record
                                    .friendly_application_name
                                    .as_deref()
                                    .or(item.record.prog_id.as_deref())
                                    .unwrap_or(""),
                            );
                            if item.managed {
                                ui.colored_label(
                                    egui::Color32::LIGHT_GREEN,
                                    format!(
                                        "managed ({})",
                                        item.mechanism.as_deref().unwrap_or("unknown")
                                    ),
                                );
                            }
                        });
                    }
                });
        });
    }
}
