//! UI panels: toolbar, feature tree, inspector, status bar, palette.

use crate::app::ForgeApp;
use crate::palette::{entries, fuzzy_score, PaletteAction};
use forge_model::{Command, DimField, Feature};

/// Top toolbar: grouped animated icon buttons (theme::tool_button) with
/// tooltips that carry the keyboard shortcut.
pub fn toolbar(ui: &mut egui::Ui, app: &mut ForgeApp) {
    use crate::icons;
    use crate::theme::tool_button;

    ui.horizontal_wrapped(|ui| {
        ui.set_min_height(34.0);

        // -- Brand ----------------------------------------------------------
        ui.horizontal(|ui| {
            ui.label(icons::colored(icons::HAMMER, 17.0, crate::theme::ACCENT));
            ui.label(crate::theme::semibold("ForgeCAD").size(14.5));
        });

        ui.separator();

        // -- History --------------------------------------------------------
        if tool_button(
            ui,
            "undo",
            icons::UNDO,
            None,
            "Undo\nCtrl+Z",
            false,
            app.commands.can_undo(),
        )
        .clicked()
        {
            PaletteAction::Undo.run(app);
        }
        if tool_button(
            ui,
            "redo",
            icons::REDO,
            None,
            "Redo\nCtrl+Y",
            false,
            app.commands.can_redo(),
        )
        .clicked()
        {
            PaletteAction::Redo.run(app);
        }

        ui.separator();

        // -- Create ---------------------------------------------------------
        let solid_menu = ui.menu_button(chevron_label(icons::PLUS, "Solid"), |ui| {
            for (kind, icon, dims) in [
                (
                    forge_model::PrimitiveKind::Box,
                    icons::BOX,
                    [40.0, 30.0, 20.0],
                ),
                (
                    forge_model::PrimitiveKind::Sphere,
                    icons::SPHERE,
                    [20.0, 0.0, 0.0],
                ),
                (
                    forge_model::PrimitiveKind::Cylinder,
                    icons::CYLINDER,
                    [10.0, 30.0, 0.0],
                ),
                (
                    forge_model::PrimitiveKind::Cone,
                    icons::CONE,
                    [12.0, 6.0, 25.0],
                ),
                (
                    forge_model::PrimitiveKind::Torus,
                    icons::TORUS,
                    [20.0, 6.0, 0.0],
                ),
            ] {
                let dims = forge_core::Vector3::new(dims[0], dims[1], dims[2]);
                let response = ui.button(icons::icon_label(icon, &kind.to_string()));
                crate::bridge::record(
                    format!("solid:{kind}"),
                    kind.to_string(),
                    "button",
                    response.rect,
                    response.enabled(),
                );
                if response.clicked() {
                    app.add_primitive(kind, dims);
                }
            }
        });
        // E2E bridge: the Solid menu trigger (menu_button returns the
        // trigger's response — record its exact rect).
        crate::bridge::record(
            "menu:Solid",
            "Solid",
            "menu",
            solid_menu.response.rect,
            true,
        );
        if tool_button(
            ui,
            "sketch-xy",
            icons::SKETCH,
            Some("Sketch"),
            "New sketch: rectangle on the XY plane",
            false,
            true,
        )
        .clicked()
        {
            PaletteAction::NewSketchXY.run(app);
        }
        if tool_button(
            ui,
            "extrude",
            icons::EXTRUDE,
            Some("Extrude"),
            "Extrude the latest sketch into a solid",
            false,
            true,
        )
        .clicked()
        {
            PaletteAction::ExtrudeLastSketch.run(app);
        }
        if tool_button(
            ui,
            "drill",
            icons::DRILL,
            Some("Drill"),
            "Cut the latest body with a cylinder",
            false,
            true,
        )
        .clicked()
        {
            PaletteAction::CutWithCylinder.run(app);
        }

        ui.separator();

        // -- View -----------------------------------------------------------
        let ortho = app.camera.orthographic;
        if tool_button(
            ui,
            "projection",
            icons::AXIS_3D,
            None,
            if ortho {
                "Orthographic projection\nP"
            } else {
                "Perspective projection\nP"
            },
            false,
            true,
        )
        .clicked()
        {
            PaletteAction::ToggleProjection.run(app);
        }
        if tool_button(
            ui,
            "fit",
            icons::FIT,
            None,
            "Fit view to the model\nF",
            false,
            true,
        )
        .clicked()
        {
            PaletteAction::FitView.run(app);
        }
        if tool_button(
            ui,
            "grid",
            icons::GRID,
            None,
            "Ground grid\nG",
            app.render_options.show_grid,
            true,
        )
        .clicked()
        {
            PaletteAction::ToggleGrid.run(app);
        }
        if tool_button(
            ui,
            "edges",
            icons::EDGES,
            None,
            "Feature edges\nE",
            app.render_options.show_edges,
            true,
        )
        .clicked()
        {
            PaletteAction::ToggleEdges.run(app);
        }

        // Display modes (W-05) + section view (W-02) + measure (W-08).
        let mode = app.render_options.display_mode;
        let mode_label = match mode {
            forge_render::DisplayMode::Shaded => icons::icon_label(icons::SHADED, "Shaded"),
            forge_render::DisplayMode::Wireframe => {
                icons::icon_label(icons::WIREFRAME, "Wireframe")
            }
            forge_render::DisplayMode::XRay => icons::icon_label(icons::XRAY, "X-Ray"),
        };
        egui::ComboBox::from_id_salt("display-mode")
            .selected_text(mode_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut app.render_options.display_mode,
                    forge_render::DisplayMode::Shaded,
                    icons::icon_label(icons::SHADED, "Shaded"),
                );
                ui.selectable_value(
                    &mut app.render_options.display_mode,
                    forge_render::DisplayMode::Wireframe,
                    icons::icon_label(icons::WIREFRAME, "Wireframe (hidden line)"),
                );
                ui.selectable_value(
                    &mut app.render_options.display_mode,
                    forge_render::DisplayMode::XRay,
                    icons::icon_label(icons::XRAY, "X-Ray (translucent)"),
                );
            });
        if tool_button(
            ui,
            "section",
            icons::SECTION,
            None,
            "Section view: cut the model with a plane",
            app.render_options.section.is_some(),
            true,
        )
        .clicked()
        {
            PaletteAction::ToggleSection.run(app);
        }
        if tool_button(
            ui,
            "measure",
            icons::MEASURE,
            None,
            "Measure: click two surface points for distance + angle",
            app.measure_mode,
            true,
        )
        .clicked()
        {
            PaletteAction::ToggleMeasure.run(app);
        }
        if tool_button(
            ui,
            "cam",
            icons::DRILL,
            None,
            "CAM workspace: toolpaths, G-code\nStrategies computed from the model",
            app.cam.panel_open,
            true,
        )
        .clicked()
        {
            app.cam.panel_open = !app.cam.panel_open;
        }

        ui.separator();

        // -- Gizmo + selection granularity (W-01 / W-04) ---------------------
        if tool_button(
            ui,
            "gizmo-move",
            icons::MOVE_3D,
            Some("Move"),
            "Gizmo translate — select a body and drag an axis\nT",
            app.gizmo_mode == crate::gizmo::GizmoMode::Translate,
            true,
        )
        .clicked()
        {
            PaletteAction::GizmoTranslate.run(app);
        }
        if tool_button(
            ui,
            "gizmo-rotate",
            icons::ROTATE_3D,
            Some("Rotate"),
            "Gizmo rotate — select a body and drag a ring\nR",
            app.gizmo_mode == crate::gizmo::GizmoMode::Rotate,
            true,
        )
        .clicked()
        {
            PaletteAction::GizmoRotate.run(app);
        }

        {
            let mut mode = app.pick_mode;
            let label = match mode {
                crate::picking::PickMode::Bodies => icons::icon_label(icons::BODIES, "Bodies"),
                crate::picking::PickMode::Faces => {
                    icons::icon_label(icons::SKETCH_ON_FACE, "Faces")
                }
                crate::picking::PickMode::Edges => icons::icon_label(icons::EDGES, "Edges"),
                crate::picking::PickMode::Vertices => icons::icon_label(icons::TARGET, "Vertices"),
            };
            egui::ComboBox::from_id_salt("pick-mode")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut mode,
                        crate::picking::PickMode::Bodies,
                        icons::icon_label(icons::BODIES, "Bodies"),
                    );
                    ui.selectable_value(
                        &mut mode,
                        crate::picking::PickMode::Faces,
                        icons::icon_label(icons::SKETCH_ON_FACE, "Faces"),
                    );
                    ui.selectable_value(
                        &mut mode,
                        crate::picking::PickMode::Edges,
                        icons::icon_label(icons::EDGES, "Edges (sharp chains)"),
                    );
                    ui.selectable_value(
                        &mut mode,
                        crate::picking::PickMode::Vertices,
                        icons::icon_label(icons::TARGET, "Vertices"),
                    );
                });
            if mode != app.pick_mode {
                app.pick_mode = mode;
                app.selection.clear();
                app.set_status(format!("Picking {}", mode.label().to_lowercase()));
            }
        }
        if app.pick_mode == crate::picking::PickMode::Faces
            && tool_button(
                ui,
                "sketch-on-face",
                icons::SKETCH_ON_FACE,
                Some("Sketch on face"),
                "Create a sketch on the selected planar face",
                false,
                true,
            )
            .clicked()
        {
            PaletteAction::SketchOnSelectedFace.run(app);
        }

        // Section controls (only while the section view is on).
        if let Some(section) = &mut app.render_options.section {
            let mut axis = if section.normal[0] != 0.0 {
                0
            } else if section.normal[1] != 0.0 {
                1
            } else {
                2
            };
            egui::ComboBox::from_id_salt("section-axis")
                .selected_text(icons::icon_label(icons::SECTION, ["X", "Y", "Z"][axis]))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut axis, 0, "cut normal +X");
                    ui.selectable_value(&mut axis, 1, "cut normal +Y");
                    ui.selectable_value(&mut axis, 2, "cut normal +Z");
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
            if tool_button(
                ui,
                "section-flip",
                icons::MIRROR,
                None,
                "Flip the section side",
                false,
                true,
            )
            .clicked()
            {
                section.normal = [-normal[0], -normal[1], -normal[2]];
                section.offset = -section.offset;
            }
            section.offset = offset;
        }

        ui.separator();

        // -- Files ------------------------------------------------------------
        if tool_button(
            ui,
            "save",
            icons::SAVE,
            None,
            "Save .forgecad\nCtrl+S",
            false,
            true,
        )
        .clicked()
        {
            PaletteAction::SaveNative.run(app);
        }
        let export_menu = ui.menu_button(chevron_label(icons::EXPORT, "Export"), |ui| {
            let r = crate::bridge::button(ui, "STL (binary)");
            if r.clicked() {
                app.export_mesh(forge_io::ExportFormat::Stl);
            }
            let r = crate::bridge::button(ui, "OBJ");
            if r.clicked() {
                app.export_mesh(forge_io::ExportFormat::Obj);
            }
            let r = crate::bridge::button(ui, "glTF 2.0");
            if r.clicked() {
                app.export_mesh(forge_io::ExportFormat::Gltf);
            }
            let r = crate::bridge::button(ui, "3MF (3D print package)");
            if r.clicked() {
                app.export_mesh(forge_io::ExportFormat::ThreeMf);
            }
        });
        // E2E bridge: the Export menu trigger.
        crate::bridge::record(
            "menu:Export",
            "Export",
            "menu",
            export_menu.response.rect,
            true,
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if tool_button(
                ui,
                "palette",
                icons::COMMAND,
                None,
                "Command palette\nCtrl+Shift+P",
                app.palette_open,
                true,
            )
            .clicked()
            {
                app.palette_open = !app.palette_open;
                app.palette_query.clear();
            }
        });
    });
}

/// Icon + label + muted chevron-down job for menu buttons ("Solid ▾").
fn chevron_label(icon: char, label: &str) -> egui::text::LayoutJob {
    use egui::{Color32, FontFamily, FontId, TextFormat};
    let mut job = crate::icons::icon_label(icon, label);
    job.append(
        "  ",
        0.0,
        TextFormat {
            font_id: FontId::new(13.0, FontFamily::Proportional),
            color: Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    job.append(
        &crate::icons::CHEVRON_DOWN.to_string(),
        0.0,
        TextFormat {
            font_id: FontId::new(13.0, crate::icons::family()),
            color: Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    job
}

/// Lucide glyph for a feature kind (tree rows + inspector title).
fn feature_icon(feature: &Feature) -> char {
    use crate::icons;
    match feature {
        Feature::Sketch(_) => icons::SKETCH,
        Feature::Extrude(_) => icons::EXTRUDE,
        Feature::Revolve(_) => icons::REVOLVE,
        Feature::Loft(_) => icons::LOFT,
        Feature::Sweep(_) => icons::SWEEP,
        Feature::Primitive(p) => match p.kind {
            forge_model::PrimitiveKind::Box => icons::BOX,
            forge_model::PrimitiveKind::Sphere => icons::SPHERE,
            forge_model::PrimitiveKind::Cylinder => icons::CYLINDER,
            forge_model::PrimitiveKind::Cone => icons::CONE,
            forge_model::PrimitiveKind::Torus => icons::TORUS,
        },
        Feature::Boolean(_) => icons::COMBINE,
        Feature::TransformBody { .. } => icons::MOVE_3D,
        Feature::LinearPattern(_) => icons::LINEAR_PATTERN,
        Feature::CircularPattern(_) => icons::CIRCULAR_PATTERN,
        Feature::Mirror(_) => icons::MIRROR,
        Feature::Hole(_) => icons::DRILL,
        Feature::Datum(_) => icons::DATUM,
        Feature::ImportedMesh(_) => icons::IMPORTED_MESH,
    }
}

/// Left panel: the parametric feature tree (FR-SM-04, FR-UI-02) — icon
/// rows with diagnostics badges, suppress toggle and a context menu
/// (both the ⋮ button and right-click on the row).
pub fn tree_panel(ui: &mut egui::Ui, app: &mut ForgeApp) {
    use crate::icons;
    use crate::theme;

    crate::theme::section_header(ui, icons::FEATURES, "Features");
    // NOTE: no ScrollArea here — the whole left panel (tree + params)
    // shares one in `app.rs`. The tree's own full-height ScrollArea
    // used to push the parameter panel off-screen on default windows
    // (the E2E harness found the Add-parameter button at y=972 on a
    // 900 px screen — P-01 was unreachable without resizing).
    ui.vertical(|ui| {
        let order: Vec<forge_core::FeatureId> = app.doc.tree.order().to_vec();
        if order.is_empty() {
            ui.add_space(10.0);
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
            let icon = feature_icon(&node.feature);

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

            // Row: [suppressed-eye] [feature icon] label .... [badges] [⋮]
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(4, 1))
                .corner_radius(5)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mut job = egui::text::LayoutJob::default();
                        let icon_color = if node.suppressed {
                            theme::MUTED
                        } else if has_error {
                            theme::ERR
                        } else {
                            theme::ACCENT
                        };
                        if node.suppressed {
                            job.append(
                                &icons::EYE_OFF.to_string(),
                                0.0,
                                egui::TextFormat {
                                    font_id: egui::FontId::new(12.0, icons::family()),
                                    color: theme::MUTED,
                                    ..Default::default()
                                },
                            );
                            job.append(
                                " ",
                                0.0,
                                egui::TextFormat::simple(
                                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                                    egui::Color32::PLACEHOLDER,
                                ),
                            );
                        }
                        job.append(
                            &icon.to_string(),
                            0.0,
                            egui::TextFormat {
                                font_id: egui::FontId::new(14.0, icons::family()),
                                color: icon_color,
                                ..Default::default()
                            },
                        );
                        job.append(
                            "  ",
                            0.0,
                            egui::TextFormat::simple(
                                egui::FontId::new(12.5, egui::FontFamily::Proportional),
                                egui::Color32::PLACEHOLDER,
                            ),
                        );
                        job.append(
                            &label,
                            0.0,
                            egui::TextFormat {
                                font_id: egui::FontId::new(
                                    13.0,
                                    if selected {
                                        egui::FontFamily::Name(theme::SEMIBOLD.into())
                                    } else {
                                        egui::FontFamily::Proportional
                                    },
                                ),
                                color: if node.suppressed {
                                    theme::MUTED
                                } else {
                                    egui::Color32::PLACEHOLDER
                                },
                                ..Default::default()
                            },
                        );
                        if has_error {
                            job.append(
                                "  ",
                                0.0,
                                egui::TextFormat::simple(
                                    egui::FontId::new(13.0, egui::FontFamily::Proportional),
                                    egui::Color32::PLACEHOLDER,
                                ),
                            );
                            job.append(
                                &icons::ERROR.to_string(),
                                0.0,
                                egui::TextFormat {
                                    font_id: egui::FontId::new(13.0, icons::family()),
                                    color: theme::ERR,
                                    ..Default::default()
                                },
                            );
                        }

                        let response = ui.selectable_label(selected, job);
                        crate::bridge::record(
                            format!("tree:{label}"),
                            label.clone(),
                            "selectable",
                            response.rect,
                            response.enabled(),
                        );
                        if response.clicked() {
                            app.selection.select(forge_model::SelectionItem::Body(
                                forge_core::BodyId::new(id.raw()),
                            ));
                        }
                        // Right-click context menu (FR-UI-02).
                        response.context_menu(|ui| {
                            feature_row_menu(ui, app, id);
                        });

                        // Right side: DOF badge + suppress eye + ⋮ menu.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.menu_button(icons::colored(icons::MENU, 13.0, theme::MUTED), |ui| {
                                feature_row_menu(ui, app, id);
                            });
                            let eye = ui
                                .selectable_label(
                                    node.suppressed,
                                    icons::sized(
                                        if node.suppressed {
                                            icons::EYE_OFF
                                        } else {
                                            icons::EYE
                                        },
                                        13.0,
                                    ),
                                )
                                .on_hover_text(if node.suppressed {
                                    "Suppressed — click to enable"
                                } else {
                                    "Suppress feature"
                                });
                            crate::bridge::record(
                                format!("tree:{label}:suppress"),
                                "Suppress",
                                "selectable",
                                eye.rect,
                                eye.enabled(),
                            );
                            if eye.clicked() {
                                app.selection.select(forge_model::SelectionItem::Body(
                                    forge_core::BodyId::new(id.raw()),
                                ));
                                PaletteAction::SuppressSelected.run(app);
                            }
                            if let Some(badge) = &dof_badge {
                                let (color, glyph) = match badge.as_str() {
                                    "fully constrained" => (theme::OK, icons::CHECK_CIRCLE),
                                    s if s.starts_with("over") => (theme::ERR, icons::ERROR),
                                    _ => (theme::MUTED, icons::ELLIPSIS),
                                };
                                ui.horizontal(|ui| {
                                    ui.label(icons::colored(glyph, 10.5, color));
                                    ui.label(
                                        egui::RichText::new(badge.clone()).size(10.5).color(color),
                                    );
                                })
                                .response
                                .on_hover_text("Sketch solver diagnostics (S-05)");
                            }
                        });
                    });
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
                            .color(theme::ERR),
                    );
                }
            }
            ui.add_space(2.0);
        }
    });
}

/// Context-menu items for a feature row (shared by ⋮ and right-click).
fn feature_row_menu(ui: &mut egui::Ui, app: &mut ForgeApp, id: forge_core::FeatureId) {
    use crate::icons;
    let edit = ui.button(icons::icon_label(icons::EYE, "Edit in inspector"));
    crate::bridge::record(
        "rowmenu:edit",
        "Edit in inspector",
        "button",
        edit.rect,
        edit.enabled(),
    );
    if edit.clicked() {
        app.selection
            .select(forge_model::SelectionItem::Body(forge_core::BodyId::new(
                id.raw(),
            )));
        ui.close();
    }
    let suppress = ui.button(icons::icon_label(icons::EYE_OFF, "Suppress / unsuppress"));
    crate::bridge::record(
        "rowmenu:suppress",
        "Suppress / unsuppress",
        "button",
        suppress.rect,
        suppress.enabled(),
    );
    if suppress.clicked() {
        app.selection
            .select(forge_model::SelectionItem::Body(forge_core::BodyId::new(
                id.raw(),
            )));
        app.toggle_suppress_selected();
        ui.close();
    }
    let delete = ui.button(icons::icon_label(icons::DELETE, "Delete"));
    crate::bridge::record(
        "rowmenu:delete",
        "Delete",
        "button",
        delete.rect,
        delete.enabled(),
    );
    if delete.clicked() {
        app.selection
            .select(forge_model::SelectionItem::Body(forge_core::BodyId::new(
                id.raw(),
            )));
        app.delete_selected();
        ui.close();
    }
}

/// Right panel: parameter inspector for the selected feature (FR-UI-03
/// live preview editing).
pub fn inspector(ui: &mut egui::Ui, app: &mut ForgeApp) {
    use crate::icons;
    use crate::theme;

    let Some(id) = app.selection.primary_feature() else {
        ui.label(egui::RichText::new("Select a feature to edit").weak());
        return;
    };
    let Some(feature) = app.doc.feature(id).cloned() else {
        return;
    };

    // Title: the feature-kind icon + label (semibold).
    ui.horizontal(|ui| {
        ui.label(icons::colored(feature_icon(&feature), 15.0, theme::ACCENT));
        ui.label(theme::semibold(feature.label()).size(14.0));
    });
    ui.add_space(4.0);
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
        if crate::bridge::button(ui, "Bind").clicked() {
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
            if crate::bridge::button(ui, "Apply").clicked() {
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
            if crate::bridge::button(ui, "Solve constraints").clicked() {
                match s.solve() {
                    Ok(report) => {
                        app.last_sketch_report = Some(report);
                        edit(app, Feature::Sketch(s));
                    }
                    Err(e) => app.set_status(format!("{e}")),
                }
            }

            // S-08: mirror tool — pick a symmetry line, mirror every other
            // entity across it with rank-complete symmetric constraints.
            {
                use forge_sketch::SketchEntity;
                let lines: Vec<(forge_core::EntityId, String)> = sketch
                    .entities
                    .values()
                    .filter_map(|e| match e {
                        SketchEntity::Line { id, start, end, .. } => Some((
                            *id,
                            format!(
                                "line {id}  ({:.1},{:.1})–({:.1},{:.1})",
                                start.x, start.y, end.x, end.y
                            ),
                        )),
                        _ => None,
                    })
                    .collect();
                if lines.is_empty() {
                    ui.label(egui::RichText::new("Mirror: add a line to mirror across").weak());
                } else {
                    ui.separator();
                    ui.strong("Mirror (S-08)");
                    // Default to the remembered pick, else the first line.
                    if app
                        .mirror_line_pick
                        .map(|m| !lines.iter().any(|(id, _)| *id == m))
                        .unwrap_or(true)
                    {
                        app.mirror_line_pick = Some(lines[0].0);
                    }
                    let mut selected = app.mirror_line_pick.unwrap_or(lines[0].0).raw() as usize;
                    egui::ComboBox::from_id_salt("mirror-line")
                        .selected_text(
                            lines
                                .iter()
                                .find(|(id, _)| id.raw() as usize == selected)
                                .map(|(_, l)| l.as_str())
                                .unwrap_or("?"),
                        )
                        .show_ui(ui, |ui| {
                            for (lid, label) in &lines {
                                ui.selectable_value(&mut selected, lid.raw() as usize, label);
                            }
                        });
                    let mirror = forge_core::EntityId::new(selected as u64);
                    app.mirror_line_pick = Some(mirror);
                    let others: Vec<forge_core::EntityId> = sketch
                        .entities
                        .keys()
                        .filter(|&&e| e != mirror)
                        .copied()
                        .collect();
                    let mirror_btn =
                        crate::bridge::button(ui, format!("Mirror {} entities", others.len()))
                            .on_hover_text(
                                "Creates mirrored copies tied to the originals by symmetric \
                         constraints — drag an original and the copy follows",
                            );
                    if mirror_btn.clicked() {
                        let mut target = sketch.clone();
                        match target.mirror_entities(&others, mirror) {
                            Ok(created) => {
                                let report = target.solve().ok();
                                if let Some(r) = &report {
                                    app.last_sketch_report = Some(r.clone());
                                }
                                edit(app, Feature::Sketch(target));
                                app.set_status(format!(
                                    "Mirrored {} entities{}",
                                    created.len(),
                                    report
                                        .map(|r| if r.is_solved() {
                                            String::new()
                                        } else {
                                            format!(" (solver: {})", r.status)
                                        })
                                        .unwrap_or_default()
                                ));
                            }
                            Err(e) => app.set_status(format!("{e}")),
                        }
                    }
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
            crate::bridge::checkbox(ui, &mut symmetric, "Symmetric about the seed");
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
            crate::bridge::checkbox(ui, &mut newp.drill_point, "Drill point (conical bottom)");
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

/// Bottom status bar: status message on the left, icon stat chips on
/// the right (PR-05 live performance stats).
pub fn status_bar(ui: &mut egui::Ui, app: &ForgeApp) {
    use crate::icons;
    use crate::theme;

    ui.horizontal(|ui| {
        ui.set_min_height(26.0);

        // Left: the status line (single line, truncated).
        ui.add(
            egui::Label::new(
                egui::RichText::new(app.status.as_str())
                    .size(12.0)
                    .color(theme::MUTED),
            )
            .truncate(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::chip(
                ui,
                icons::ZAP,
                "mm".into(),
                "Millimeters — ForgeCAD's native unit",
            );

            let (bodies, tris) = app
                .last_evaluation
                .as_ref()
                .map(|ev| (ev.bodies.len(), ev.total_triangles))
                .unwrap_or((0, 0));
            theme::chip(
                ui,
                icons::FEATURES,
                format!("{}", app.doc.tree.len()),
                "Features in the tree",
            );
            let eval_state = if app.eval_pending {
                "evaluating…"
            } else {
                "ready"
            };
            let (state_icon, _state_color) = if app.eval_pending {
                (icons::ACTIVITY, theme::ACCENT)
            } else {
                (icons::CHECK_CIRCLE, theme::MUTED)
            };
            // PR-05: live performance stats — frame pacing and the wall
            // duration of the last evaluation (geometry work happens on
            // the background worker, so eval > frame time is normal).
            let (fps, frame_ms) = if app.frame_times.is_empty() {
                (0.0, 0.0)
            } else {
                let n = app.frame_times.len() as f32;
                let avg = app.frame_times.iter().sum::<f32>() / n;
                (1.0 / avg.max(1e-6), avg * 1000.0)
            };
            let eval_ms = app
                .last_eval_duration
                .map(|d| format!("{:.0} ms", d.as_secs_f64() * 1e3))
                .unwrap_or_else(|| "—".into());
            theme::chip(
                ui,
                icons::TIMER,
                format!("eval {eval_ms}"),
                "Wall time of the last full evaluation",
            );
            theme::chip(
                ui,
                icons::FPS,
                format!("{fps:.0} fps · {frame_ms:.1} ms"),
                "Rolling frame rate and frame time",
            );
            theme::chip(
                ui,
                icons::TRIANGLE,
                format!("{tris} tris"),
                "Triangles currently rendered",
            );
            theme::chip(ui, icons::BODIES, format!("{bodies}"), "Solid bodies");
            theme::chip(
                ui,
                state_icon,
                eval_state.into(),
                "Background evaluation worker",
            );

            // S-05: live sketch diagnostics for the selected sketch.
            if let Some(sketch_id) = app.selection.primary_feature() {
                if let Some(report) = app
                    .last_evaluation
                    .as_ref()
                    .and_then(|ev| ev.sketch_reports.get(&sketch_id))
                {
                    let (text, color, glyph) = if report.dof_balance > 0 {
                        (
                            format!("{} DOF", report.dof_balance),
                            theme::MUTED,
                            icons::ELLIPSIS,
                        )
                    } else if report.dof_balance < 0 {
                        (
                            format!("over-constrained {}", -report.dof_balance),
                            theme::ERR,
                            icons::ERROR,
                        )
                    } else {
                        (
                            "fully constrained".to_string(),
                            theme::OK,
                            icons::CHECK_CIRCLE,
                        )
                    };
                    // A colored diagnostics chip before the neutral ones.
                    let frame = egui::Frame::new()
                        .fill(theme::SURFACE)
                        .corner_radius(5)
                        .inner_margin(egui::Margin::symmetric(7, 2));
                    frame
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(icons::colored(glyph, 11.5, color));
                                ui.label(egui::RichText::new(text).size(11.5).color(color));
                            });
                        })
                        .response
                        .on_hover_text("Live sketch solver report (S-05)");
                }
            }
        });
    });
}

/// P-01: the user parameter table (name, expression or value, unit) with
/// undoable edits and per-row deletion. Angle parameters display degrees
/// (stored as radians).
pub fn params_panel(ui: &mut egui::Ui, app: &mut ForgeApp) {
    crate::theme::section_header(ui, crate::icons::PARAMS, "Parameters");
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
                    if crate::bridge::button(ui, "Apply").clicked() {
                        apply_clicked = true;
                    }
                    if crate::bridge::button(ui, "\u{2715}").clicked() {
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
    if crate::bridge::button(ui, "\u{ff0b} Add parameter").clicked() {
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

/// The command palette overlay (FR-UI-01): searchable list of every
/// command with icons, ↑/↓/↵ keyboard navigation and hover sync.
pub fn palette_overlay(ctx: &egui::Context, app: &mut ForgeApp) {
    use crate::icons;
    use crate::theme;

    if !app.palette_open {
        return;
    }

    // Pull navigation keys out of this frame's event stream BEFORE the
    // search field sees them (a single-line query never needs ↑/↓/↵,
    // so they belong to the list).
    let mut up = false;
    let mut down = false;
    let mut enter = false;
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::Key {
                key: egui::Key::ArrowDown,
                pressed: true,
                ..
            } => {
                down = true;
                false
            }
            egui::Event::Key {
                key: egui::Key::ArrowUp,
                pressed: true,
                ..
            } => {
                up = true;
                false
            }
            egui::Event::Key {
                key: egui::Key::Enter,
                pressed: true,
                ..
            } => {
                enter = true;
                false
            }
            _ => true,
        });
    });

    let mut run_action: Option<PaletteAction> = None;

    egui::Window::new("Command Palette")
        .anchor(egui::Align2::CENTER_TOP, [0.0, 56.0])
        .fixed_size([540.0, 400.0])
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .show(ctx, |ui| {
            // -- Search field -------------------------------------------------
            ui.horizontal(|ui| {
                ui.label(icons::colored(icons::SEARCH, 15.0, theme::MUTED));
                let edit = egui::TextEdit::singleline(&mut app.palette_query)
                    .hint_text("Search commands…")
                    .margin(egui::Margin::symmetric(8, 6))
                    .desired_width(ui.available_width());
                let response = ui.add(edit);
                // Claim keyboard focus when nothing else has it.
                if !ui.ctx().egui_wants_keyboard_input() {
                    response.request_focus();
                }
            });

            ui.add_space(6.0);
            ui.separator();

            // -- Matching rows ---------------------------------------------
            let query = app.palette_query.clone();
            let mut scored: Vec<(usize, char, &'static str, PaletteAction)> = entries()
                .into_iter()
                .filter_map(|e| {
                    fuzzy_score(&query, e.label, e.keywords)
                        .map(|score| (score, e.icon, e.label, e.action))
                })
                .collect();
            scored.sort_by_key(|(score, _, _, _)| *score);

            let visible = 14usize;
            app.palette_cursor = app.palette_cursor.min(scored.len().saturating_sub(1));
            if up {
                app.palette_cursor = app.palette_cursor.saturating_sub(1);
            }
            if down {
                app.palette_cursor = (app.palette_cursor + 1).min(scored.len().saturating_sub(1));
            }

            let mut hovered_row: Option<usize> = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if scored.is_empty() {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("No matching commands").weak());
                    }
                    for (row, (_, icon, label, action)) in scored.iter().enumerate().take(visible) {
                        let is_cursor = row == app.palette_cursor;
                        let response =
                            ui.selectable_label(is_cursor, icons::icon_label(*icon, label));
                        crate::bridge::record(
                            format!("palette:{label}"),
                            *label,
                            "selectable",
                            response.rect,
                            response.enabled(),
                        );
                        if response.clicked() {
                            run_action = Some(*action);
                        }
                        if response.hovered() {
                            hovered_row = Some(row);
                        }
                    }
                });
            if let Some(h) = hovered_row {
                app.palette_cursor = h;
            }

            // -- Footer hint -------------------------------------------------
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(icons::colored(icons::COMMAND, 11.0, theme::MUTED));
                ui.label(
                    egui::RichText::new("↑ ↓ navigate    ↵ run    esc close")
                        .size(11.0)
                        .color(theme::MUTED),
                );
            });

            if enter {
                if let Some((_, _, _, action)) = scored.get(app.palette_cursor) {
                    run_action = Some(*action);
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.palette_open = false;
            }
        });

    if let Some(action) = run_action {
        app.palette_open = false;
        app.palette_cursor = 0;
        action.run(app);
    }
}

/// CAM workspace dock (C-03): operations, tools, compute, G-code export.
pub fn cam_panel(ui: &mut egui::Ui, app: &mut ForgeApp) {
    use crate::cam::CamStrategyKind;
    use crate::icons;
    use crate::theme;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());

            // -- Header ---------------------------------------------------
            ui.horizontal(|ui| {
                ui.label(icons::colored(icons::DRILL, 15.0, theme::ACCENT));
                ui.label(theme::semibold("CAM").size(13.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::bridge::button(ui, "Close").clicked() {
                        app.cam.panel_open = false;
                    }
                    if crate::bridge::button(ui, "Export G-code")
                        .on_hover_text("Write the .nc program for all computed operations")
                        .clicked()
                    {
                        app.cam_export_gcode();
                    }
                    if crate::bridge::button(ui, "Compute")
                        .on_hover_text("Run every enabled operation against the current model")
                        .clicked()
                    {
                        app.cam_compute();
                    }
                });
            });
            ui.add_space(4.0);
            ui.separator();

            ui.add_space(4.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Add operation:");
                for kind in CamStrategyKind::ALL {
                    // "+" prefix keeps the label distinct from toolbar
                    // buttons with the same word (e.g. "Drill").
                    if crate::bridge::button(ui, format!("+ {}", kind.label()))
                        .on_hover_text(match kind {
                            CamStrategyKind::Rough => {
                                "Raster roughing: clear stock above the floor (stepdown/stepover)"
                            }
                            CamStrategyKind::Face => "Face the stock top down to a level",
                            CamStrategyKind::Waterline => {
                                "Waterline finish: constant-Z wall & slope passes"
                            }
                            CamStrategyKind::Drill => {
                                "Peck-drill the holes recognized from Hole features"
                            }
                        })
                        .clicked()
                    {
                        let id = app.cam.add_op(kind);
                        let _ = id;
                        app.set_status(format!("CAM: added {} operation", kind.label()));
                    }
                }
            });
            ui.add_space(6.0);

            if app.cam.ops.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "No operations. Add Rough, Face, Waterline or Drill; pick a tool; press Compute.",
                    )
                    .weak()
                    .size(11.5),
                );
                return;
            }

            // -- Operation list -------------------------------------------
            let mut remove: Option<u32> = None;
            let mut eye_toggle: Option<usize> = None;
            let mut selected = app.cam_selected_op;
            for (i, op) in app.cam.ops.iter().enumerate() {
                let is_sel = app.cam_selected_op == i;
                let result_info = op.result.as_ref().map(|r| {
                    format!(
                        "cut {:.0}mm · {:.1}min · {} moves",
                        r.cut_length(),
                        r.time_minutes(),
                        r.moves.len()
                    )
                });
                egui::Frame::default()
                    .fill(if is_sel {
                        egui::Color32::from_rgba_premultiplied(30, 34, 40, 255)
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let name = format!("{} #{}", op.kind.label(), op.id + 1);
                            if ui
                                .selectable_label(is_sel, icons::icon_label(icons::ACTIVITY, &name))
                                .clicked()
                            {
                                selected = i;
                            }
                            let eye = ui
                                .selectable_label(
                                    !op.enabled,
                                    icons::sized(
                                        if op.enabled { icons::EYE } else { icons::EYE_OFF },
                                        13.0,
                                    ),
                                )
                                .on_hover_text("Suppress / enable this operation");
                            crate::bridge::record(
                                format!("cam:op{}:suppress", op.id),
                                "Suppress operation",
                                "selectable",
                                eye.rect,
                                eye.enabled(),
                            );
                            if eye.clicked() {
                                eye_toggle = Some(i);
                            }
                            let del = ui
                                .selectable_label(false, icons::sized(icons::DELETE, 13.0))
                                .on_hover_text("Delete operation");
                            crate::bridge::record(
                                format!("cam:op{}:delete", op.id),
                                "Delete operation",
                                "selectable",
                                del.rect,
                                del.enabled(),
                            );
                            if del.clicked() {
                                remove = Some(op.id);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if let Some(info) = result_info {
                                        ui.label(egui::RichText::new(info).size(10.5).weak());
                                    } else if !op.enabled {
                                        ui.label(egui::RichText::new("suppressed").size(10.5).weak());
                                    } else {
                                        ui.label(egui::RichText::new("not computed").size(10.5).weak());
                                    }
                                },
                            );
                        });
                        if !op.warnings.is_empty() {
                            for w in &op.warnings {
                                ui.label(
                                    egui::RichText::new(format!("{} {w}", icons::ALERT))
                                        .size(10.5)
                                        .color(egui::Color32::from_rgb(220, 170, 80)),
                                );
                            }
                        }
                    });
            }
            app.cam_selected_op = selected.min(app.cam.ops.len().saturating_sub(1));
            if let Some(i) = eye_toggle {
                if let Some(o) = app.cam.ops.get_mut(i) {
                    o.enabled = !o.enabled;
                }
            }
            if let Some(id) = remove {
                app.cam.remove_op(id);
                if app.cam_selected_op >= app.cam.ops.len() {
                    app.cam_selected_op = app.cam.ops.len().saturating_sub(1);
                }
            }
            ui.add_space(6.0);
            ui.separator();

            ui.add_space(4.0);
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(theme::semibold("Display").size(12.0));
                crate::bridge::checkbox(ui, &mut app.cam.show_stock, "stock");
                crate::bridge::checkbox(ui, &mut app.cam.show_toolpaths, "paths");
                crate::bridge::checkbox(ui, &mut app.cam.show_rapids, "rapids");
            });
            egui::Grid::new("cam-stock-grid")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Stock XY margin (mm):");
                    let mut v = app.cam.stock_margin;
                    ui.add(
                        egui::DragValue::new(&mut v)
                            .speed(0.5)
                            .range(0.0..=50.0)
                    );
                    app.cam.stock_margin = v;
                    ui.end_row();
                    ui.label("Stock top margin (mm):");
                    let mut v = app.cam.stock_top_margin;
                    ui.add(
                        egui::DragValue::new(&mut v)
                            .speed(0.5)
                            .range(0.0..=50.0)
                    );
                    app.cam.stock_top_margin = v;
                    ui.end_row();
                });
            app.cam.build_overlays(&mut app.scene);

            // -- Selected operation editor ---------------------------------
            let idx = app.cam_selected_op.min(app.cam.ops.len() - 1);
            let tool_names: Vec<String> = app
                .cam
                .library
                .tools
                .iter()
                .map(|t| format!("T{} {} ({:.1}mm)", t.id + 1, t.name, t.diameter))
                .collect();
            if let Some(op) = app.cam.ops.get_mut(idx) {
                ui.label(theme::semibold("Operation").size(12.0));
                ui.add_space(2.0);
                egui::Grid::new("cam-op-grid")
                    .num_columns(2)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Tool:");
                        let current = app
                            .cam
                            .library
                            .get(op.tool_id)
                            .map(|t| t.id as usize)
                            .unwrap_or(0);
                        let mut pick = current;
                        let combo = egui::ComboBox::from_id_salt("cam-op-tool")
                            .selected_text(
                                tool_names.get(pick).cloned().unwrap_or_else(|| "?".into()),
                            )
                            .show_ui(ui, |ui| {
                                for (k, name) in tool_names.iter().enumerate() {
                                    ui.selectable_value(&mut pick, k, name.clone());
                                }
                            });
                        let _ = combo.response;
                        crate::bridge::record(
                            "cam:op:tool",
                            "Tool",
                            "combo",
                            combo.response.rect,
                            true,
                        );
                        if pick != current {
                            if let Some(t) = app.cam.library.tools.get(pick) {
                                op.tool_id = t.id;
                            }
                        }
                        ui.end_row();

                        let show_stepdown = op.kind != CamStrategyKind::Drill;
                        let show_stepover =
                            matches!(op.kind, CamStrategyKind::Rough | CamStrategyKind::Face);
                        let show_leave = op.kind != CamStrategyKind::Drill;
                        let show_floor = matches!(
                            op.kind,
                            CamStrategyKind::Rough | CamStrategyKind::Face
                        );

                        if show_stepdown {
                            ui.label(if op.kind == CamStrategyKind::Waterline {
                                "Stepdown (mm):"
                            } else {
                                "Depth of cut (mm):"
                            });
                            let mut v = op.stepdown;
                            ui.add(
                                egui::DragValue::new(&mut v)
                                    .speed(0.1)
                                    .range(0.1..=20.0),
                            );
                            op.stepdown = v;
                            ui.end_row();
                        }
                        if show_stepover {
                            ui.label("Stepover (mm):");
                            let mut v = op.stepover;
                            ui.add(
                                egui::DragValue::new(&mut v)
                                    .speed(0.1)
                                    .range(0.2..=20.0),
                            );
                            op.stepover = v;
                            ui.end_row();
                        }
                        if show_leave {
                            ui.label("Stock to leave (mm):");
                            let mut v = op.leave;
                            ui.add(
                                egui::DragValue::new(&mut v)
                                    .speed(0.02)
                                    .range(0.0..=2.0),
                            );
                            op.leave = v;
                            ui.end_row();
                        }
                        if show_floor {
                            ui.label("Floor Z:");
                            let mut on = op.floor_z.is_some();
                            crate::bridge::checkbox(ui, &mut on, "");
                            if on {
                                let mut v = op.floor_z.unwrap_or_else(|| {
                                    app.last_evaluation
                                        .as_ref()
                                        .map(|ev| {
                                            ev.bodies
                                                .iter()
                                                .map(|b| b.mesh.bbox().min.z)
                                                .fold(f64::INFINITY, f64::min)
                                        })
                                        .unwrap_or(0.0)
                                        .max(-100.0)
                                });
                                ui.add(
                                    egui::DragValue::new(&mut v)
                                        .speed(0.1)
                                        .range(-100.0..=100.0),
                                );
                                op.floor_z = Some(v);
                            } else {
                                op.floor_z = None;
                                ui.label("(stock bottom)");
                            }
                            ui.end_row();
                        }
                        if op.kind == CamStrategyKind::Drill {
                            ui.label("Peck depth:");
                            let mut on = op.peck.is_some();
                            crate::bridge::checkbox(ui, &mut on, "");
                            if on {
                                let mut v = op.peck.unwrap_or(1.0);
                                ui.add(
                                    egui::DragValue::new(&mut v)
                                        .speed(0.1)
                                        .range(0.2..=10.0),
                                );
                                op.peck = Some(v);
                            } else {
                                op.peck = None;
                                ui.label("(G81 single shot)");
                            }
                            ui.end_row();
                        }
                    });
            }

            // -- Status ----------------------------------------------------
            if let Some(status) = &app.cam.last_status {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(status).size(11.5).weak());
            }
        });
}
