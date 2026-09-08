//! UI panels: toolbar, feature tree, inspector, status bar, palette.

use crate::app::ForgeApp;
use crate::palette::{entries, fuzzy_score, PaletteAction};
use forge_model::{Command, Feature};

/// Top toolbar.
pub fn toolbar(ui: &mut egui::Ui, app: &mut ForgeApp) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("ForgeCAD").strong().size(15.0));

        ui.separator();

        // Undo / redo.
        let undo = egui::Button::new("⟲ Undo").small();
        if ui
            .add_enabled(app.commands.can_undo(), undo)
            .on_disabled_hover_text("Nothing to undo")
            .clicked()
        {
            PaletteAction::Undo.run(app);
        }
        let redo = egui::Button::new("⟳ Redo").small();
        if ui
            .add_enabled(app.commands.can_redo(), redo)
            .on_disabled_hover_text("Nothing to redo")
            .clicked()
        {
            PaletteAction::Redo.run(app);
        }

        ui.separator();

        // Primitives quick menu.
        let prim = egui::Button::new("＋ Primitive ▾").small();
        let response = ui.menu_button("＋ Solid", |ui| {
            for (kind, dims) in [
                (forge_model::PrimitiveKind::Box, [40.0, 30.0, 20.0]),
                (forge_model::PrimitiveKind::Sphere, [20.0, 0.0, 0.0]),
                (forge_model::PrimitiveKind::Cylinder, [10.0, 30.0, 0.0]),
                (forge_model::PrimitiveKind::Cone, [12.0, 6.0, 25.0]),
                (forge_model::PrimitiveKind::Torus, [20.0, 6.0, 0.0]),
            ] {
                let dims = forge_core::Vector3::new(dims[0], dims[1], dims[2]);
                if ui.button(kind.to_string()).clicked() {
                    app.add_primitive(kind, dims);
                }
            }
        });
        let _ = prim;
        let _ = response;

        if ui.add(egui::Button::new("＋ Sketch Rect").small()).clicked() {
            PaletteAction::NewSketchXY.run(app);
        }
        if ui.add(egui::Button::new("Extrude ▲").small()).clicked() {
            PaletteAction::ExtrudeLastSketch.run(app);
        }
        if ui.add(egui::Button::new("Drill ⌾").small()).clicked() {
            PaletteAction::CutWithCylinder.run(app);
        }

        ui.separator();

        // View toggles.
        let proj_label = if app.camera.orthographic {
            "Ortho"
        } else {
            "Persp"
        };
        if ui.button(proj_label).on_hover_text("Projection (P)").clicked() {
            PaletteAction::ToggleProjection.run(app);
        }
        if ui
            .add(egui::Button::new("Fit").small())
            .on_hover_text("Fit view (F)")
            .clicked()
        {
            PaletteAction::FitView.run(app);
        }
        if ui
            .button(if app.render_options.show_grid { "Grid ✓" } else { "Grid" })
            .on_hover_text("Ground grid (G)")
            .clicked()
        {
            PaletteAction::ToggleGrid.run(app);
        }
        if ui
            .button(if app.render_options.show_edges { "Edges ✓" } else { "Edges" })
            .on_hover_text("Feature edges (E)")
            .clicked()
        {
            PaletteAction::ToggleEdges.run(app);
        }

        ui.separator();

        if ui.button("Save").on_hover_text("Save .forgecad (Ctrl+S)").clicked() {
            PaletteAction::SaveNative.run(app);
        }
        let export = ui.menu_button("Export", |ui| {
            if ui.button("STL (binary)").clicked() {
                app.export_mesh(forge_io::ExportFormat::Stl);
            }
            if ui.button("OBJ").clicked() {
                app.export_mesh(forge_io::ExportFormat::Obj);
            }
            if ui.button("glTF 2.0").clicked() {
                app.export_mesh(forge_io::ExportFormat::Gltf);
            }
        });
        let _ = export;

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new("⌘ Palette (Ctrl+Shift+P)").small())
                .clicked()
            {
                app.palette_open = !app.palette_open;
                app.palette_query.clear();
            }
        });
    });
}

/// Left panel: the parametric feature tree (FR-SM-04, FR-UI-02).
pub fn tree_panel(ui: &mut egui::Ui, app: &mut ForgeApp) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let order: Vec<forge_core::FeatureId> = app.doc.tree.order().to_vec();
        if order.is_empty() {
            ui.label(
                egui::RichText::new("No features yet.\nUse the palette (Ctrl+Shift+P)\nto add solids and sketches.")
                    .weak(),
            );
            return;
        }

        for id in order {
            let Some(node) = app.doc.tree.get(id).cloned() else { continue };
            let label = node.feature.label();
            let selected = app.selection.primary_feature() == Some(id);
            let has_error = app
                .last_evaluation
                .as_ref()
                .map(|ev| ev.errors.contains_key(&id))
                .unwrap_or(false);

            let icon = match &node.feature {
                Feature::Sketch(_) => "✎",
                Feature::Extrude(_) => "▲",
                Feature::Revolve(_) => "↻",
                Feature::Loft(_) => "⌒",
                Feature::Sweep(_) => "➰",
                Feature::Primitive(_) => "◧",
                Feature::Boolean(_) => "⊕",
                Feature::TransformBody { .. } => "✥",
            };

            let mut title = format!("{icon} {label}");
            if node.suppressed {
                title = format!("∅ {title}");
            }
            if has_error {
                title = format!("⚠ {title}");
            }

            ui.horizontal(|ui| {
                let text = egui::RichText::new(title);
                let text = if selected {
                    text.strong()
                } else {
                    text
                };
                if ui
                    .selectable_label(selected, text)
                    .clicked()
                {
                    app.selection.select(forge_model::SelectionItem::Body(
                        forge_core::BodyId::new(id.raw()),
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut suppressed = node.suppressed;
                    if ui
                        .checkbox(&mut suppressed, "")
                        .on_hover_text("Suppress feature")
                        .changed()
                    {
                        app.selection.select(forge_model::SelectionItem::Body(
                            forge_core::BodyId::new(id.raw()),
                        ));
                        PaletteAction::SuppressSelected.run(app);
                    }
                });
            });

            // Right-click context menu (FR-UI-02).
            ui.menu_button("  ⋮", |ui| {
                if ui.button("Edit in inspector").clicked() {
                    app.selection.select(forge_model::SelectionItem::Body(
                        forge_core::BodyId::new(id.raw()),
                    ));
                }
                if ui.button("Suppress / unsuppress").clicked() {
                    app.selection.select(forge_model::SelectionItem::Body(
                        forge_core::BodyId::new(id.raw()),
                    ));
                    app.toggle_suppress_selected();
                }
                if ui.button("Delete").clicked() {
                    app.selection.select(forge_model::SelectionItem::Body(
                        forge_core::BodyId::new(id.raw()),
                    ));
                    app.delete_selected();
                }
            });

            // Error text under the row.
            if has_error {
                if let Some(err) = app
                    .last_evaluation
                    .as_ref()
                    .and_then(|ev| ev.errors.get(&id))
                {
                    ui.label(
                        egui::RichText::new(format!("  {err}"))
                            .small()
                            .color(egui::Color32::from_rgb(220, 120, 100)),
                    );
                }
            }
            ui.add_space(2.0);
        }
    });
}

/// Right panel: parameter inspector for the selected feature (FR-UI-03
/// live preview editing).
pub fn inspector(ui: &mut egui::Ui, app: &mut ForgeApp) {
    let Some(id) = app.selection.primary_feature() else {
        ui.label(egui::RichText::new("Select a feature to edit").weak());
        return;
    };
    let Some(feature) = app.doc.feature(id).cloned() else {
        return;
    };

    ui.heading(feature.label());
    ui.separator();

    fn edit_feature(
        app: &mut ForgeApp,
        id: forge_core::FeatureId,
        before: &Feature,
        new_feature: Feature,
    ) {
        if app.doc.edit_feature(id, new_feature.clone()).is_ok() {
            let _ = app.commands.execute(
                Command::EditFeature {
                    id,
                    before: Box::new(before.clone()),
                    after: Box::new(new_feature),
                },
                &mut app.doc,
            );
            app.request_evaluation();
        }
    }
    let edit = |app: &mut ForgeApp, new_feature: Feature| {
        edit_feature(app, id, &feature, new_feature)
    };

    match &feature {
        Feature::Primitive(p) => {
            let mut center = p.center;
            let mut dims = p.dims;
            egui::Grid::new("prim").num_columns(2).show(ui, |ui| {
                ui.label("Center X");
                ui.add(egui::DragValue::new(&mut center.x).speed(0.5));
                ui.end_row();
                ui.label("Center Y");
                ui.add(egui::DragValue::new(&mut center.y).speed(0.5));
                ui.end_row();
                ui.label("Center Z");
                ui.add(egui::DragValue::new(&mut center.z).speed(0.5));
                ui.end_row();
                ui.label("Dim A");
                ui.add(egui::DragValue::new(&mut dims.x).speed(0.5));
                ui.end_row();
                if p.kind != forge_model::PrimitiveKind::Sphere {
                    ui.label("Dim B");
                    ui.add(egui::DragValue::new(&mut dims.y).speed(0.5));
                    ui.end_row();
                }
                ui.end_row();
            });
            ui.label(
                egui::RichText::new("Dim A/B/C meaning per primitive kind:\nBox: dx/dy/dz, Sphere: r,\nCylinder: r/h, Cone: r_base/r_top/h,\nTorus: R/r")
                    .small()
                    .weak(),
            );
            let mut newp = p.clone();
            newp.center = center;
            newp.dims = dims;
            if ui.button("Apply").clicked() {
                edit(app, Feature::Primitive(newp));
            }
        }
        Feature::Extrude(p) => {
            // Live preview: dragging the slider re-evaluates the model
            // (FR-UI-03).
            let mut distance = p.distance;
            ui.add(
                egui::Slider::new(&mut distance, 0.1..=200.0).text("distance (mm)"),
            );
            if distance != p.distance {
                let mut newp = p.clone();
                newp.distance = distance;
                edit(app, Feature::Extrude(newp));
            }
            ui.separator();
            ui.label(format!("Operation: {}", p.operation));
            ui.label(format!("Direction: {:?}", p.direction));
            ui.label(
                egui::RichText::new(
                    "The profile sketch is referenced by the tree.\n3D drag manipulators are a Phase-2 roadmap item.",
                )
                .small()
                .weak(),
            );
        }
        Feature::Revolve(p) => {
            let mut angle = p.angle.to_degrees();
            ui.add(
                egui::Slider::new(&mut angle, 1.0..=360.0).text("angle (deg)"),
            );
            let new_angle = angle.to_radians();
            if (new_angle - p.angle).abs() > 1e-9 {
                let mut newp = p.clone();
                newp.angle = new_angle;
                edit(app, Feature::Revolve(newp));
            }
        }
        Feature::Sketch(sketch) => {
            ui.label(format!("Plane: {:?}", sketch.plane));
            ui.label(format!("Entities: {}", sketch.entities.len()));
            ui.label(format!("Constraints: {}", sketch.constraints.len()));
            if let Some(report) = &app.last_sketch_report {
                ui.label(format!(
                    "Solver: {} (residual {:.2e}, {} iters)",
                    report.status, report.residual, report.iterations
                ));
            }
            let mut s = sketch.clone();
            if ui.button("Solve constraints").clicked() {
                match s.solve() {
                    Ok(report) => {
                        app.last_sketch_report = Some(report);
                        edit(app, Feature::Sketch(s));
                    }
                    Err(e) => app.set_status(format!("{e}")),
                }
            }
        }
        Feature::Boolean(b) => {
            ui.label(format!("Operation: {}", b.op));
            ui.label(format!("Operands: {} bodies", b.operands.len()));
        }
        Feature::TransformBody { source, translation, rotation } => {
            let mut t = *translation;
            ui.add(egui::Slider::new(&mut t.x, -200.0..=200.0).text("translate X"));
            ui.add(egui::Slider::new(&mut t.y, -200.0..=200.0).text("translate Y"));
            ui.add(egui::Slider::new(&mut t.z, -200.0..=200.0).text("translate Z"));
            if (t.x - translation.x).abs() > 1e-9
                || (t.y - translation.y).abs() > 1e-9
                || (t.z - translation.z).abs() > 1e-9
            {
                edit(
                    app,
                    Feature::TransformBody {
                        source: *source,
                        translation: t,
                        rotation: *rotation,
                    },
                );
            }
        }
        _ => {
            ui.label("No parameters for this feature yet.");
        }
    }
}

/// Bottom status bar.
pub fn status_bar(ui: &mut egui::Ui, app: &ForgeApp) {
    ui.horizontal(|ui| {
        let (bodies, tris) = app
            .last_evaluation
            .as_ref()
            .map(|ev| (ev.bodies.len(), ev.total_triangles))
            .unwrap_or((0, 0));
        let eval_state = if app.eval_pending {
            "evaluating…"
        } else {
            "ready"
        };
        ui.label(format!(
            "{eval_state}  |  bodies: {bodies}  |  tris: {tris}"
        ));
        ui.separator();
        ui.label(format!("features: {}", app.doc.tree.len()));
        ui.separator();
        if app.commands.can_undo() {
            ui.label(format!("undo depth: {}", app.commands.undo_depth()));
            ui.separator();
        }
        ui.label("mm");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(&app.status).weak());
        });
    });
}

/// The command palette overlay (FR-UI-01).
pub fn palette_overlay(ctx: &egui::Context, app: &mut ForgeApp) {
    if !app.palette_open {
        return;
    }
    egui::Window::new("Command Palette")
        .anchor(egui::Align2::CENTER_TOP, [0.0, 60.0])
        .fixed_size([520.0, 360.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.text_edit_singleline(&mut app.palette_query);
            ui.add_space(4.0);
            let query = app.palette_query.clone();
            let mut scored: Vec<(usize, usize, &'static str, PaletteAction)> = entries()
                .into_iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    fuzzy_score(&query, e.label, e.keywords)
                        .map(|s| (s, i, e.label, e.action))
                })
                .collect();
            scored.sort_by_key(|(s, i, _, _)| *s * 1000 + *i);

            let mut run_idx: Option<PaletteAction> = None;
            let mut hovered = false;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (rank, idx, label, action) in scored.iter().take(12) {
                    let selected = *idx == 0 && query.is_empty();
                    let _ = selected;
                    let response = ui.selectable_label(false, format!("{:>4}  {label}", (*rank).min(999)));
                    if response.clicked() {
                        run_idx = Some(*action);
                    }
                    if response.hovered() {
                        hovered = true;
                    }
                }
            });
            let _ = hovered;
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.palette_open = false;
            }
            if let Some(action) = run_idx {
                app.palette_open = false;
                action.run(app);
            }
        });
}
