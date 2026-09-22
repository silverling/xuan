use super::{EditorApp, Tool};
use egui::{Event, Key, Modifiers};

impl EditorApp {
    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }

        // Native backends translate clipboard shortcuts into these events,
        // including Ctrl+Shift+C. Leave them alone when a text field has focus.
        let clipboard_commands = ctx.input_mut(|input| {
            let mut commands = Vec::new();
            if self.develop.is_some() {
                return commands;
            }
            input.events.retain(|event| {
                let command = match event {
                    Event::Copy if input.modifiers.shift => "copy_merged",
                    Event::Copy => "copy",
                    Event::Cut => "cut",
                    Event::Paste(text) => {
                        commands.push(("paste", Some(text.clone())));
                        return false;
                    }
                    _ => return true,
                };
                commands.push((command, None));
                false
            });
            commands
        });
        if !clipboard_commands.is_empty() {
            for (command, text) in clipboard_commands {
                if command == "paste" {
                    self.paste_clipboard(text.as_deref());
                } else {
                    self.command(command);
                }
            }
            return;
        }

        let pressed = |key| ctx.input(|i| i.key_pressed(key));
        let modifiers = ctx.input(|i| i.modifiers);
        let consume = |mods, key| ctx.input_mut(|i| i.consume_key(mods, key));
        let ctrl = Modifiers::CTRL;
        let shift = Modifiers::CTRL | Modifiers::SHIFT;
        for (mods, key, command) in [
            (ctrl | Modifiers::ALT | Modifiers::SHIFT, Key::S, "export"),
            (shift, Key::N, "new_layer"),
            (shift, Key::O, "import"),
            (shift, Key::S, "save_as"),
            (shift, Key::Z, "redo"),
            (shift, Key::C, "copy_merged"),
            (shift, Key::G, "ungroup"),
            (shift, Key::I, "invert_selection"),
            (ctrl | Modifiers::ALT, Key::G, "clip"),
            (ctrl, Key::N, "new"),
            (ctrl, Key::O, "open"),
            (ctrl, Key::S, "save"),
            (ctrl, Key::W, "close"),
            (ctrl, Key::Z, "undo"),
            (ctrl, Key::Y, "redo"),
            (ctrl, Key::J, "duplicate"),
            (ctrl, Key::G, "group"),
            (ctrl, Key::E, "merge"),
            (ctrl, Key::A, "select_all"),
            (ctrl, Key::D, "deselect"),
            (ctrl, Key::I, "invert"),
            (ctrl, Key::L, "levels"),
            (ctrl, Key::U, "hue"),
            (ctrl, Key::M, "curves"),
            (ctrl, Key::C, "copy"),
            (ctrl, Key::X, "cut"),
            (ctrl, Key::V, "paste"),
            (ctrl, Key::Num0, "fit"),
            (ctrl, Key::Num1, "actual"),
            (ctrl, Key::Plus, "zoom_in"),
            (ctrl, Key::Equals, "zoom_in"),
            (ctrl, Key::Minus, "zoom_out"),
            (Modifiers::ALT, Key::Backspace, "fill_fg"),
            (ctrl, Key::Backspace, "fill_bg"),
        ] {
            if consume(mods, key) {
                self.command(command);
                return;
            }
        }
        if let Some(develop) = &mut self.develop {
            if pressed(Key::Escape) {
                develop.picker = false;
                develop.draw_overlay = false;
            }
            if pressed(Key::F1) {
                self.command("shortcuts");
            }
            return;
        }
        if modifiers.ctrl {
            if pressed(Key::H) {
                self.show_controls = !self.show_controls;
            }
            if pressed(Key::T) {
                self.set_tool(Tool::Move);
                self.show_controls = true;
            }
            return;
        }
        if modifiers.shift && pressed(Key::F5) {
            self.command("content_fill");
        }
        if pressed(Key::F1) {
            self.command("shortcuts");
        }
        if pressed(Key::Escape) {
            self.cancel_gesture();
            self.crop_rect = None;
            self.polygon.clear();
        }
        if pressed(Key::Enter) {
            if let Some((start, end)) = self.crop_rect.take() {
                self.edit("Crop", |doc| xuan::operations::crop(doc, start, end));
                if let Some(s) = self.session_mut() {
                    s.fit = true;
                }
            } else if self.polygon.len() >= 3 {
                self.finish_polygon();
            }
        }
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            if self
                .session()
                .is_some_and(|s| s.document.selection.is_some())
            {
                self.command("clear");
            } else {
                self.command("delete_layer");
            }
        }
        for (key, tool) in [
            (Key::V, Tool::Move),
            (Key::M, Tool::Marquee),
            (Key::L, Tool::Lasso),
            (Key::W, Tool::Wand),
            (Key::C, Tool::Crop),
            (Key::B, Tool::Brush),
            (Key::E, Tool::Erase),
            (Key::J, Tool::Heal),
            (Key::S, Tool::Clone),
            (Key::R, Tool::Blur),
            (Key::G, Tool::Gradient),
            (Key::U, Tool::Shape),
            (Key::T, Tool::Text),
            (Key::I, Tool::Dropper),
            (Key::H, Tool::Hand),
            (Key::Z, Tool::Zoom),
        ] {
            if pressed(key) {
                if modifiers.shift && tool == Tool::Marquee {
                    self.ellipse = !self.ellipse;
                }
                if modifiers.shift && tool == Tool::Lasso {
                    self.polygonal = !self.polygonal;
                }
                if modifiers.shift && tool == Tool::Shape {
                    self.shape_kind = if self.shape_kind == xuan::paint::ShapeKind::Ellipse {
                        xuan::paint::ShapeKind::Rectangle
                    } else {
                        xuan::paint::ShapeKind::Ellipse
                    };
                }
                self.set_tool(tool);
            }
        }
        if pressed(Key::X) {
            std::mem::swap(&mut self.brush.color, &mut self.background);
        }
        if pressed(Key::D) {
            self.brush.color = [0, 0, 0, 255];
            self.background = [255; 4];
        }
        if pressed(Key::OpenBracket) {
            if modifiers.shift {
                self.brush.hardness = (self.brush.hardness - 0.1).max(0.0);
            } else {
                self.brush.diameter = (self.brush.diameter / 1.15).round().max(1.0);
            }
        }
        if pressed(Key::CloseBracket) {
            if modifiers.shift {
                self.brush.hardness = (self.brush.hardness + 0.1).min(1.0);
            } else {
                self.brush.diameter = (self.brush.diameter * 1.15).round().min(2000.0);
            }
        }
        for (index, key) in [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
            Key::Num0,
        ]
        .into_iter()
        .enumerate()
        {
            if pressed(key) {
                let opacity = (index + 1) as f32 / 10.0;
                if self.tool.is_brush() || self.tool == Tool::Gradient {
                    self.brush.opacity = opacity;
                } else {
                    self.edit("Layer Opacity", |doc| {
                        let selected = doc.selected.clone();
                        for l in &mut doc.layers {
                            if selected.contains(&l.id) && !l.group {
                                l.opacity = opacity;
                            }
                        }
                        Ok(())
                    });
                }
            }
        }
        let step = if modifiers.shift { 10.0 } else { 1.0 };
        let mut dx = 0.0;
        let mut dy = 0.0;
        if pressed(Key::ArrowLeft) {
            dx -= step;
        }
        if pressed(Key::ArrowRight) {
            dx += step;
        }
        if pressed(Key::ArrowUp) {
            dy -= step;
        }
        if pressed(Key::ArrowDown) {
            dy += step;
        }
        if dx != 0.0 || dy != 0.0 {
            let mask_target = self.transforming_mask();
            self.edit("Nudge", |doc| {
                if let Some(mut transform) = xuan::operations::transform_box(doc, mask_target) {
                    transform.x += dx;
                    transform.y += dy;
                    xuan::operations::apply_transform(doc, transform, mask_target)?;
                }
                Ok(())
            });
        }
    }
}
