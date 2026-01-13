use crate::app::Mp3rgainApp;

pub fn render(app: &mut Mp3rgainApp, ui: &mut egui::Ui) {
    egui::ScrollArea::both().show(ui, |ui| {
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::auto().at_least(250.0)) // Path/File
            .column(egui_extras::Column::auto().at_least(70.0)) // Volume
            .column(egui_extras::Column::auto().at_least(50.0)) // Clipping
            .column(egui_extras::Column::auto().at_least(80.0)) // Track Gain
            .column(egui_extras::Column::auto().at_least(50.0)) // Clip (Track)
            .column(egui_extras::Column::auto().at_least(80.0)) // Album Volume
            .column(egui_extras::Column::auto().at_least(80.0)) // Album Gain
            .column(egui_extras::Column::auto().at_least(50.0)) // Clip (Album)
            .column(egui_extras::Column::remainder()) // Status
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Path/File");
                });
                header.col(|ui| {
                    ui.strong("Volume");
                });
                header.col(|ui| {
                    ui.strong("Clip");
                });
                header.col(|ui| {
                    ui.strong("Track Gain");
                });
                header.col(|ui| {
                    ui.strong("Clip(T)");
                });
                header.col(|ui| {
                    ui.strong("Album Vol");
                });
                header.col(|ui| {
                    ui.strong("Album Gain");
                });
                header.col(|ui| {
                    ui.strong("Clip(A)");
                });
                header.col(|ui| {
                    ui.strong("Status");
                });
            })
            .body(|mut body| {
                for (idx, file) in app.files.iter().enumerate() {
                    let is_selected = app.selected_indices.contains(&idx);
                    body.row(18.0, |mut row| {
                        row.set_selected(is_selected);

                        row.col(|ui| {
                            if ui.selectable_label(is_selected, &file.filename).clicked() {
                                if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
                                    if is_selected {
                                        app.selected_indices.retain(|&i| i != idx);
                                    } else {
                                        app.selected_indices.push(idx);
                                    }
                                } else {
                                    app.selected_indices.clear();
                                    app.selected_indices.push(idx);
                                }
                            }
                        });
                        row.col(|ui| {
                            if let Some(v) = file.volume {
                                ui.label(format!("{:.1}", v));
                            }
                        });
                        row.col(|ui| {
                            if file.clipping {
                                ui.colored_label(egui::Color32::RED, "Y");
                            }
                        });
                        row.col(|ui| {
                            if let Some(g) = file.track_gain {
                                let color = if file.track_clip {
                                    egui::Color32::RED
                                } else {
                                    ui.style().visuals.text_color()
                                };
                                ui.colored_label(color, format!("{:+.1} dB", g));
                            }
                        });
                        row.col(|ui| {
                            if file.track_clip {
                                ui.colored_label(egui::Color32::RED, "Y");
                            }
                        });
                        row.col(|ui| {
                            if let Some(v) = file.album_volume {
                                ui.label(format!("{:.1}", v));
                            }
                        });
                        row.col(|ui| {
                            if let Some(g) = file.album_gain {
                                let color = if file.album_clip {
                                    egui::Color32::RED
                                } else {
                                    ui.style().visuals.text_color()
                                };
                                ui.colored_label(color, format!("{:+.1} dB", g));
                            }
                        });
                        row.col(|ui| {
                            if file.album_clip {
                                ui.colored_label(egui::Color32::RED, "Y");
                            }
                        });
                        row.col(|ui| {
                            ui.label(file.status.as_str());
                        });
                    });
                }
            });
    });
}
