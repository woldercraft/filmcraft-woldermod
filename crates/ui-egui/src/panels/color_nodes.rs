//! DaVinci Resolve-style Node Graph panel for color grading.
//!
//! Visualizes Lumetri color grading layers as an interactive serial node chain:
//! `[ IN ] -> [ Node 01 ] -> [ Node 02 ] -> ... -> [ OUT ]`
//!
//! Keyboard shortcuts:
//! - `Alt + S`: Add Serial Node
//! - `Ctrl + D` / `Cmd + D`: Bypass Active Node
//! - `Shift + D`: Bypass All Nodes
//! - `Delete` / `Backspace`: Delete Active Node

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use filmcraft_project::{EffectInstance, ParamValue};
use serde_json::json;

use crate::FilmcraftApp;
use crate::panels::lumetri::active_color_clip;
use crate::theme::Tokens;

/// Analyze an effect instance to determine which color modules are active.
fn detect_node_badges(e: &EffectInstance) -> Vec<(&'static str, &'static str)> {
    let mut badges = Vec::new();
    let has_f = |id: &str, def: f64| {
        e.param(id)
            .and_then(|p| match p.value {
                ParamValue::Float(v) => Some((v - def).abs() > 0.001),
                _ => None,
            })
            .unwrap_or(false)
    };
    let has_txt = |id: &str| {
        e.param(id)
            .and_then(|p| match &p.value {
                ParamValue::Text(s) => Some(!s.is_empty()),
                _ => None,
            })
            .unwrap_or(false)
    };
    let has_choice = |id: &str| {
        e.param(id)
            .and_then(|p| match p.value {
                ParamValue::Choice(c) => Some(c > 0),
                _ => None,
            })
            .unwrap_or(false)
    };

    if has_f("exposure", 0.0)
        || has_f("contrast", 0.0)
        || has_f("temperature", 0.0)
        || has_f("tint", 0.0)
        || has_f("highlights", 0.0)
        || has_f("shadows", 0.0)
        || has_f("whites", 0.0)
        || has_f("blacks", 0.0)
        || has_f("saturation", 100.0)
    {
        badges.push(("PRI", "Primary"));
    }
    if has_txt("input_lut") || has_txt("look_lut") || has_choice("look") {
        badges.push(("LUT", "Look / LUT"));
    }
    if e.param("curves_on").and_then(|p| p.value.as_bool()).unwrap_or(false) {
        badges.push(("CRV", "Curves"));
    }
    if has_f("wheel_shadows_l", 0.0) || has_f("wheel_midtones_l", 0.0) || has_f("wheel_highlights_l", 0.0) {
        badges.push(("WHL", "Wheels"));
    }
    if e.param("hsl_on").and_then(|p| p.value.as_bool()).unwrap_or(false) {
        badges.push(("HSL", "HSL"));
    }
    if has_f("vignette_amount", 0.0) {
        badges.push(("VIG", "Vignette"));
    }

    if badges.is_empty() {
        badges.push(("GRADE", "Grade"));
    }
    badges
}

/// Draw a smooth cubic bezier connection line between two pins.
fn draw_bezier_wire(painter: &egui::Painter, p0: Pos2, p1: Pos2, stroke: Stroke) {
    let dx = (p1.x - p0.x).abs() * 0.55;
    let c0 = pos2(p0.x + dx, p0.y);
    let c1 = pos2(p1.x - dx, p1.y);
    let steps = 28;
    let mut prev = p0;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        let p = pos2(
            u * u * u * p0.x + 3.0 * u * u * t * c0.x + 3.0 * u * t * t * c1.x + t * t * t * p1.x,
            u * u * u * p0.y + 3.0 * u * u * t * c0.y + 3.0 * u * t * t * c1.y + t * t * t * p1.y,
        );
        painter.line_segment([prev, p], stroke);
        prev = p;
    }
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;

    // Outer frame with dark background
    ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(20, 22, 26));

    let Some((clip, it)) = active_color_clip(app) else {
        crate::dock::placeholder(ui, rect, &t, tl!("(no clip selected)"));
        return;
    };

    let lumetri_effects: Vec<(usize, &EffectInstance)> = it.effects.iter().enumerate().filter(|(_, e)| e.effect == "lumetri").collect();

    let node_count = lumetri_effects.len();
    let active_node = if node_count == 0 {
        0
    } else {
        app.ui.active_color_node.min(node_count - 1)
    };
    app.ui.active_color_node = active_node;

    // Handle Keyboard Shortcuts when typing isn't active
    let mut add_serial = false;
    let mut bypass_active = false;
    let mut bypass_all = false;
    let mut delete_active = false;
    let mut reset_active = false;

    if !ui.ctx().egui_wants_keyboard_input() && ui.rect_contains_pointer(rect) {
        ui.input(|i| {
            if i.modifiers.alt && i.key_pressed(egui::Key::S) {
                add_serial = true;
            } else if (i.modifiers.command || i.modifiers.ctrl) && !i.modifiers.shift && i.key_pressed(egui::Key::D) {
                bypass_active = true;
            } else if i.modifiers.shift && i.key_pressed(egui::Key::D) {
                bypass_all = true;
            } else if (i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) && node_count > 1 {
                delete_active = true;
            } else if i.modifiers.alt && i.key_pressed(egui::Key::Home) {
                reset_active = true;
            }
        });
    }

    // Top Toolbar
    let toolbar_h = 34.0;
    let toolbar_r = Rect::from_min_size(rect.min, vec2(rect.width(), toolbar_h));
    ui.painter().rect_filled(toolbar_r, 0.0, Color32::from_rgb(26, 28, 33));
    ui.painter().line_segment(
        [pos2(toolbar_r.min.x, toolbar_r.max.y), pos2(toolbar_r.max.x, toolbar_r.max.y)],
        Stroke::new(1.0, Color32::from_rgb(38, 41, 48)),
    );

    let mut bx = toolbar_r.min.x + 8.0;
    let by = toolbar_r.min.y + 5.0;
    let btn_h = 24.0;

    // + Add Serial Node
    let add_w = 145.0;
    let add_r = Rect::from_min_size(pos2(bx, by), vec2(add_w, btn_h));
    let add_resp = ui.interact(add_r, egui::Id::new("color_nodes_add_serial_btn"), Sense::click()).on_hover_text(tl!("Add Serial Node (Alt+S)"));
    app.auto.add("color_nodes.add_serial", add_r, "Add Serial Node");
    let add_bg = if add_resp.hovered() { t.hover } else { Color32::from_rgb(36, 40, 48) };
    ui.painter().rect_filled(add_r, 4.0, add_bg);
    ui.painter().rect_stroke(add_r, 4.0, Stroke::new(1.0, Color32::from_rgb(55, 60, 72)), StrokeKind::Inside);
    ui.painter().text(add_r.center(), Align2::CENTER_CENTER, tl!("+ Add Serial (Alt+S)"), Tokens::ui(11.0), t.text);
    if add_resp.clicked() {
        add_serial = true;
    }
    bx += add_w + 8.0;

    if node_count > 0 {
        // Bypass Node
        let curr_enabled = lumetri_effects[active_node].1.enabled;
        let bypass_w = 130.0;
        let bypass_r = Rect::from_min_size(pos2(bx, by), vec2(bypass_w, btn_h));
        let bypass_resp = ui.interact(bypass_r, egui::Id::new("color_nodes_bypass_btn"), Sense::click());
        app.auto.add("color_nodes.bypass_node", bypass_r, "Bypass Node");
        let bypass_bg = if bypass_resp.hovered() { t.hover } else { Color32::from_rgb(36, 40, 48) };
        ui.painter().rect_filled(bypass_r, 4.0, bypass_bg);
        ui.painter().rect_stroke(bypass_r, 4.0, Stroke::new(1.0, Color32::from_rgb(55, 60, 72)), StrokeKind::Inside);
        let b_text = if curr_enabled { tl!("Bypass Node (Ctrl+D)") } else { tl!("Enable Node (Ctrl+D)") };
        ui.painter().text(bypass_r.center(), Align2::CENTER_CENTER, b_text, Tokens::ui(11.0), t.text);
        if bypass_resp.clicked() {
            bypass_active = true;
        }
        bx += bypass_w + 6.0;

        // Bypass All
        let all_bypassed = lumetri_effects.iter().all(|(_, e)| !e.enabled);
        let all_w = 125.0;
        let all_r = Rect::from_min_size(pos2(bx, by), vec2(all_w, btn_h));
        let all_resp = ui.interact(all_r, egui::Id::new("color_nodes_bypass_all_btn"), Sense::click());
        app.auto.add("color_nodes.bypass_all", all_r, "Bypass All Nodes");
        let all_bg = if all_resp.hovered() { t.hover } else { Color32::from_rgb(36, 40, 48) };
        ui.painter().rect_filled(all_r, 4.0, all_bg);
        ui.painter().rect_stroke(all_r, 4.0, Stroke::new(1.0, Color32::from_rgb(55, 60, 72)), StrokeKind::Inside);
        let a_text = if all_bypassed { tl!("Enable All (Shift+D)") } else { tl!("Bypass All (Shift+D)") };
        ui.painter().text(all_r.center(), Align2::CENTER_CENTER, a_text, Tokens::ui(11.0), t.text);
        if all_resp.clicked() {
            bypass_all = true;
        }
        bx += all_w + 6.0;

        // Reset Node
        let rst_w = 75.0;
        let rst_r = Rect::from_min_size(pos2(bx, by), vec2(rst_w, btn_h));
        let rst_resp = ui.interact(rst_r, egui::Id::new("color_nodes_rst_btn"), Sense::click());
        app.auto.add("color_nodes.reset_node", rst_r, "Reset Node");
        let rst_bg = if rst_resp.hovered() { t.hover } else { Color32::from_rgb(36, 40, 48) };
        ui.painter().rect_filled(rst_r, 4.0, rst_bg);
        ui.painter().rect_stroke(rst_r, 4.0, Stroke::new(1.0, Color32::from_rgb(55, 60, 72)), StrokeKind::Inside);
        ui.painter().text(rst_r.center(), Align2::CENTER_CENTER, tl!("Reset"), Tokens::ui(11.0), t.text);
        if rst_resp.clicked() {
            reset_active = true;
        }
        bx += rst_w + 6.0;

        // Delete Node
        if node_count > 1 {
            let del_w = 60.0;
            let del_r = Rect::from_min_size(pos2(bx, by), vec2(del_w, btn_h));
            let del_resp = ui.interact(del_r, egui::Id::new("color_nodes_del_btn"), Sense::click());
            app.auto.add("color_nodes.delete_node", del_r, "Delete Node");
            let del_bg = if del_resp.hovered() { t.hover } else { Color32::from_rgb(36, 40, 48) };
            ui.painter().rect_filled(del_r, 4.0, del_bg);
            ui.painter().rect_stroke(del_r, 4.0, Stroke::new(1.0, Color32::from_rgb(55, 60, 72)), StrokeKind::Inside);
            ui.painter().text(del_r.center(), Align2::CENTER_CENTER, tl!("Delete"), Tokens::ui(11.0), t.text);
            if del_resp.clicked() {
                delete_active = true;
            }
        }
    }

    // Right info: node counter & clip name
    let info_r = Rect::from_min_max(pos2(toolbar_r.max.x - 220.0, toolbar_r.min.y), pos2(toolbar_r.max.x - 8.0, toolbar_r.max.y));
    let mut right_text = format!("Master · {}", it.name);
    if node_count > 0 {
        right_text = format!("Node {} / {} | {}", active_node + 1, node_count, right_text);
    }
    ui.painter().text(info_r.right_center(), Align2::RIGHT_CENTER, right_text, Tokens::ui(11.0), t.text_dim);

    // Execute Shortcut / Action requests
    if add_serial {
        let _ = app.session.execute("effects.apply", json!({"clips": [clip.0], "effect": "lumetri"}));
        app.ui.active_color_node = node_count;
        return;
    }
    if bypass_active && node_count > 0 {
        let (idx, _) = lumetri_effects[active_node];
        let _ = app.session.execute("effects.toggleEnabled", json!({"clip": clip.0, "index": idx}));
        return;
    }
    if bypass_all && node_count > 0 {
        let any_enabled = lumetri_effects.iter().any(|(_, e)| e.enabled);
        for &(idx, e) in &lumetri_effects {
            if e.enabled == any_enabled {
                let _ = app.session.execute("effects.toggleEnabled", json!({"clip": clip.0, "index": idx}));
            }
        }
        return;
    }
    if reset_active && node_count > 0 {
        let (idx, _) = lumetri_effects[active_node];
        let _ = app.session.execute("effects.reset", json!({"clip": clip.0, "index": idx}));
        return;
    }
    if delete_active && node_count > 1 {
        let (idx, _) = lumetri_effects[active_node];
        let _ = app.session.execute("effects.remove", json!({"clip": clip.0, "index": idx}));
        if active_node >= node_count - 1 {
            app.ui.active_color_node = active_node.saturating_sub(1);
        }
        return;
    }

    // Canvas Area
    let canvas_r = Rect::from_min_max(pos2(rect.min.x, toolbar_r.max.y), rect.max);
    if canvas_r.width() < 10.0 || canvas_r.height() < 10.0 {
        return;
    }

    let painter = ui.painter_at(canvas_r);

    // Draw DaVinci Resolve-style subtle dot grid
    let dot_spacing = 24.0;
    let dot_color = Color32::from_rgb(34, 37, 44);
    let mut gx = canvas_r.min.x + (canvas_r.width() % dot_spacing) * 0.5;
    while gx < canvas_r.max.x {
        let mut gy = canvas_r.min.y + (canvas_r.height() % dot_spacing) * 0.5;
        while gy < canvas_r.max.y {
            painter.circle_filled(pos2(gx, gy), 1.0, dot_color);
            gy += dot_spacing;
        }
        gx += dot_spacing;
    }

    if node_count == 0 {
        // Prompt to add initial node
        let center = canvas_r.center();
        painter.text(
            pos2(center.x, center.y - 20.0),
            Align2::CENTER_CENTER,
            tl!("No color grading nodes on active clip"),
            Tokens::ui(13.0),
            t.text_dim,
        );
        let btn_r = Rect::from_center_size(pos2(center.x, center.y + 15.0), vec2(180.0, 30.0));
        let btn_resp = ui.interact(btn_r, egui::Id::new("init_node_btn"), Sense::click());
        app.auto.add("color_nodes.init_node", btn_r, "Add Color Node");
        if btn_resp.hovered() {
            painter.rect_filled(btn_r, 4.0, t.hover);
        } else {
            painter.rect_filled(btn_r, 4.0, t.accent.gamma_multiply(0.20));
        }
        painter.rect_stroke(btn_r, 4.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
        painter.text(btn_r.center(), Align2::CENTER_CENTER, tl!("+ Add Initial Node"), Tokens::ui(12.0), t.text);
        if btn_resp.clicked() {
            let _ = app.session.execute("effects.apply", json!({"clips": [clip.0], "effect": "lumetri"}));
            app.ui.active_color_node = 0;
        }
        return;
    }

    // Node layout parameters
    let node_w = 140.0;
    let node_h = 78.0;
    let gap = 52.0;
    let port_w = 42.0;
    let port_h = 32.0;

    let total_chain_w = port_w + gap + (node_count as f32 * node_w) + ((node_count - 1) as f32 * gap) + gap + port_w;
    let start_x = (canvas_r.center().x - total_chain_w * 0.5).max(canvas_r.min.x + 24.0);
    let center_y = canvas_r.center().y;

    // Draw [ IN ] Terminal on left
    let in_rect = Rect::from_center_size(pos2(start_x + port_w * 0.5, center_y), vec2(port_w, port_h));
    painter.rect_filled(in_rect, 4.0, Color32::from_rgb(28, 30, 36));
    painter.rect_stroke(in_rect, 4.0, Stroke::new(1.0, Color32::from_rgb(50, 54, 64)), StrokeKind::Inside);
    painter.text(in_rect.center(), Align2::CENTER_CENTER, "IN", Tokens::ui(11.0), Color32::from_rgb(140, 150, 170));

    let in_pin = pos2(in_rect.max.x, center_y);
    painter.circle_filled(in_pin, 4.0, Color32::from_rgb(60, 200, 80));

    // Calculate node pin positions for wires
    let mut node_rects = Vec::with_capacity(node_count);
    let mut prev_output_pin = in_pin;

    for i in 0..node_count {
        let x = start_x + port_w + gap + i as f32 * (node_w + gap);
        let node_rect = Rect::from_min_size(pos2(x, center_y - node_h * 0.5), vec2(node_w, node_h));
        node_rects.push(node_rect);

        let input_pin = pos2(node_rect.min.x, center_y);
        let output_pin = pos2(node_rect.max.x, center_y);

        // Draw wire from previous pin to this node's input pin
        let e = lumetri_effects[i].1;
        let wire_color = if e.enabled {
            Color32::from_rgb(60, 200, 80) // Vibrant green wire
        } else {
            Color32::from_rgb(80, 85, 95) // Dim gray wire when bypassed
        };
        draw_bezier_wire(&painter, prev_output_pin, input_pin, Stroke::new(2.0, wire_color));

        prev_output_pin = output_pin;
    }

    // Draw [ OUT ] Terminal on right
    let out_x = start_x + total_chain_w - port_w * 0.5;
    let out_rect = Rect::from_center_size(pos2(out_x, center_y), vec2(port_w, port_h));
    painter.rect_filled(out_rect, 4.0, Color32::from_rgb(28, 30, 36));
    painter.rect_stroke(out_rect, 4.0, Stroke::new(1.0, Color32::from_rgb(50, 54, 64)), StrokeKind::Inside);
    painter.text(out_rect.center(), Align2::CENTER_CENTER, "OUT", Tokens::ui(11.0), Color32::from_rgb(140, 150, 170));

    let out_pin = pos2(out_rect.min.x, center_y);
    painter.circle_filled(out_pin, 4.0, Color32::from_rgb(60, 200, 80));

    // Draw wire from last node output pin to OUT terminal
    let last_enabled = lumetri_effects.last().map(|(_, e)| e.enabled).unwrap_or(true);
    let out_wire_color = if last_enabled {
        Color32::from_rgb(60, 200, 80)
    } else {
        Color32::from_rgb(80, 85, 95)
    };
    draw_bezier_wire(&painter, prev_output_pin, out_pin, Stroke::new(2.0, out_wire_color));

    // Render Each Node Card
    for (i, &(fx_idx, fx_inst)) in lumetri_effects.iter().enumerate() {
        let node_rect = node_rects[i];
        let is_active = i == active_node;
        let is_enabled = fx_inst.enabled;

        // Card interaction
        let card_id = egui::Id::new(("color_node_card", clip.0, i));
        let card_resp = ui.interact(node_rect, card_id, Sense::click());
        app.auto.add(&format!("color_nodes.node.{i}"), node_rect, &format!("Node {:02}", i + 1));

        if card_resp.clicked() {
            app.ui.active_color_node = i;
        }

        // Node card background
        let bg_color = if is_enabled {
            if is_active {
                Color32::from_rgb(32, 35, 42)
            } else {
                Color32::from_rgb(28, 30, 36)
            }
        } else {
            Color32::from_rgb(22, 23, 27)
        };
        painter.rect_filled(node_rect, 6.0, bg_color);

        // Header bar of the node card
        let header_h = 22.0;
        let header_rect = Rect::from_min_size(node_rect.min, vec2(node_rect.width(), header_h));
        let header_bg = if is_active {
            Color32::from_rgb(38, 42, 52)
        } else {
            Color32::from_rgb(22, 24, 28)
        };
        painter.rect_filled(header_rect, 6.0, header_bg);
        // Clean square bottom for the header
        painter.rect_filled(
            Rect::from_min_max(pos2(header_rect.min.x, header_rect.max.y - 4.0), header_rect.max),
            0.0,
            header_bg,
        );

        // Node number badge (e.g. "01", "02")
        let num_str = format!("{:02}", i + 1);
        painter.text(
            pos2(header_rect.min.x + 8.0, header_rect.center().y),
            Align2::LEFT_CENTER,
            num_str,
            Tokens::ui(11.0),
            if is_active { Color32::WHITE } else { Color32::from_rgb(180, 185, 195) },
        );

        // Power bypass toggle indicator (Resolve LED circle)
        let power_center = pos2(header_rect.max.x - 12.0, header_rect.center().y);
        let power_r = Rect::from_center_size(power_center, vec2(16.0, 16.0));
        let power_resp = ui.interact(power_r, egui::Id::new(("node_power", clip.0, i)), Sense::click());
        app.auto.add(&format!("color_nodes.node.{i}.power"), power_r, "Toggle Node Bypass");

        let led_color = if is_enabled {
            Color32::from_rgb(70, 210, 90) // Vibrant green
        } else {
            Color32::from_rgb(150, 45, 45) // Dim red
        };
        painter.circle_filled(power_center, 4.5, led_color);

        if !is_enabled {
            // Draw a tiny strike line across the LED
            painter.line_segment(
                [pos2(power_center.x - 4.0, power_center.y - 4.0), pos2(power_center.x + 4.0, power_center.y + 4.0)],
                Stroke::new(1.5, Color32::WHITE),
            );
        }

        if power_resp.clicked() {
            let _ = app.session.execute("effects.toggleEnabled", json!({"clip": clip.0, "index": fx_idx}));
        }

        // Custom Node Label (e.g. "Primary", "Curves", "Skin", "Look")
        let label_key = format!("{}:{i}", clip.0);
        let custom_label = app.ui.color_node_labels.get(&label_key).cloned();
        let display_label = custom_label.unwrap_or_else(|| {
            if i == 0 {
                "Primary".to_string()
            } else {
                format!("Node {:02}", i + 1)
            }
        });

        // Clickable label to rename
        let label_r = Rect::from_min_size(pos2(node_rect.min.x + 8.0, header_rect.max.y + 6.0), vec2(node_rect.width() - 16.0, 16.0));
        painter.text(
            label_r.min,
            Align2::LEFT_TOP,
            display_label,
            Tokens::ui(11.0),
            if is_enabled { t.text } else { t.text_faint },
        );

        // Badges indicating active modules on this node
        let badges = detect_node_badges(fx_inst);
        let mut bx = node_rect.min.x + 8.0;
        let by = node_rect.max.y - 18.0;

        for (badge_text, badge_tip) in badges {
            let badge_w = 26.0;
            let badge_h = 13.0;
            let br = Rect::from_min_size(pos2(bx, by), vec2(badge_w, badge_h));
            let b_bg = if is_enabled {
                Color32::from_rgb(20, 22, 28)
            } else {
                Color32::from_rgb(18, 19, 22)
            };
            painter.rect_filled(br, 2.0, b_bg);
            painter.rect_stroke(
                br,
                2.0,
                Stroke::new(1.0, if is_active { Color32::from_rgb(70, 75, 90) } else { Color32::from_rgb(45, 48, 56) }),
                StrokeKind::Inside,
            );

            let txt_col = match badge_text {
                "PRI" => Color32::from_rgb(230, 180, 50),
                "LUT" => Color32::from_rgb(70, 180, 240),
                "CRV" => Color32::from_rgb(180, 110, 240),
                "WHL" => Color32::from_rgb(240, 120, 90),
                "HSL" => Color32::from_rgb(80, 210, 160),
                "VIG" => Color32::from_rgb(210, 160, 210),
                _ => Color32::from_rgb(140, 150, 165),
            };
            painter.text(br.center(), Align2::CENTER_CENTER, badge_text, Tokens::ui(9.0), txt_col);

            let badge_resp = ui.interact(br, egui::Id::new(("badge", clip.0, i, badge_text)), Sense::hover());
            badge_resp.on_hover_text(badge_tip);

            bx += badge_w + 4.0;
            if bx + badge_w > node_rect.max.x - 6.0 {
                break;
            }
        }

        // Draw Pins on card edges
        let pin_in = pos2(node_rect.min.x, center_y);
        let pin_out = pos2(node_rect.max.x, center_y);
        let pin_col = if is_enabled {
            Color32::from_rgb(60, 200, 80)
        } else {
            Color32::from_rgb(90, 95, 105)
        };
        painter.circle_filled(pin_in, 4.5, pin_col);
        painter.circle_filled(pin_out, 4.5, pin_col);

        // Node card border stroke (DaVinci Resolve signature gold/amber selection)
        let stroke = if is_active {
            Stroke::new(2.0, Color32::from_rgb(230, 175, 45)) // DaVinci gold active border
        } else if card_resp.hovered() {
            Stroke::new(1.0, Color32::from_rgb(90, 95, 110))
        } else {
            Stroke::new(1.0, Color32::from_rgb(48, 52, 62))
        };
        painter.rect_stroke(node_rect, 6.0, stroke, StrokeKind::Inside);

        // Context menu on right click
        card_resp.context_menu(|ui| {
            if ui.button(tl!("Add Serial Node (Alt+S)")).clicked() {
                let _ = app.session.execute("effects.apply", json!({"clips": [clip.0], "effect": "lumetri"}));
                app.ui.active_color_node = node_count;
                ui.close();
            }
            let toggle_label = if is_enabled {
                tl!("Bypass Node (Ctrl+D)")
            } else {
                tl!("Enable Node (Ctrl+D)")
            };
            if ui.button(toggle_label).clicked() {
                let _ = app.session.execute("effects.toggleEnabled", json!({"clip": clip.0, "index": fx_idx}));
                ui.close();
            }
            if ui.button(tl!("Reset Node")).clicked() {
                let _ = app.session.execute("effects.reset", json!({"clip": clip.0, "index": fx_idx}));
                ui.close();
            }
            if node_count > 1 && ui.button(tl!("Delete Node")).clicked() {
                let _ = app.session.execute("effects.remove", json!({"clip": clip.0, "index": fx_idx}));
                if active_node >= node_count - 1 {
                    app.ui.active_color_node = active_node.saturating_sub(1);
                }
                ui.close();
            }
        });
    }
}
