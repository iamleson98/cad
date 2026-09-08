//! UI panels: toolbar, feature tree, inspector, status bar, palette.

use crate::app::ForgeApp;
use crate::palette::{entries, fuzzy_score, PaletteAction};
use forge_model::{Command, DimField, Feature};

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

        if ui
            .add(egui::Button::new("＋ Sketch Rect").small())
            .clicked()
        {
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
        if ui
            .button(proj_label)
            .on_hover_text("Projection (P)")
            .clicked()
        {
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
            .button(if app.render_options.show_grid {
                "Grid ✓"
            } else {
                "Grid"
            })
            .on_hover_text("Ground grid (G)")
            .clicked()
        {
            PaletteAction::ToggleGrid.run(app);
        }
        if ui
            .button(if app.render_options.show_edges {
                "Edges ✓"
            } else {
                "Edges"
            })
            .on_hover_text("Feature edges (E)")
            .clicked()
        {
            PaletteAction::ToggleEdges.run(app);
        }

        ui.separator();

        // Display modes (W-05) + section view (W-02).
        let mode = app.render_options.display_mode;
        egui::ComboBox::from_id_salt("display-mode")
            .selected_text(match mode {
                forge_render::DisplayMode::Shaded => "◑ Shaded",
                forge_render::DisplayMode::Wireframe => "⌗ Wireframe",
                forge_render::DisplayMode::XRay => "◐ X-Ray",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut app.render_options.display_mode,
                    forge_render::DisplayMode::Shaded,
                    "◑ Shaded",
                );
                ui.selectable_value(
                    &mut app.render_options.display_mode,
                    forge_render::DisplayMode::Wireframe,
                    "⌗ Wireframe (hidden line)",
                );
                ui.selectable_value(
                    &mut app.render_options.display_mode,
                    forge_render::DisplayMode::XRay,
                    "◐ X-Ray (translucent)",
                );
            });
        if ui
            .button(if app.render_options.section.is_some() {
                "Section ✓"
            } else {
                "Section"
            })
            .on_hover_text("Section view: cut the model with a plane")
            .clicked()
        {
            PaletteAction::ToggleSection.run(app);
        }
        if ui
            .button(if app.measure_mode {
                "Measure ✓"
            } else {
                "Measure"
            })
            .on_hover_text("Measure: click two surface points for distance + angle")
            .clicked()
        {
            PaletteAction::ToggleMeasure.run(app);
        }

        ui.separator();

        // Gizmo mode (W-01): translate / rotate manipulator.
        if ui
            .add(
                egui::Button::new("⇔ Move")
                    .small()
                    .selected(app.gizmo_mode == crate::gizmo::GizmoMode::Translate),
            )
            .on_hover_text("Gizmo translate mode (T) — select a body and drag an axis")
            .clicked()
        {
            PaletteAction::GizmoTranslate.run(app);
        }
        if ui
            .add(
                egui::Button::new("⟳ Rotate")
                    .small()
                    .selected(app.gizmo_mode == crate::gizmo::GizmoMode::Rotate),
            )
            .on_hover_text("Gizmo rotate mode (R) — select a body and drag a ring")
            .clicked()
        {
            PaletteAction::GizmoRotate.run(app);
        }

        // Selection granularity (W-04): bodies / faces / edges / vertices.
        let mut mode = app.pick_mode;
        egui::ComboBox::from_id_salt("pick-mode")
            .selected_text(format!("Select: {}", mode.label()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mode, crate::picking::PickMode::Bodies, "Bodies");
                ui.selectable_value(&mut mode, crate::picking::PickMode::Faces, "Faces");
                ui.selectable_value(
                    &mut mode,
                    crate::picking::PickMode::Edges,
                    "Edges (sharp chains)",
                );
                ui.selectable_value(&mut mode, crate::picking::PickMode::Vertices, "Vertices");
            });
        if mode != app.pick_mode {
            app.pick_mode = mode;
            app.selection.clear();
            app.set_status(format!("Picking {}", mode.label().to_lowercase()));
        }
        if app.pick_mode == crate::picking::PickMode::Faces
            && ui
                .button("Sketch on face")
                .on_hover_text("Create a sketch on the selected planar face (W-04)")
                .clicked()
        {
            PaletteAction::SketchOnSelectedFace.run(app);
        }
        if let Some(section) = &mut app.render_options.section {
            let mut axis = if section.normal[0] != 0.0 {
                0
            } else if section.normal[1] != 0.0 {
                1
            } else {
                2
            };
            egui::ComboBox::from_id_salt("section-axis")
                .selected_text(["X", "Y", "Z"][axis])
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut axis, 0, "cut ⟂ X");
                    ui.selectable_value(&mut axis, 1, "cut ⟂ Y");
                    ui.selectable_value(&mut axis, 2, "cut ⟂ Z");
                });
            let mut normal = [0.0f64; 3];
            normal[axis] = if section.normal[axis] != 0.0 {
                section.normal[axis].signum()
            } else {
                1.0
            };
            section.normal = normal;
            let mut offset = section.offset;
            let range = 200.0;
            ui.add(
                egui::Slider::new(&mut offset, -range..=range)
                    .text("offset")
                    .smart_aim(false),
            );
            if ui.button("Flip").clicked() {
                section.normal = [-normal[0], -normal[1], -normal[2]];
                section.offset = -section.offset;
            }
            section.offset = offset;
        }

        ui.separator();

        if ui
            .button("Save")
            .on_hover_text("Save .forgecad (Ctrl+S)")
            .clicked()
        {
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
                egui::RichText::new(
                    "No features yet.\nUse the palette (Ctrl+Shift+P)\nto add solids and sketches.",
                )
                .weak(),
            );
            return;
        }

        for id in order {
            let Some(node) = app.doc.tree.get(id).cloned() else {
                continue;
            };
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
                Feature::LinearPattern(_) => "⋮",
                Feature::CircularPattern(_) => "◍",
                Feature::Mirror(_) => "⇄",
                Feature::Hole(_) => "⌾",
                Feature::Datum(_) => "▱",
                Feature::ImportedMesh(_) => "⤓",
            };

            // S-05: sketch diagnostics badge (DOF / over-constrained).
            let dof_badge: Option<String> = match &node.feature {
                Feature::Sketch(_) => app
                    .last_evaluation
                    .as_ref()
                    .and_then(|ev| ev.sketch_reports.get(&id))
                    .map(|r| {
                        if r.dof_balance > 0 {
                            format!("{} DOF", r.dof_balance)
                        } else if r.dof_balance < 0 {
                            format!("over-constrained {}", -r.dof_balance)
                        } else {
                            "fully constrained".to_string()
                        }
                    }),
                _ => None,
            };

            let mut title = format!("{icon} {label}");
            if let Some(badge) = &dof_badge {
                title = format!("{title} — {badge}");
            }
            if node.suppressed {
                title = format!("∅ {title}");
            }
            if has_error {
                title = format!("⚠ {title}");
            }

            ui.horizontal(|ui| {
                let text = egui::RichText::new(title);
                let text = if selected { text.strong() } else { text };
                if ui.selectable_label(selected, text).clicked() {
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

    /// Text field binding a feature dimension to an expression (P-01).
    /// The stored numeric value stays the fallback; the binding (when the
    /// expression parses) drives the dimension instead.
    fn dim_binding_field(
        ui: &mut egui::Ui,
        app: &mut ForgeApp,
        id: forge_core::FeatureId,
        field: DimField,
        stored: f64,
    ) {
        let current = app
            .doc
            .binding(id, field)
            .map(|b| b.expression.clone())
            .unwrap_or_default();
        let bound = !current.is_empty();
        ui.label(format!(
            "{} = {:.2}{}",
            field,
            stored,
            if bound { "  [bound]" } else { "" }
        ));
        let mut expr = current.clone();
        let response = ui.text_edit_singleline(&mut expr);
        response.on_hover_text("Expression (e.g. `2*th + 1`); empty = numeric value");
        if ui.button("Bind").clicked() {
            let before = app.doc.binding(id, field).map(|b| b.expression.clone());
            let after = if expr.trim().is_empty() {
                None
            } else {
                Some(expr.trim().to_string())
            };
            let _ = app.commands.execute(
                Command::SetBinding {
                    feature: id,
                    field,
                    before,
                    after,
                },
                &mut app.doc,
            );
            app.request_evaluation();
        }
    }

    let edit =
        |app: &mut ForgeApp, new_feature: Feature| edit_feature(app, id, &feature, new_feature);

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
            ui.add(egui::Slider::new(&mut distance, 0.1..=200.0).text("distance (mm)"));
            if distance != p.distance {
                let mut newp = p.clone();
                newp.distance = distance;
                edit(app, Feature::Extrude(newp));
            }
            // F-06: draft / taper angle.
            let mut draft = p.draft_angle.to_degrees();
            let draft_response =
                ui.add(egui::Slider::new(&mut draft, -30.0..=30.0).text("draft (deg)"));
            draft_response.on_hover_text(
                "Taper: the far cap shrinks (positive) or grows (negative) toward the profile centroid",
            );
            let new_draft = draft.to_radians();
            if (new_draft - p.draft_angle).abs() > 1e-9 {
                let mut newp = p.clone();
                newp.draft_angle = new_draft;
                edit(app, Feature::Extrude(newp));
            }
            ui.separator();
            ui.label(format!("Operation: {}", p.operation));
            ui.label(format!("Direction: {:?}", p.direction));
            // P-01: bind the distance to a parameter expression.
            ui.separator();
            dim_binding_field(ui, app, id, DimField::ExtrudeDistance, p.distance);
        }
        Feature::Revolve(p) => {
            let mut angle = p.angle.to_degrees();
            ui.add(egui::Slider::new(&mut angle, 1.0..=360.0).text("angle (deg)"));
            let new_angle = angle.to_radians();
            if (new_angle - p.angle).abs() > 1e-9 {
                let mut newp = p.clone();
                newp.angle = new_angle;
                edit(app, Feature::Revolve(newp));
            }
            dim_binding_field(ui, app, id, DimField::RevolveAngle, p.angle);
        }
        Feature::Sketch(sketch) => {
            ui.label(format!("Plane: {:?}", sketch.plane));
            ui.label(format!("Entities: {}", sketch.entities.len()));
            ui.label(format!("Constraints: {}", sketch.constraints.len()));
            // S-05: live diagnostics from the background evaluation.
            if let Some(report) = app
                .last_evaluation
                .as_ref()
                .and_then(|ev| ev.sketch_reports.get(&id))
            {
                let dof = if report.dof_balance > 0 {
                    format!("{} DOF remaining", report.dof_balance)
                } else if report.dof_balance < 0 {
                    format!("over-constrained by {}", -report.dof_balance)
                } else {
                    "fully constrained".to_string()
                };
                ui.label(format!(
                    "Solver: {} ({dof}, residual {:.2e}, {} iters)",
                    report.status, report.residual, report.iterations
                ));
            } else if let Some(report) = &app.last_sketch_report {
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
        Feature::TransformBody {
            source,
            translation,
            rotation,
            pivot,
        } => {
            let mut t = *translation;
            ui.add(egui::Slider::new(&mut t.x, -200.0..=200.0).text("translate X"));
            ui.add(egui::Slider::new(&mut t.y, -200.0..=200.0).text("translate Y"));
            ui.add(egui::Slider::new(&mut t.z, -200.0..=200.0).text("translate Z"));
            ui.weak(format!(
                "Rotation: {:.1}\u{00b0} / {:.1}\u{00b0} / {:.1}\u{00b0} about ({:.1}, {:.1}, {:.1}) \u{2014} drag the viewport gizmo",
                rotation.x.to_degrees(),
                rotation.y.to_degrees(),
                rotation.z.to_degrees(),
                pivot.x,
                pivot.y,
                pivot.z
            ));
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
                        pivot: *pivot,
                    },
                );
            }
        }
        Feature::LinearPattern(p) => {
            let mut count = p.count as i32;
            let mut spacing = p.spacing;
            let mut symmetric = p.symmetric;
            egui::Grid::new("linpat").num_columns(2).show(ui, |ui| {
                ui.label("Count");
                ui.add(egui::DragValue::new(&mut count).range(2..=200));
                ui.end_row();
                ui.label("Spacing");
                ui.add(
                    egui::DragValue::new(&mut spacing)
                        .speed(0.5)
                        .range(0.1..=500.0),
                );
                ui.end_row();
            });
            ui.checkbox(&mut symmetric, "Symmetric about the seed");
            ui.label(format!(
                "Direction: ({:.2}, {:.2}, {:.2})  |  Operation: {}",
                p.direction.x, p.direction.y, p.direction.z, p.operation
            ));
            if count != p.count as i32
                || (spacing - p.spacing).abs() > 1e-9
                || symmetric != p.symmetric
            {
                let mut newp = p.clone();
                newp.count = count.max(2) as usize;
                newp.spacing = spacing;
                newp.symmetric = symmetric;
                edit(app, Feature::LinearPattern(newp));
            }
        }
        Feature::CircularPattern(p) => {
            let mut count = p.count as i32;
            let mut angle = p.angle.to_degrees();
            egui::Grid::new("circpat").num_columns(2).show(ui, |ui| {
                ui.label("Count");
                ui.add(egui::DragValue::new(&mut count).range(2..=200));
                ui.end_row();
                ui.label("Span");
                ui.add(
                    egui::DragValue::new(&mut angle)
                        .speed(1.0)
                        .range(1.0..=360.0)
                        .suffix("\u{00b0}"),
                );
                ui.end_row();
            });
            ui.label(format!(
                "Axis: ({:.2}, {:.2}, {:.2}) through ({:.1}, {:.1}, {:.1})  |  Operation: {}",
                p.axis_dir.x,
                p.axis_dir.y,
                p.axis_dir.z,
                p.axis_point.x,
                p.axis_point.y,
                p.axis_point.z,
                p.operation
            ));
            let new_angle = angle.to_radians();
            if count != p.count as i32 || (new_angle - p.angle).abs() > 1e-9 {
                let mut newp = p.clone();
                newp.count = count.max(2) as usize;
                newp.angle = new_angle;
                edit(app, Feature::CircularPattern(newp));
            }
        }
        Feature::Mirror(p) => {
            let mut plane_x = p.plane_point.x;
            ui.add(
                egui::Slider::new(&mut plane_x, -200.0..=200.0).text("mirror plane X (YZ offset)"),
            );
            ui.label(format!(
                "Plane normal: ({:.2}, {:.2}, {:.2})  |  Operation: {}",
                p.plane_normal.x, p.plane_normal.y, p.plane_normal.z, p.operation
            ));
            if (plane_x - p.plane_point.x).abs() > 1e-9 {
                let mut newp = p.clone();
                newp.plane_point.x = plane_x;
                edit(app, Feature::Mirror(newp));
            }
            dim_binding_field(ui, app, id, DimField::MirrorOffset, p.plane_point.x);
        }
        Feature::Hole(p) => {
            // F-04: hole wizard parameters.
            let mut newp = p.clone();
            let kinds = [
                forge_model::HoleKind::Simple,
                forge_model::HoleKind::Counterbore,
                forge_model::HoleKind::Countersink,
            ];
            let selected = kinds.iter().position(|k| *k == p.kind).unwrap_or(0);
            let mut picked = selected;
            egui::ComboBox::from_id_salt("hole-kind")
                .selected_text(format!("{}", p.kind))
                .show_ui(ui, |ui| {
                    for (i, k) in kinds.iter().enumerate() {
                        ui.selectable_value(&mut picked, i, format!("{k}"));
                    }
                });
            newp.kind = kinds[picked];
            egui::Grid::new("hole-params")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Diameter");
                    ui.add(
                        egui::DragValue::new(&mut newp.diameter)
                            .speed(0.1)
                            .range(0.1..=200.0)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                    ui.label("Depth");
                    ui.add(
                        egui::DragValue::new(&mut newp.depth)
                            .speed(0.5)
                            .range(0.1..=500.0)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                    if newp.kind == forge_model::HoleKind::Counterbore {
                        ui.label("Counterbore \u{2300}");
                        ui.add(
                            egui::DragValue::new(&mut newp.counterbore_diameter)
                                .speed(0.1)
                                .range(0.1..=300.0)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                        ui.label("Counterbore depth");
                        ui.add(
                            egui::DragValue::new(&mut newp.counterbore_depth)
                                .speed(0.1)
                                .range(0.1..=100.0)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                    }
                    if newp.kind == forge_model::HoleKind::Countersink {
                        ui.label("Countersink \u{2300}");
                        ui.add(
                            egui::DragValue::new(&mut newp.countersink_diameter)
                                .speed(0.1)
                                .range(0.1..=300.0)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                        ui.label("Countersink angle");
                        let mut cs = newp.countersink_angle.to_degrees();
                        ui.add(
                            egui::DragValue::new(&mut cs)
                                .speed(1.0)
                                .range(10.0..=170.0)
                                .suffix("\u{00b0}"),
                        );
                        newp.countersink_angle = cs.to_radians();
                        ui.end_row();
                    }
                    ui.end_row();
                });
            ui.checkbox(&mut newp.drill_point, "Drill point (conical bottom)");
            if newp.drill_point {
                let mut dp = newp.drill_angle.to_degrees();
                ui.add(egui::Slider::new(&mut dp, 60.0..=180.0).text("drill angle"));
                newp.drill_angle = dp.to_radians();
            }
            let mut dir = match newp.direction {
                forge_geometry::ExtrudeDirection::Positive => 0usize,
                forge_geometry::ExtrudeDirection::Negative => 1usize,
                forge_geometry::ExtrudeDirection::Symmetric => 2usize,
            };
            egui::ComboBox::from_id_salt("hole-dir")
                .selected_text(["Positive", "Negative", "Both"].dir_label(dir))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut dir, 0, "Positive (along normal)");
                    ui.selectable_value(&mut dir, 1, "Negative (against normal)");
                    ui.selectable_value(&mut dir, 2, "Both directions");
                });
            newp.direction = match dir {
                0 => forge_geometry::ExtrudeDirection::Positive,
                1 => forge_geometry::ExtrudeDirection::Negative,
                _ => forge_geometry::ExtrudeDirection::Symmetric,
            };
            if newp != *p {
                edit(app, Feature::Hole(newp));
            }
            ui.separator();
            dim_binding_field(ui, app, id, DimField::HoleDiameter, p.diameter);
            dim_binding_field(ui, app, id, DimField::HoleDepth, p.depth);
        }
        Feature::Datum(d) => {
            // D-01: datum plane parameters.
            let mut newd = *d;
            match d {
                forge_model::DatumParams::Offset { base, offset } => {
                    ui.label(format!("Offset plane from {base:?}"));
                    let mut off = *offset;
                    ui.add(egui::Slider::new(&mut off, -200.0..=200.0).text("offset (mm)"));
                    if let forge_model::DatumParams::Offset { offset, .. } = &mut newd {
                        *offset = off;
                    }
                }
                forge_model::DatumParams::Angle { base, axis, angle } => {
                    ui.label(format!("Tilted plane from {base:?}"));
                    let mut a = angle.to_degrees();
                    let mut ax = *axis;
                    ui.add(egui::Slider::new(&mut a, -89.0..=89.0).text("tilt (deg)"));
                    egui::ComboBox::from_id_salt("datum-axis")
                        .selected_text(if ax == 0 { "about X" } else { "about Y" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut ax, 0, "about local X");
                            ui.selectable_value(&mut ax, 1, "about local Y");
                        });
                    if let forge_model::DatumParams::Angle { axis, angle, .. } = &mut newd {
                        *axis = ax;
                        *angle = a.to_radians();
                    }
                }
            }
            if newd != *d {
                edit(app, Feature::Datum(newd));
            }
            ui.label(
                egui::RichText::new(
                    "Datums are construction geometry: create sketches on them\n(palette: New Sketch on latest datum).",
                )
                .small()
                .weak(),
            );
        }
        Feature::ImportedMesh(p) => {
            // I-01: read-only summary of the imported mesh body.
            ui.label(format!("Source: {}", p.source));
            ui.label(format!("Triangles: {}", p.mesh.tri_count()));
            ui.label(format!("Vertices: {}", p.mesh.vertex_count()));
            let closed = p.mesh.is_closed();
            ui.label(format!(
                "Watertight: {}",
                if closed { "yes" } else { "no (open mesh)" }
            ));
            if closed {
                ui.label(format!("Volume: {:.2} mm³", p.mesh.volume_signed()));
            }
            ui.label(
                egui::RichText::new(
                    "Imported meshes are parametric-free bodies: booleans,\npatterns, mirrors and transforms all work on them.",
                )
                .small()
                .weak(),
            );
        }
        _ => {
            ui.label("No parameters for this feature yet.");
        }
    }
}

/// Label helper for the hole direction combo.
trait DirLabel {
    fn dir_label(&self, i: usize) -> String;
}
impl DirLabel for [&str; 3] {
    fn dir_label(&self, i: usize) -> String {
        self[i.min(2)].to_string()
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
        // S-05: live sketch diagnostics for the selected sketch.
        if let Some(sketch_id) = app.selection.primary_feature() {
            if let Some(report) = app
                .last_evaluation
                .as_ref()
                .and_then(|ev| ev.sketch_reports.get(&sketch_id))
            {
                let text = if report.dof_balance > 0 {
                    format!("sketch: {} DOF", report.dof_balance)
                } else if report.dof_balance < 0 {
                    format!("sketch: over-constrained by {}", -report.dof_balance)
                } else {
                    "sketch: fully constrained".to_string()
                };
                let color = if report.dof_balance < 0 {
                    egui::Color32::from_rgb(220, 120, 100)
                } else {
                    egui::Color32::from_rgb(140, 200, 140)
                };
                ui.label(egui::RichText::new(text).color(color));
                ui.separator();
            }
        }
        ui.label(format!("features: {}", app.doc.tree.len()));
        if !app.doc.params.is_empty() {
            ui.separator();
            ui.label(format!("params: {}", app.doc.params.len()));
        }
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

/// P-01: the user parameter table (name, expression or value, unit) with
/// undoable edits and per-row deletion. Angle parameters display degrees
/// (stored as radians).
pub fn params_panel(ui: &mut egui::Ui, app: &mut ForgeApp) {
    ui.heading("Parameters");
    ui.separator();
    if app.doc.params.is_empty() {
        ui.label(
            egui::RichText::new(
                "No parameters.\nAdd one and bind dimensions to it,\ne.g. `width = 2*th + 1`.",
            )
            .small()
            .weak(),
        );
    }
    let ids: Vec<forge_core::ParamId> = app.doc.params.keys().copied().collect();
    for pid in ids {
        let Some(param) = app.doc.params.get(&pid).cloned() else {
            continue;
        };
        let mut name = param.name.clone();
        let mut expr = param.expression.clone().unwrap_or_default();
        // Value edits happen in display units (degrees for angles).
        let mut display_value = if param.is_angle {
            param.value.to_degrees()
        } else {
            param.value
        };
        let mut apply_clicked = false;
        let mut delete_clicked = false;
        egui::Grid::new(format!("param-{pid:?}"))
            .num_columns(3)
            .show(ui, |ui| {
                ui.text_edit_singleline(&mut name);
                if param.expression.is_some() {
                    // Expression-driven: the value is computed, not edited.
                    ui.text_edit_singleline(&mut expr);
                    ui.label(format!(
                        "= {:.4}{}",
                        display_value,
                        if param.is_angle { "\u{00b0}" } else { "mm" }
                    ));
                } else {
                    // Plain value: expr field empty = value edit.
                    ui.text_edit_singleline(&mut expr);
                    ui.add(
                        egui::DragValue::new(&mut display_value)
                            .speed(0.1)
                            .suffix(if param.is_angle { "\u{00b0}" } else { "mm" }),
                    );
                }
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply_clicked = true;
                    }
                    if ui.button("\u{2715}").clicked() {
                        delete_clicked = true;
                    }
                });
                ui.end_row();
            });
        if delete_clicked {
            let _ = app.commands.execute(
                Command::SetParam {
                    id: pid,
                    before: Some(param.clone()),
                    after: None,
                },
                &mut app.doc,
            );
            app.request_evaluation();
            continue;
        }
        if apply_clicked {
            let expr_trim = expr.trim();
            let mut new_param = param.clone();
            new_param.name = name.trim().to_string();
            if expr_trim.is_empty() {
                new_param.expression = None;
                new_param.value = if param.is_angle {
                    display_value.to_radians()
                } else {
                    display_value
                };
                let _ = app.commands.execute(
                    Command::SetParam {
                        id: pid,
                        before: Some(param.clone()),
                        after: Some(new_param),
                    },
                    &mut app.doc,
                );
                app.request_evaluation();
            } else {
                // Validate the expression before recording the command.
                match forge_model::expr::validate(expr_trim) {
                    Err(e) => app.set_status(format!("expression: {}", e.message)),
                    Ok(_) => {
                        new_param.expression = Some(expr_trim.to_string());
                        let _ = app.commands.execute(
                            Command::SetParam {
                                id: pid,
                                before: Some(param.clone()),
                                after: Some(new_param),
                            },
                            &mut app.doc,
                        );
                        // Re-resolve immediately so the table shows the
                        // computed value (the worker re-resolves too).
                        if let Err(e) = app.doc.resolve_params() {
                            app.set_status(format!("{e}"));
                        }
                        app.request_evaluation();
                    }
                }
            }
        }
        ui.add_space(2.0);
    }
    if ui.button("\u{ff0b} Add parameter").clicked() {
        let pid = app.doc.next_param_id();
        let mut p = forge_model::Param::length("new_param", 10.0);
        p.id = pid;
        let _ = app.commands.execute(
            Command::SetParam {
                id: pid,
                before: None,
                after: Some(p),
            },
            &mut app.doc,
        );
        app.request_evaluation();
    }
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
                    fuzzy_score(&query, e.label, e.keywords).map(|s| (s, i, e.label, e.action))
                })
                .collect();
            scored.sort_by_key(|(s, i, _, _)| *s * 1000 + *i);

            let mut run_idx: Option<PaletteAction> = None;
            let mut hovered = false;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (rank, idx, label, action) in scored.iter().take(12) {
                    let selected = *idx == 0 && query.is_empty();
                    let _ = selected;
                    let response =
                        ui.selectable_label(false, format!("{:>4}  {label}", (*rank).min(999)));
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
