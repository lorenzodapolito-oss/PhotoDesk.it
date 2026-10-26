//! The small windows over the main one: confirm the recycle bin, rename, batch rename, new folder
//! and the list of keyboard shortcuts.

use super::{Modal, PhotoDesk};
use crate::ops;
use crate::theme;
use egui::{Align2, Color32, Key};
use std::path::PathBuf;

enum Do {
    Trash(Vec<PathBuf>),
    Rename(PathBuf, String),
    Batch(Vec<ops::Planned>),
    NewFolder(String),
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("Spazio o F", "Anteprima a schermo intero"),
    ("← → ↑ ↓", "Muoversi tra le foto (Maiusc = seleziona)"),
    ("Home / Fine", "Prima o ultima foto"),
    ("0 – 5", "Stelle (anche +  e  −)"),
    ("6 – 9", "Etichetta rossa, gialla, verde, blu"),
    ("Invio o doppio clic", "Apri in DarkRaw o LightRaw"),
    ("C", "Confronta da 2 a 4 foto selezionate"),
    ("Ctrl+A", "Seleziona tutto"),
    ("Ctrl+Maiusc+I", "Inverti la selezione"),
    ("Esc", "Deseleziona (o chiude l'anteprima)"),
    ("F2 o Ctrl+R", "Rinomina (una foto, o più foto con un modello)"),
    ("Canc", "Sposta nel cestino"),
    ("Ctrl+C / X / V", "Copia, taglia, incolla nella cartella aperta"),
    ("Ctrl+Maiusc+N", "Nuova cartella"),
    ("Backspace", "Cartella superiore"),
    ("Alt+← / Alt+→", "Indietro e avanti tra le cartelle visitate"),
    ("Ctrl+L", "Scrivere un percorso"),
    ("F5", "Aggiorna"),
    ("Ctrl+0", "Miniature di dimensione normale"),
    ("? o F1", "Questa lista"),
];

impl PhotoDesk {
    pub(super) fn modal_windows(&mut self, ctx: &egui::Context) {
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        let mut keep = true;
        let mut todo: Option<Do> = None;
        let esc = ctx.input(|i| i.key_pressed(Key::Escape));
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        match &mut modal {
            Modal::Trash(paths) => {
                let n = paths.len();
                window("Spostare nel cestino?", ctx).show(ctx, |ui| {
                    ui.label(format!(
                        "{n} {} andr{} nel cestino di sistema: potrai recuperar{} da lì.",
                        if n == 1 { "file" } else { "file" },
                        if n == 1 { "à" } else { "anno" },
                        if n == 1 { "lo" } else { "li" },
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(theme::primary_button("Sposta nel cestino")).clicked() || enter {
                            todo = Some(Do::Trash(std::mem::take(paths)));
                            keep = false;
                        }
                        if ui.button("Annulla").clicked() || esc {
                            keep = false;
                        }
                    });
                });
            }
            Modal::Rename { path, text } => {
                window("Rinomina", ctx).show(ctx, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(320.0));
                    if !r.has_focus() && !ui.memory(|m| m.had_focus_last_frame(r.id)) {
                        r.request_focus();
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let clean = ops::clean_name(text);
                        if (ui.add(theme::primary_button("Rinomina")).clicked() || enter) && !clean.is_empty() {
                            todo = Some(Do::Rename(path.clone(), clean));
                            keep = false;
                        }
                        if ui.button("Annulla").clicked() || esc {
                            keep = false;
                        }
                    });
                });
            }
            Modal::Batch { pattern, start } => {
                let items = self.rename_items();
                let plan = ops::plan_rename(&items, pattern, *start);
                let bad = plan.iter().any(|p| !p.problem.is_empty());
                window("Rinomina in blocco", ctx).default_width(520.0).show(ctx, |ui| {
                    ui.label("Modello del nome (l'estensione resta quella di prima):");
                    ui.add(egui::TextEdit::singleline(pattern).desired_width(480.0));
                    ui.label(
                        egui::RichText::new(
                            "{originale} nome attuale · {n} o {n:3} contatore · {data} o {data:%Y-%m-%d} · {ora} · {camera} · {cartella}",
                        )
                        .size(12.0)
                        .color(theme::FAINT),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Il contatore parte da");
                        ui.add(egui::DragValue::new(start).range(0..=999_999));
                    });
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Anteprima").strong());
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for p in plan.iter().take(60) {
                            let old = p.from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(old).color(theme::MUTED));
                                ui.label("→");
                                if p.problem.is_empty() {
                                    ui.label(&p.to_name);
                                } else {
                                    ui.colored_label(
                                        Color32::from_rgb(0xb0, 0x30, 0x40),
                                        format!("{} ({})", p.to_name, p.problem),
                                    );
                                }
                            });
                        }
                        if plan.len() > 60 {
                            ui.label(format!("… e altre {}", plan.len() - 60));
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!bad && !plan.is_empty(), theme::primary_button("Rinomina")).clicked() {
                            todo = Some(Do::Batch(plan.clone()));
                            keep = false;
                        }
                        if ui.button("Annulla").clicked() || esc {
                            keep = false;
                        }
                    });
                });
            }
            Modal::NewFolder(name) => {
                window("Nuova cartella", ctx).show(ctx, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(320.0).hint_text("Nome della cartella"));
                    if !r.has_focus() && !ui.memory(|m| m.had_focus_last_frame(r.id)) {
                        r.request_focus();
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if (ui.add(theme::primary_button("Crea")).clicked() || enter) && !name.trim().is_empty() {
                            todo = Some(Do::NewFolder(name.clone()));
                            keep = false;
                        }
                        if ui.button("Annulla").clicked() || esc {
                            keep = false;
                        }
                    });
                });
            }
            Modal::Shortcuts => {
                window("Tasti rapidi", ctx).default_width(520.0).show(ctx, |ui| {
                    egui::Grid::new("shortcuts").num_columns(2).spacing([20.0, 6.0]).show(ui, |ui| {
                        for (k, d) in SHORTCUTS {
                            ui.label(egui::RichText::new(*k).strong().color(theme::ACCENT));
                            ui.label(*d);
                            ui.end_row();
                        }
                    });
                    ui.add_space(8.0);
                    if ui.button("Chiudi").clicked() || esc || enter {
                        keep = false;
                    }
                });
            }
        }
        if keep {
            self.modal = Some(modal);
        }
        match todo {
            Some(Do::Trash(paths)) => self.do_trash(&paths),
            Some(Do::Rename(path, name)) => {
                let items: Vec<ops::RenameItem> = vec![ops::RenameItem {
                    path: path.clone(),
                    date: None,
                    camera: String::new(),
                    group: 0,
                }];
                let pattern = name.replace('{', "(").replace('}', ")");
                let mut plan = ops::plan_rename(&items, &pattern, 1);
                // the name was typed with its extension: use it as is
                for p in &mut plan {
                    p.to_name = name.clone();
                    p.problem.clear();
                    let target = p.from.with_file_name(&name);
                    if target != p.from && target.exists() {
                        p.problem = "esiste già un file con questo nome".into();
                    }
                }
                let mut all = plan;
                // the JPEG stacked under a RAW keeps its name in step
                if let (Some(old), Some(new)) = (path.file_stem(), std::path::Path::new(&name).file_stem()) {
                    let old = old.to_string_lossy().into_owned();
                    if let Some(entry) = self.entries.iter().find(|e| e.path == path) {
                        if let Some(j) = self.companions.get(&entry.name).cloned() {
                            if self.settings.stack_pairs {
                                if let Some(je) = self.entries.iter().find(|e| e.name == j) {
                                    let ext = je.path.extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default();
                                    let _ = old;
                                    all.push(ops::Planned {
                                        from: je.path.clone(),
                                        to_name: format!("{}{ext}", new.to_string_lossy()),
                                        problem: String::new(),
                                    });
                                }
                            }
                        }
                    }
                }
                self.finish_rename(&all);
            }
            Some(Do::Batch(plan)) => self.finish_rename(&plan),
            Some(Do::NewFolder(name)) => match ops::new_folder(&self.folder, &name) {
                Ok(p) => {
                    self.status = format!("Cartella creata: {}", p.display());
                    self.status_error = false;
                    self.rescan();
                }
                Err(e) => {
                    self.status = format!("Cartella non creata: {e}");
                    self.status_error = true;
                }
            },
            None => {}
        }
    }
}

fn window(title: &str, _ctx: &egui::Context) -> egui::Window<'static> {
    egui::Window::new(title.to_owned())
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
}
