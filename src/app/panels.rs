use egui::{Color32, RichText, Sense, Stroke, StrokeKind, TextureOptions, vec2};
use uuid::Uuid;
use xuan::{
    blend::BlendMode,
    document::{Document, Layer},
    paint::{PaintMode, ShapeKind},
    selection::SelectionMode,
};

use super::{EditorApp, Tool, icons, menus, theme};

fn value(
    ui: &mut egui::Ui,
    label: &str,
    number: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.label(RichText::new(label).color(theme::MUTED));
    ui.add(
        egui::DragValue::new(number)
            .speed(1.0)
            .range(range)
            .max_decimals(1),
    )
    .changed()
}

fn layer_rows(
    document: &Document,
    collapsed: &std::collections::HashSet<Uuid>,
) -> Vec<(Layer, usize)> {
    fn visit(
        document: &Document,
        collapsed: &std::collections::HashSet<Uuid>,
        parent: Option<Uuid>,
        depth: usize,
        out: &mut Vec<(Layer, usize)>,
    ) {
        if depth > 64 {
            return;
        }
        for layer in document.layers.iter().rev().filter(|l| l.parent == parent) {
            out.push((layer.clone(), depth));
            if layer.group && !collapsed.contains(&layer.id) {
                visit(document, collapsed, Some(layer.id), depth + 1, out);
            }
        }
    }
    let mut rows = Vec::new();
    visit(document, collapsed, None, 0, &mut rows);
    rows
}

impl EditorApp {
    pub(super) fn tool_options(&mut self, ctx: &egui::Context) {
        let mut transform = self
            .session()
            .and_then(|s| xuan::operations::transform_box(&s.document, self.mask_target));
        let mut changed = false;
        egui::TopBottomPanel::top("tool_options")
            .exact_height(42.0)
            .frame(theme::frame())
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.dialog.is_none(), |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("options_scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if self.mask_target {
                                        "Mask"
                                    } else {
                                        self.tool.label()
                                    })
                                    .strong(),
                                );
                                ui.add_space(8.0);
                                match self.tool {
                                    Tool::Move => {
                                        ui.checkbox(&mut self.auto_select, "Auto Select");
                                        ui.checkbox(&mut self.show_controls, "Show Controls");
                                        ui.separator();
                                        if let Some(t) = &mut transform {
                                            changed |= value(
                                                ui,
                                                "X",
                                                &mut t.x,
                                                -1_000_000.0..=1_000_000.0,
                                            );
                                            changed |= value(
                                                ui,
                                                "Y",
                                                &mut t.y,
                                                -1_000_000.0..=1_000_000.0,
                                            );
                                            let old = *t;
                                            if value(ui, "W", &mut t.width, 1.0..=300_000.0) {
                                                if self.lock_ratio {
                                                    t.height *= t.width / old.width;
                                                }
                                                changed = true;
                                            }
                                            if value(ui, "H", &mut t.height, 1.0..=300_000.0) {
                                                if self.lock_ratio {
                                                    t.width *= t.height / old.height;
                                                }
                                                changed = true;
                                            }
                                            ui.checkbox(&mut self.lock_ratio, "Link");
                                            changed |=
                                                value(ui, "Angle", &mut t.rotation, -360.0..=360.0);
                                            ui.label("°");
                                        } else {
                                            ui.label(
                                                RichText::new("Select a layer to transform")
                                                    .color(theme::MUTED),
                                            );
                                        }
                                    }
                                    tool if tool.is_brush() => {
                                        if self.tool == Tool::Blur {
                                            ui.selectable_value(
                                                &mut self.blur_mode,
                                                PaintMode::Blur,
                                                "Blur",
                                            );
                                            ui.selectable_value(
                                                &mut self.blur_mode,
                                                PaintMode::Smudge,
                                                "Smudge",
                                            );
                                        }
                                        if self.tool == Tool::Clone {
                                            ui.checkbox(&mut self.clone_aligned, "Aligned");
                                            ui.checkbox(&mut self.clone_all, "All Layers");
                                        }
                                        value(ui, "Size", &mut self.brush.diameter, 1.0..=2000.0);
                                        ui.label("px");
                                        ui.label("Hardness");
                                        ui.add(
                                            egui::Slider::new(&mut self.brush.hardness, 0.0..=1.0)
                                                .custom_formatter(|v, _| {
                                                    format!("{:.0}%", v * 100.0)
                                                }),
                                        );
                                        ui.label("Opacity");
                                        ui.add(
                                            egui::Slider::new(&mut self.brush.opacity, 0.01..=1.0)
                                                .custom_formatter(|v, _| {
                                                    format!("{:.0}%", v * 100.0)
                                                }),
                                        );
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                    }
                                    tool if tool.is_selection() => {
                                        for (mode, label) in [
                                            (SelectionMode::Replace, "New"),
                                            (SelectionMode::Add, "Add"),
                                            (SelectionMode::Subtract, "Subtract"),
                                            (SelectionMode::Intersect, "Intersect"),
                                        ] {
                                            ui.selectable_value(
                                                &mut self.selection_mode,
                                                mode,
                                                label,
                                            );
                                        }
                                        ui.separator();
                                        match self.tool {
                                            Tool::Marquee => {
                                                ui.selectable_value(
                                                    &mut self.ellipse,
                                                    false,
                                                    "Rectangle",
                                                );
                                                ui.selectable_value(
                                                    &mut self.ellipse,
                                                    true,
                                                    "Ellipse",
                                                );
                                            }
                                            Tool::Lasso => {
                                                ui.selectable_value(
                                                    &mut self.polygonal,
                                                    false,
                                                    "Freehand",
                                                );
                                                ui.selectable_value(
                                                    &mut self.polygonal,
                                                    true,
                                                    "Polygonal",
                                                );
                                            }
                                            Tool::Wand => {
                                                ui.label("Tolerance");
                                                ui.add(
                                                    egui::DragValue::new(&mut self.tolerance)
                                                        .range(0..=255),
                                                );
                                                ui.checkbox(&mut self.contiguous, "Contiguous");
                                            }
                                            _ => {}
                                        }
                                    }
                                    Tool::Gradient => {
                                        ui.selectable_value(&mut self.radial, false, "Linear");
                                        ui.selectable_value(&mut self.radial, true, "Radial");
                                        ui.separator();
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                        ui.label("→");
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.background,
                                        );
                                        ui.label("Opacity");
                                        ui.add(
                                            egui::Slider::new(&mut self.brush.opacity, 0.0..=1.0)
                                                .custom_formatter(|v, _| {
                                                    format!("{:.0}%", v * 100.0)
                                                }),
                                        );
                                    }
                                    Tool::Shape => {
                                        ui.selectable_value(
                                            &mut self.shape_kind,
                                            ShapeKind::Rectangle,
                                            "Rectangle",
                                        );
                                        ui.selectable_value(
                                            &mut self.shape_kind,
                                            ShapeKind::RoundedRectangle,
                                            "Rounded",
                                        );
                                        ui.selectable_value(
                                            &mut self.shape_kind,
                                            ShapeKind::Ellipse,
                                            "Ellipse",
                                        );
                                        ui.separator();
                                        ui.label("Fill");
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                        if self.shape_kind == ShapeKind::RoundedRectangle {
                                            value(
                                                ui,
                                                "Radius",
                                                &mut self.corner_radius,
                                                0.0..=1000.0,
                                            );
                                            ui.label("px");
                                        }
                                    }
                                    Tool::Crop => {
                                        ui.label(
                                            RichText::new(
                                                "Drag a crop area, then press Enter to apply",
                                            )
                                            .color(theme::MUTED),
                                        );
                                    }
                                    Tool::Dropper => {
                                        ui.label("Sample: All visible layers");
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                    }
                                    Tool::Hand | Tool::Zoom => {
                                        ui.label(
                                        RichText::new(
                                            "Scroll to zoom · Space-drag to pan · Ctrl+0 to fit",
                                        )
                                        .color(theme::MUTED),
                                    );
                                    }
                                    _ => {}
                                }
                            });
                        });
                });
            });
        if changed && let Some(transform) = transform {
            let mask_target = self.mask_target;
            self.edit("Transform", |doc| {
                xuan::operations::apply_transform(doc, transform, mask_target)
            });
        }
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(30.0)
            .frame(theme::frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(session) = self.session() {
                        ui.label(
                            RichText::new(format!("{:.1}%", session.zoom * 100.0))
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "{} × {} px",
                                session.document.width, session.document.height
                            ))
                            .size(11.0)
                            .color(theme::MUTED),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("sRGB · Transparent")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    } else {
                        ui.label(
                            RichText::new("Ready when you are")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(self.tool.hint())
                                    .size(11.0)
                                    .color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    });
                });
            });
    }

    pub(super) fn tool_rail(&mut self, ctx: &egui::Context) {
        let mut tool = None;
        egui::SidePanel::left("tools")
            .exact_width(56.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 12)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.dialog.is_none(), |ui| {
                    egui::ScrollArea::vertical()
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 5.0;
                            for t in Tool::ALL {
                                if icons::tool_button(ui, t, self.tool == t).clicked() {
                                    tool = Some(t);
                                }
                            }
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(5.0);
                            ui.color_edit_button_srgba_unmultiplied(&mut self.brush.color)
                                .on_hover_text("Foreground color");
                            ui.color_edit_button_srgba_unmultiplied(&mut self.background)
                                .on_hover_text("Background color");
                            if ui
                                .small_button("⇄")
                                .on_hover_text("Swap colors (X)")
                                .clicked()
                            {
                                std::mem::swap(&mut self.brush.color, &mut self.background);
                            }
                        });
                });
            });
        if let Some(tool) = tool {
            self.set_tool(tool);
        }
    }

    pub(super) fn layers_panel(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let mut adjustment = None;
        let mut visibility = None;
        let mut select = None;
        let mut collapse = None;
        let mut reorder = None;
        let mut appearance = None;
        let mut rename = None;
        let mut edit_adjustment = None;
        egui::SidePanel::right("layers_panel").default_width(252.0).width_range(202.0..=352.0).resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(0))).show(ctx,|ui|{
            ui.add_enabled_ui(self.dialog.is_none(),|ui|{
                egui::Frame::new().inner_margin(egui::Margin::symmetric(17,15)).show(ui,|ui|{ui.horizontal(|ui|{
                    ui.label(RichText::new("Layers").strong());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(RichText::new(self.session().map_or(0,|s|s.document.layers.len()).to_string()).color(theme::MUTED).small());});
                });});
                ui.separator();
                let active=self.session().and_then(|s|s.document.active()).cloned();
                egui::Frame::new().inner_margin(egui::Margin::symmetric(12,7)).show(ui,|ui|{
                    ui.add_enabled_ui(active.is_some(),|ui|{
                        let mut blend=active.as_ref().map_or(BlendMode::Normal,|l|l.blend);
                        let mut opacity=active.as_ref().map_or(1.0,|l|l.opacity);
                        let mut changed=false;
                        ui.horizontal(|ui|{
                            egui::ComboBox::from_id_salt("blend_mode").width(ui.available_width()-38.0).selected_text(blend.name()).show_ui(ui,|ui|{for b in BlendMode::ALL{changed|=ui.selectable_value(&mut blend,b,b.name()).changed();}});
                            if let Some(active)=&active {let mut locked=active.locked;if ui.checkbox(&mut locked,"").on_hover_text("Lock layer").changed(){appearance=Some((blend,opacity,locked));}}
                        });
                        ui.horizontal(|ui|{ui.label(RichText::new("Opacity").color(theme::MUTED));ui.spacing_mut().slider_width=(ui.available_width()-72.0).max(40.0);changed|=ui.add(egui::Slider::new(&mut opacity,0.0..=1.0).custom_formatter(|v,_|format!("{:.0}%",v*100.0))).changed();});
                        if changed {appearance=Some((blend,opacity,active.as_ref().is_some_and(|l|l.locked)));}
                    });
                });
                ui.separator();
                let list_height=(ui.available_height()-76.0).max(40.0);
                egui::ScrollArea::vertical().id_salt("layers_scroll").max_height(list_height).min_scrolled_height(list_height).auto_shrink([false,false]).show(ui,|ui|{
                    if self.sessions.is_empty(){
                        ui.add_space((list_height*0.5-48.0).max(10.0));
                        ui.vertical_centered(|ui|{ui.label(RichText::new("◇").size(28.0).color(theme::MUTED));ui.label(RichText::new("No layers yet").color(theme::MUTED));ui.add_space(2.0);ui.label(RichText::new("Create a canvas or import an image.").size(11.0).color(theme::MUTED));});
                    }else{
                        let rows=layer_rows(&self.sessions[self.current].document,&self.sessions[self.current].collapsed);
                        for (layer,depth) in rows {
                            let selected=self.sessions[self.current].document.selected.contains(&layer.id);
                            let row_width = ui.available_width();
                            let row=egui::Frame::new().fill(if selected{Color32::from_gray(57)}else{theme::PANEL}).inner_margin(egui::Margin::symmetric(8,6)).show(ui,|ui|{
                                ui.set_min_width((row_width - 16.0).max(0.0));
                                ui.horizontal(|ui|{
                                    ui.spacing_mut().item_spacing.x=6.0;
                                    if icons::eye(ui,layer.visible).clicked(){visibility=Some(layer.id);}
                                    ui.add_space((depth as f32*12.0).min(60.0));
                                    if layer.group {
                                        let collapsed=self.sessions[self.current].collapsed.contains(&layer.id);
                                        if ui.small_button(if collapsed{"▸"}else{"▾"}).clicked(){collapse=Some(layer.id);}
                                        icons::action_button(ui, "group");
                                    } else {
                                        if layer.clip_to.is_some(){ui.label(RichText::new("↳").small().color(theme::MUTED));}
                                        let key=(layer.id,false);
                                        if let Some(pixels)=&layer.pixels {
                                            let session=&mut self.sessions[self.current];
                                            let texture=session.thumbnails.entry(key).or_insert_with(||{
                                                let image=image::imageops::thumbnail(&**pixels,42,32);
                                                ctx.load_texture(format!("thumb-{}",layer.id),egui::ColorImage::from_rgba_unmultiplied([image.width() as usize,image.height() as usize],image.as_raw()),TextureOptions::LINEAR)
                                            });
                                            if ui.add(egui::Image::new((texture.id(),vec2(38.0,30.0))).fit_to_exact_size(vec2(38.0,30.0)).sense(Sense::click())).clicked(){select=Some((layer.id,false));}
                                        }else{
                                            let (rect,response)=ui.allocate_exact_size(vec2(38.0,30.0),Sense::click());
                                            ui.painter().rect(rect,2.0,Color32::from_gray(48),Stroke::new(1.0_f32,Color32::from_gray(85)),StrokeKind::Inside);
                                            ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,if layer.adjustment.is_some(){"◐"}else{"▧"},egui::FontId::proportional(18.0),theme::MUTED);
                                            if response.clicked(){select=Some((layer.id,false));}
                                        }
                                    }
                                    if let Some(mask)=&layer.mask {
                                        let texture=self.sessions[self.current].thumbnails.entry((layer.id,true)).or_insert_with(||{
                                            let image=image::imageops::resize(&*mask.pixels,28,28,image::imageops::FilterType::Triangle);
                                            let color=egui::ColorImage::from_gray([28,28],image.as_raw());
                                            ctx.load_texture(format!("mask-{}",layer.id),color,TextureOptions::LINEAR)
                                        });
                                        let response=ui.add(egui::Image::new((texture.id(),vec2(28.0,28.0))).sense(Sense::click()));
                                        if selected&&self.mask_target{ui.painter().rect_stroke(response.rect.expand(2.0),2.0,Stroke::new(1.0_f32,theme::TEXT),StrokeKind::Outside);}
                                        if response.clicked(){select=Some((layer.id,true));}
                                        if !mask.enabled{ui.painter().line_segment([response.rect.left_top(),response.rect.right_bottom()],Stroke::new(1.5_f32,Color32::LIGHT_RED));}
                                    }
                                    let response=ui.add(egui::Label::new(RichText::new(&layer.name).color(if layer.visible{theme::TEXT}else{theme::MUTED})).truncate().sense(Sense::click_and_drag()));
                                    if response.clicked(){select=Some((layer.id,false));}
                                    if response.double_clicked(){if layer.adjustment.is_some(){edit_adjustment=Some(layer.id);}else{rename=Some((layer.id,layer.name.clone()));}}
                                    if response.drag_started(){response.dnd_set_drag_payload(layer.id);}
                                    if layer.locked {ui.label(RichText::new("·").color(theme::MUTED));}
                                });
                            });
                            row.response.context_menu(|ui|{
                                if layer.adjustment.is_some() && ui.button("Edit adjustment…").clicked() {edit_adjustment=Some(layer.id);ui.close();}
                                if ui.button("Rename…").clicked(){rename=Some((layer.id,layer.name.clone()));ui.close();}
                                for (label,command) in [("Duplicate","duplicate"),("New Group","group"),("Merge Down / Selected","merge"),("Add Mask","mask"),("Clipping Mask","clip"),("Delete","delete_layer")] {
                                    if ui.button(label).clicked(){select=Some((layer.id,false));action=Some(command);ui.close();}
                                }
                                if layer.mask.is_some(){ui.separator();for(label,command)in[("Enable / Disable Mask","disable_mask"),("Link / Unlink Mask","link_mask"),("Delete Mask","delete_mask")] {if ui.button(label).clicked(){select=Some((layer.id,true));action=Some(command);ui.close();}}}
                            });
                            if let Some(source)=row.response.dnd_release_payload::<Uuid>() {reorder=Some((*source,layer.id,ctx.input(|i|i.modifiers.alt)));}
                            if row.response.dnd_hover_payload::<Uuid>().is_some(){ui.painter().line_segment([row.response.rect.left_top(),row.response.rect.right_top()],Stroke::new(2.0_f32,theme::ACCENT));}
                        }
                    }
                });
                ui.separator();
                egui::Frame::new().inner_margin(egui::Margin::symmetric(9,7)).show(ui,|ui|{ui.add_enabled_ui(!self.sessions.is_empty(),|ui|{ui.horizontal(|ui|{
                    ui.spacing_mut().item_spacing.x=4.0;
                    for (tip, command) in [("New layer (Ctrl+Shift+N)", "new_layer"), ("Group layers (Ctrl+G)", "group"), ("Add layer mask", "mask")] {
                        if icons::action_button(ui, command).on_hover_text(tip).clicked() { action = Some(command); }
                    }
                    ui.menu_button("Adjust",|ui|{adjustment=menus::adjustment_menu(ui);}).response.on_hover_text("New adjustment layer");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if icons::action_button(ui, "delete_layer").on_hover_text("Delete layer").clicked(){action=Some("delete_layer");}});
                });});});
            });
        });
        if let Some(id) = visibility {
            self.edit("Layer Visibility", |doc| {
                if let Some(l) = doc.layers.iter_mut().find(|l| l.id == id) {
                    l.visible = !l.visible;
                }
                Ok(())
            });
        }
        if let Some((id, mask)) = select {
            let extend = ctx.input(|i| i.modifiers.shift || i.modifiers.ctrl);
            if let Some(s) = self.session_mut() {
                s.document.select(id, extend);
            }
            self.mask_target = mask;
        }
        if let Some(id) = collapse
            && let Some(s) = self.session_mut()
            && !s.collapsed.remove(&id)
        {
            s.collapsed.insert(id);
        }
        if let Some((blend, opacity, locked)) = appearance {
            self.edit("Layer Appearance", |doc| {
                if let Some(l) = doc.active_mut() {
                    l.blend = blend;
                    l.opacity = opacity;
                    l.locked = locked;
                }
                Ok(())
            });
        }
        if let Some((source, target, duplicate)) = reorder {
            self.edit("Reorder Layer", |doc| {
                if source == target || doc.descendants(source).contains(&target) {
                    return Ok(());
                }
                let Some(destination) = doc.layers.iter().find(|l| l.id == target).cloned() else {
                    return Ok(());
                };
                if duplicate {
                    doc.select(source, false);
                    xuan::operations::duplicate(doc);
                }
                let source = if duplicate {
                    doc.active.unwrap()
                } else {
                    source
                };
                let Some(index) = doc.layers.iter().position(|l| l.id == source) else {
                    return Ok(());
                };
                let mut layer = doc.layers.remove(index);
                layer.parent = if destination.group {
                    Some(target)
                } else {
                    destination.parent
                };
                layer.clip_to = None;
                let index = doc
                    .layers
                    .iter()
                    .position(|l| l.id == target)
                    .map_or(doc.layers.len(), |i| i + 1);
                doc.layers.insert(index, layer);
                doc.select(source, false);
                Ok(())
            });
        }
        if let Some(id) = edit_adjustment {
            self.edit_adjustment_layer(id);
        }
        if let Some(rename) = rename {
            self.rename = Some(rename);
        }
        if let Some(action) = action {
            self.command(action);
        }
        if let Some(adjustment) = adjustment {
            self.start_adjustment(adjustment, true);
        }
    }
}
