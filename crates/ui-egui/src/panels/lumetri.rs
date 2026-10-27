//! Lumetri Color panel: Basic Correction, Creative, Curves (RGB + hue curves), Color Wheels,
//! HSL Secondary, Vignette — editing the selected clip's Lumetri effect through engine commands.
//!
//! In an HDR sequence (Rec. 2100 PQ / HLG working space) Basic Correction shows HDR White and
//! HDR Specular and Curves shows HDR Range (cd/m²): the sliders and curves then span 0 … that
//! many nits (see `filmcraft_color::grade`). Automation ids for those and HSL Secondary ▸ Refine:
//! `lumetri.param.<id>` (`hdr_white`, `hdr_specular`, `curves_hdr_range`, `hsl_denoise`,
//! `hsl_blur`); Color Match: `lumetri.match.comparisonView` (toggles the Program monitor's
//! Comparison View, whose reference frame Apply Match then uses).

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use filmcraft_project::{ClipId, EffectInstance, ParamValue};
use filmcraft_time::Tick;
use serde_json::{Value, json};

use crate::FilmcraftApp;
#[allow(unused_imports)]
use crate::icons::{self, Icon};
use crate::native_dialogs::FileDialog;
use crate::theme::Tokens;

type Actions = Vec<Value>;

pub fn active_color_clip(app: &FilmcraftApp) -> Option<(ClipId, filmcraft_project::TrackItem)> {
    let seq = app.session.active_sequence()?;
    if let Some(res) = app.session.state.selection.iter().find_map(|c| {
        let (tid, it) = seq.find_item(*c)?;
        (seq.track(tid)?.kind == filmcraft_project::TrackKind::Video).then(|| (*c, it.clone()))
    }) {
        return Some(res);
    }
    let ph = app.session.playhead();
    for track in seq.video_tracks.iter().rev() {
        if !track.locked && !track.muted {
            for item in &track.items {
                if item.start <= ph && ph < item.end() {
                    return Some((item.id, item.clone()));
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy)]
pub struct LumetriToolDef {
    pub id: &'static str,
    pub icon: &'static str,
    pub label: &'static str,
    pub tooltip: &'static str,
    pub param: &'static str,
    pub sec: &'static str,
}

pub const ALL_TOOLS: [LumetriToolDef; 7] = [
    LumetriToolDef {
        id: "Basic Correction",
        icon: "⬡",
        label: "Primarias",
        tooltip: "Ajustes primarios y perillas",
        param: "basic_on",
        sec: "basic",
    },
    LumetriToolDef {
        id: "Color Wheels & Match",
        icon: "◎",
        label: "Ruedas & Match",
        tooltip: "Ruedas de color Lift/Gamma/Gain y Color Match",
        param: "wheels_on",
        sec: "wheels",
    },
    LumetriToolDef {
        id: "Curves",
        icon: "∿",
        label: "Curvas",
        tooltip: "Curvas RGB y curvas de matiz/saturación",
        param: "curves_on",
        sec: "curves",
    },
    LumetriToolDef {
        id: "Power Windows / Máscara",
        icon: "⬚",
        label: "Máscara",
        tooltip: "Power Windows y máscaras",
        param: "masks_on",
        sec: "masks",
    },
    LumetriToolDef {
        id: "HSL Secondary",
        icon: "✑",
        label: "Cualificador",
        tooltip: "Cualificador HSL secundario",
        param: "hsl_on",
        sec: "hsl",
    },
    LumetriToolDef {
        id: "Creative",
        icon: "🎞",
        label: "Creative & LUTs",
        tooltip: "Looks, LUTs creativos e intensidad",
        param: "creative_on",
        sec: "creative",
    },
    LumetriToolDef {
        id: "Vignette",
        icon: "◐",
        label: "Viñeta",
        tooltip: "Ajustes de viñeta",
        param: "vignette_on",
        sec: "vignette",
    },
];

impl LumetriToolDef {
    pub fn panel_kind(&self) -> crate::dock::PanelKind {
        match self.id {
            "Basic Correction" => crate::dock::PanelKind::ColorPrimaries,
            "Color Wheels & Match" => crate::dock::PanelKind::ColorWheels,
            "Curves" => crate::dock::PanelKind::ColorCurves,
            "Power Windows / Máscara" => crate::dock::PanelKind::ColorMasks,
            "HSL Secondary" => crate::dock::PanelKind::ColorQualifier,
            "Creative" => crate::dock::PanelKind::ColorCreative,
            "Vignette" => crate::dock::PanelKind::ColorVignette,
            _ => crate::dock::PanelKind::LumetriColor,
        }
    }
}

pub fn tool_for_panel(panel: crate::dock::PanelKind) -> Option<&'static str> {
    match panel {
        crate::dock::PanelKind::ColorPrimaries => Some("Basic Correction"),
        crate::dock::PanelKind::ColorWheels => Some("Color Wheels & Match"),
        crate::dock::PanelKind::ColorCurves => Some("Curves"),
        crate::dock::PanelKind::ColorMasks => Some("Power Windows / Máscara"),
        crate::dock::PanelKind::ColorQualifier => Some("HSL Secondary"),
        crate::dock::PanelKind::ColorCreative => Some("Creative"),
        crate::dock::PanelKind::ColorVignette => Some("Vignette"),
        _ => None,
    }
}

pub fn draw_tool_icon(painter: &egui::Painter, center: Pos2, tool_id: &str, color: Color32) {
    match tool_id {
        "Basic Correction" => {
            // Three mini fader sliders with knobs
            for col in 0..3 {
                let x = center.x + (col as f32 - 1.0) * 4.5;
                painter.line_segment([pos2(x, center.y - 5.5), pos2(x, center.y + 5.5)], Stroke::new(1.0, color.gamma_multiply(0.5)));
                let pip_y = center.y + match col { 0 => -1.5, 1 => 2.0, _ => -0.5 };
                painter.circle_filled(pos2(x, pip_y), 1.6, color);
            }
        }
        "Color Wheels & Match" => {
            // Color wheel circle with central reticle
            painter.circle_stroke(center, 5.5, Stroke::new(1.2, color));
            painter.circle_filled(center, 1.8, color);
        }
        "Curves" => {
            // Smooth tone curve
            let p0 = pos2(center.x - 5.5, center.y + 4.5);
            let p1 = pos2(center.x - 2.0, center.y + 2.5);
            let p2 = pos2(center.x + 2.0, center.y - 2.5);
            let p3 = pos2(center.x + 5.5, center.y - 4.5);
            painter.line_segment([p0, p1], Stroke::new(1.3, color));
            painter.line_segment([p1, p2], Stroke::new(1.3, color));
            painter.line_segment([p2, p3], Stroke::new(1.3, color));
        }
        "Power Windows / Máscara" => {
            // Mask / Power Window rectangle
            let m_rect = Rect::from_center_size(center, vec2(11.0, 10.0));
            painter.rect_stroke(m_rect, 2.0, Stroke::new(1.2, color), StrokeKind::Inside);
        }
        "HSL Secondary" => {
            // Eyedropper / Qualifier tool
            let tip = pos2(center.x - 4.5, center.y + 4.5);
            let top = pos2(center.x + 4.5, center.y - 4.5);
            painter.line_segment([tip, top], Stroke::new(1.5, color));
            painter.circle_filled(tip, 1.3, color);
            painter.circle_filled(top, 2.0, color);
        }
        "Creative" => {
            // Film frame with sprockets
            let f_rect = Rect::from_center_size(center, vec2(12.0, 10.0));
            painter.rect_stroke(f_rect, 1.5, Stroke::new(1.0, color), StrokeKind::Inside);
            painter.line_segment([pos2(f_rect.min.x + 3.0, f_rect.min.y), pos2(f_rect.min.x + 3.0, f_rect.max.y)], Stroke::new(0.8, color.gamma_multiply(0.5)));
            painter.line_segment([pos2(f_rect.max.x - 3.0, f_rect.min.y), pos2(f_rect.max.x - 3.0, f_rect.max.y)], Stroke::new(0.8, color.gamma_multiply(0.5)));
        }
        "Vignette" => {
            // Vignette aperture lens
            painter.circle_stroke(center, 5.5, Stroke::new(1.0, color.gamma_multiply(0.6)));
            painter.circle_filled(center, 3.2, color);
        }
        _ => {
            painter.circle_filled(center, 3.0, color);
        }
    }
}

pub fn get_tool_def(id: &str) -> Option<LumetriToolDef> {
    ALL_TOOLS.iter().find(|t| t.id == id).copied()
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect, current_panel: crate::dock::PanelKind) {
    let t = app.tokens;
    let Some((clip, it)) = active_color_clip(app) else {
        crate::dock::placeholder(ui, rect, &t, tl!("(no clip selected)"));
        return;
    };
    let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 6.0))).id_salt(("lumetri2", current_panel)));
    let lumetri_effects: Vec<(usize, &EffectInstance)> = it.effects.iter().enumerate().filter(|(_, e)| e.effect == "lumetri").collect();
    if lumetri_effects.is_empty() {
        bui.add_space(8.0);
        bui.label(egui::RichText::new(tlf!("Master · {name}", name = it.name)).color(t.text_dim));
        bui.add_space(8.0);
        if bui.button(tl!("Add Lumetri Color to clip")).clicked() {
            let _ = app.session.execute("effects.apply", json!({"clips": [clip.0], "effect": "lumetri"}));
            app.ui.active_color_node = 0;
        }
        return;
    }
    let active_node = app.ui.active_color_node.min(lumetri_effects.len().saturating_sub(1));
    app.ui.active_color_node = active_node;
    let (idx, e_ref) = lumetri_effects[active_node];
    let e = e_ref.clone();
    // HDR mode: the sequence's working space is Rec. 2100 PQ or HLG
    let hdr = app.session.active_sequence().is_some_and(|q| q.settings.color.working.is_hdr());
    let mut auto_rows: Vec<(String, Rect, &'static str)> = Vec::new();
    let ph = app.session.playhead();
    let mt = it.source_time_at(ph.clamp(it.start, it.end() - Tick(1)));
    let mut actions: Actions = Vec::new();
    // header: clip context + fx toggle
    // the fx toggle is laid out first so a long clip name is truncated instead of widening the panel
    bui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let fx = ui.add(egui::Button::new(egui::RichText::new("fx").italics().color(if e.enabled { t.text } else { t.text_faint })).frame(false));
            if fx.clicked() {
                let _ = app.session.execute("effects.toggleEnabled", json!({"clip": clip.0, "index": idx}));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(tlf!("Master · {name}", name = it.name)).color(t.text_dim)).truncate());
            });
        });
    });

    // DaVinci Resolve-style Node switcher bar: [ Node: ] [ + ] | [ Node 01 ✓ ] [ Node 02 ] ...
    bui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Node:")).color(t.text_dim).small());
        let add_btn = ui.button(egui::RichText::new("+").small()).on_hover_text(tl!("Add Serial Node (Alt+S)"));
        app.auto.add("lumetri.node.add", add_btn.rect, "Add Serial Node");
        if add_btn.clicked() {
            let _ = app.session.execute("effects.apply", json!({"clips": [clip.0], "effect": "lumetri"}));
            app.ui.active_color_node = lumetri_effects.len();
        }
        egui::ScrollArea::horizontal().auto_shrink([false, true]).max_height(26.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (n_idx, &(_fx_i, fx_inst)) in lumetri_effects.iter().enumerate() {
                    let is_curr = n_idx == active_node;
                    let node_num = format!("{:02}", n_idx + 1);
                    let custom_name = app.ui.color_node_labels.get(&format!("{}:{n_idx}", clip.0)).cloned();
                    let label = custom_name.unwrap_or_else(|| format!("Node {node_num}"));
                    let text = if fx_inst.enabled {
                        format!("{label} ✓")
                    } else {
                        format!("{label} ✗")
                    };
                    let mut btn = egui::Button::new(egui::RichText::new(text).small());
                    if is_curr {
                        btn = btn.fill(t.accent.gamma_multiply(0.25)).stroke(egui::Stroke::new(1.0, t.accent));
                    }
                    let resp = ui.add(btn);
                    app.auto.add(&format!("lumetri.node.{n_idx}"), resp.rect, &label);
                    if resp.clicked() {
                        app.ui.active_color_node = n_idx;
                    }
                }
            });
        });
    });

    let mut sections: Vec<&'static str> = Vec::new();
    let mut lut_actions: Vec<(&'static str, Value)> = Vec::new();
    let mut mask_actions: Vec<(String, Value)> = Vec::new();

    let bar_pos = app.ui.lumetri_bar_pos.clone();
    match bar_pos.as_str() {
        "bottom" => {
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("lumetri_scroll", current_panel)).show(&mut bui, |ui| {
                render_palette_content(ui, app, &e, current_panel, clip, idx, mt, hdr, &mut actions, &mut lut_actions, &mut mask_actions, &mut auto_rows, &mut sections);
                ui.add_space(20.0);
            });
            bui.add_space(4.0);
            render_palette_toolbar(&mut bui, app, &e, current_panel, false, &mut sections);
        }
        "left" => {
            bui.horizontal(|ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("lumetri_side_scroll", current_panel)).show(ui, |ui| {
                    render_palette_toolbar(ui, app, &e, current_panel, true, &mut sections);
                });
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("lumetri_scroll", current_panel)).show(ui, |ui| {
                    render_palette_content(ui, app, &e, current_panel, clip, idx, mt, hdr, &mut actions, &mut lut_actions, &mut mask_actions, &mut auto_rows, &mut sections);
                    ui.add_space(20.0);
                });
            });
        }
        _ => {
            // default: "top"
            render_palette_toolbar(&mut bui, app, &e, current_panel, false, &mut sections);
            bui.add_space(4.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("lumetri_scroll", current_panel)).show(&mut bui, |ui| {
                render_palette_content(ui, app, &e, current_panel, clip, idx, mt, hdr, &mut actions, &mut lut_actions, &mut mask_actions, &mut auto_rows, &mut sections);
                ui.add_space(20.0);
            });
        }
    }
    for (id, r, label) in auto_rows {
        if r.is_positive() {
            app.auto.add(&id, r, label);
        }
    }
    // a dragged value is one undo step, as in Effect Controls
    crate::panels::effect_controls::run(app, ui.ctx(), actions.into_iter().map(|a| ("effects.setParam".to_string(), a)).collect());
    for sec in sections {
        if let Err(err) = app.session.execute("lumetri.setSection", json!({"clip": clip.0, "section": sec})) {
            app.ui.status = err.to_string();
        }
    }
    for (cmd, mut p) in lut_actions {
        if cmd == "view.display" {
            // Comparison View on / off (a Program monitor display mode)
            let d = p["display"].as_str().unwrap_or("composite").to_string();
            if let Some(Err(err)) = crate::panels::monitor_view::route(app, &format!("view.display.{d}"), &json!({"monitor": "program"})) {
                app.ui.status = err;
            }
            continue;
        }
        if p.get("browse").is_some() {
            let (cmd, clip) = (cmd.to_string(), clip.0);
            app.pick_ui(FileDialog::open_file(tl!("LUT"), &["cube", "3dl"]), move |app, paths| {
                let Some(path) = paths.into_iter().next() else { return };
                if let Err(err) = app.session.execute(&cmd, json!({"path": path, "clip": clip})) {
                    app.ui.status = err.to_string();
                }
            });
            continue;
        }
        p["clip"] = json!(clip.0);
        if let Err(err) = app.session.execute(cmd, p) {
            app.ui.status = err.to_string();
        }
    }
    for (cmd, val) in mask_actions {
        if let Err(err) = app.session.execute(&cmd, val) {
            app.ui.status = err.to_string();
        }
    }
}

fn render_tool_buttons(
    ui: &mut egui::Ui,
    app: &mut FilmcraftApp,
    e: &EffectInstance,
    current_panel: crate::dock::PanelKind,
    vertical: bool,
    sections: &mut Vec<&'static str>,
) {
    let t = app.tokens;
    let num_tools = app.ui.lumetri_tool_order.len();
    let current_tool_id = tool_for_panel(current_panel)
        .map(String::from)
        .unwrap_or_else(|| app.ui.lumetri_active_tool.clone());

    for i in 0..num_tools {
        let tool_id = app.ui.lumetri_tool_order[i].clone();
        let Some(def) = get_tool_def(&tool_id) else { continue };

        let key = format!("lumetri2:{}", def.id);
        let is_open = !app.ui.collapsed_fx.contains(&key) && (def.id == "Basic Correction" || app.ui.expanded_fx.contains(&key));
        let is_primary = if current_panel == crate::dock::PanelKind::LumetriColor {
            is_open || def.id == current_tool_id
        } else {
            def.id == current_tool_id
        };
        let is_secondary = app.ui.lumetri_dual_view && app.ui.lumetri_secondary_tool == def.id;
        let is_docked_elsewhere = def.panel_kind() != current_panel
            && (app.ui.dock.contains(def.panel_kind()) || app.ui.floating.iter().any(|g| g.panels.contains(&def.panel_kind())));
        let is_on = e.param(def.param).and_then(|p| p.value.as_bool()).unwrap_or(def.param != "hsl_on");

        let btn_size = if vertical { vec2(148.0, 26.0) } else { vec2(132.0, 26.0) };
        let (r, resp) = ui.allocate_exact_size(btn_size, Sense::click_and_drag());

        // Drag to dock / float window anywhere across screen
        if resp.drag_started() {
            let target_panel = def.panel_kind();
            app.begin_panel_drag(ui.ctx(), target_panel, current_panel);
        }

        // Draw button background & border
        let bg_color = if is_primary {
            t.accent.gamma_multiply(0.24)
        } else if is_secondary {
            t.accent.gamma_multiply(0.12)
        } else if resp.hovered() {
            Color32::from_rgb(38, 42, 52)
        } else {
            Color32::from_rgb(26, 29, 36)
        };

        let border_stroke = if is_primary {
            Stroke::new(1.5, t.accent)
        } else if is_secondary {
            Stroke::new(1.0, Color32::from_rgb(160, 175, 200))
        } else if is_docked_elsewhere {
            Stroke::new(1.0, Color32::from_rgb(60, 110, 160))
        } else if resp.hovered() {
            Stroke::new(1.0, Color32::from_rgb(70, 75, 90))
        } else {
            Stroke::new(1.0, Color32::from_rgb(42, 46, 56))
        };

        ui.painter().rect(r, 4.0, bg_color, border_stroke, StrokeKind::Inside);

        // 1. Clean resolution-independent 6-dot drag grip handle (no missing glyph □□)
        let dot_col = if resp.hovered() || resp.dragged() {
            Color32::from_rgb(180, 190, 210)
        } else {
            Color32::from_rgb(75, 80, 95)
        };
        let gx = r.min.x + 7.0;
        let gy = r.center().y;
        for col in 0..2 {
            for row in 0..3 {
                let px = gx + col as f32 * 3.5;
                let py = gy + (row as f32 - 1.0) * 3.5;
                ui.painter().circle_filled(pos2(px, py), 1.0, dot_col);
            }
        }

        // 2. Crisp vector icon drawn cleanly (no missing glyph □)
        let text_color = if is_primary {
            Color32::WHITE
        } else if resp.hovered() {
            t.text
        } else {
            t.text_dim
        };
        draw_tool_icon(ui.painter(), pos2(r.min.x + 22.0, r.center().y), def.id, text_color);

        // 3. Translated tool label
        let label_text = crate::i18n::t(def.label);
        ui.painter().text(
            pos2(r.min.x + 32.0, r.center().y),
            Align2::LEFT_CENTER,
            label_text,
            FontId::proportional(11.0),
            text_color,
        );

        // 4. Switch / Bypass indicator dot on the right of the button
        let sw_center = pos2(r.max.x - 11.0, r.center().y);
        let sw_rect = Rect::from_center_size(sw_center, vec2(14.0, 14.0));
        let sw_resp = ui.interact(sw_rect.expand(2.0), ui.id().with("sw_btn").with(i), Sense::click());

        let dot_color = if is_on {
            Color32::from_rgb(52, 211, 153)
        } else {
            Color32::from_rgb(65, 70, 82)
        };
        ui.painter().circle_filled(sw_center, 4.0, dot_color);
        if is_on {
            ui.painter().circle_stroke(sw_center, 5.5, Stroke::new(1.0, Color32::from_rgb(52, 211, 153).gamma_multiply(0.4)));
        }
        if sw_resp.clicked() {
            sections.push(def.sec);
        }

        // 5. Click button to switch this window to the selected tool
        if resp.clicked() && !sw_resp.clicked() {
            if current_panel == crate::dock::PanelKind::LumetriColor {
                if is_open {
                    app.ui.collapsed_fx.push(key.clone());
                    app.ui.expanded_fx.retain(|k| k != &key);
                } else {
                    app.ui.collapsed_fx.retain(|k| k != &key);
                    app.ui.expanded_fx.push(key.clone());
                }
                app.ui.lumetri_active_tool = def.id.to_string();
            } else {
                app.switch_color_window(current_panel, def.panel_kind(), def.id);
            }
        }

        // Hover tooltip explaining both click and drag
        let hover_msg = format!(
            "{}\n• Clic: Cambiar esta ventana a {}\n• Arrastrar: Ubicar / acoplar ventana en cualquier lugar de la pantalla",
            def.tooltip,
            crate::i18n::t(def.label)
        );
        resp.on_hover_text(hover_msg);

        // Automation registers
        app.auto.add(&format!("lumetri.section.{}", def.id), r, def.id);
        app.auto.add(&format!("lumetri.switch.{}", def.id), sw_rect.expand(4.0), &format!("{} on/off", def.id));
        if def.id.contains("Power Windows") {
            app.auto.add("lumetri.section.Power Windows", r, "Power Windows");
            app.auto.add("lumetri.section.Masks", r, "Masks");
            app.auto.add("lumetri.switch.Power Windows", sw_rect.expand(4.0), "Power Windows on/off");
            app.auto.add("lumetri.switch.Masks", sw_rect.expand(4.0), "Masks on/off");
        }
    }
}

fn render_palette_toolbar(
    ui: &mut egui::Ui,
    app: &mut FilmcraftApp,
    e: &EffectInstance,
    current_panel: crate::dock::PanelKind,
    vertical: bool,
    sections: &mut Vec<&'static str>,
) {
    for def in &ALL_TOOLS {
        if !app.ui.lumetri_tool_order.iter().any(|id| id == def.id) {
            app.ui.lumetri_tool_order.push(def.id.to_string());
        }
    }
    app.ui.lumetri_tool_order.retain(|id| ALL_TOOLS.iter().any(|def| def.id == id));

    if vertical {
        ui.vertical(|ui| {
            render_tool_buttons(ui, app, e, current_panel, true, sections);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let cur_pos = app.ui.lumetri_bar_pos.clone();
                if ui.selectable_label(cur_pos == "top", "⬒").on_hover_text(tl!("Barra Arriba")).clicked() {
                    app.ui.lumetri_bar_pos = "top".into();
                }
                if ui.selectable_label(cur_pos == "bottom", "⬓").on_hover_text(tl!("Barra Abajo")).clicked() {
                    app.ui.lumetri_bar_pos = "bottom".into();
                }
                if ui.selectable_label(cur_pos == "left", "◧").on_hover_text(tl!("Barra Lateral Izquierda")).clicked() {
                    app.ui.lumetri_bar_pos = "left".into();
                }
                let dual_text = if app.ui.lumetri_dual_view { "◫ ✓" } else { "◫" };
                if ui.selectable_label(app.ui.lumetri_dual_view, dual_text).on_hover_text(tl!("Vista dual (dos herramientas)")).clicked() {
                    app.ui.lumetri_dual_view = !app.ui.lumetri_dual_view;
                }
            });
        });
    } else {
        ui.horizontal(|ui| {
            egui::ScrollArea::horizontal().auto_shrink([false, false]).id_salt(("tool_strip", current_panel)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    render_tool_buttons(ui, app, e, current_panel, false, sections);
                });
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            let cur_pos = app.ui.lumetri_bar_pos.clone();
            if ui.selectable_label(cur_pos == "top", "⬒").on_hover_text(tl!("Barra Arriba")).clicked() {
                app.ui.lumetri_bar_pos = "top".into();
            }
            if ui.selectable_label(cur_pos == "bottom", "⬓").on_hover_text(tl!("Barra Abajo")).clicked() {
                app.ui.lumetri_bar_pos = "bottom".into();
            }
            if ui.selectable_label(cur_pos == "left", "◧").on_hover_text(tl!("Barra Lateral Izquierda")).clicked() {
                app.ui.lumetri_bar_pos = "left".into();
            }
            let dual_text = if app.ui.lumetri_dual_view { "◫ Dual ✓" } else { "◫ Dual" };
            if ui.selectable_label(app.ui.lumetri_dual_view, dual_text).on_hover_text(tl!("Vista dual (dos herramientas)")).clicked() {
                app.ui.lumetri_dual_view = !app.ui.lumetri_dual_view;
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn render_palette_content(
    ui: &mut egui::Ui,
    app: &mut FilmcraftApp,
    e: &EffectInstance,
    current_panel: crate::dock::PanelKind,
    clip: ClipId,
    idx: usize,
    mt: Tick,
    hdr: bool,
    actions: &mut Actions,
    lut_actions: &mut Vec<(&'static str, Value)>,
    mask_actions: &mut Vec<(String, Value)>,
    auto_rows: &mut Vec<(String, Rect, &'static str)>,
    sections: &mut Vec<&'static str>,
) {
    let tools_to_render = if let Some(forced) = tool_for_panel(current_panel) {
        vec![forced.to_string()]
    } else {
        let open_tools: Vec<String> = app.ui.lumetri_tool_order.iter()
            .filter(|id| {
                let key = format!("lumetri2:{id}");
                !app.ui.collapsed_fx.contains(&key) && (*id == "Basic Correction" || app.ui.expanded_fx.contains(&key))
            })
            .cloned()
            .collect();
        if open_tools.is_empty() {
            vec![app.ui.lumetri_active_tool.clone()]
        } else {
            open_tools
        }
    };

    if app.ui.lumetri_dual_view && tools_to_render.len() >= 2 {
        ui.columns(2, |cols| {
            cols[0].vertical(|ui| {
                render_single_palette(&tools_to_render[0], ui, app, e, clip, idx, mt, hdr, actions, lut_actions, mask_actions, auto_rows, sections);
            });
            cols[1].vertical(|ui| {
                render_single_palette(&tools_to_render[1], ui, app, e, clip, idx, mt, hdr, actions, lut_actions, mask_actions, auto_rows, sections);
            });
        });
        for tool_id in &tools_to_render[2..] {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            render_single_palette(tool_id, ui, app, e, clip, idx, mt, hdr, actions, lut_actions, mask_actions, auto_rows, sections);
        }
    } else {
        for (idx_t, tool_id) in tools_to_render.iter().enumerate() {
            if idx_t > 0 {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
            }
            render_single_palette(tool_id, ui, app, e, clip, idx, mt, hdr, actions, lut_actions, mask_actions, auto_rows, sections);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_single_palette(
    tool_id: &str,
    ui: &mut egui::Ui,
    app: &mut FilmcraftApp,
    e: &EffectInstance,
    clip: ClipId,
    idx: usize,
    mt: Tick,
    hdr: bool,
    actions: &mut Actions,
    lut_actions: &mut Vec<(&'static str, Value)>,
    mask_actions: &mut Vec<(String, Value)>,
    auto_rows: &mut Vec<(String, Rect, &'static str)>,
    sections: &mut Vec<&'static str>,
) {
    let t = app.tokens;
    let Some(def) = get_tool_def(tool_id) else { return };
    let set = |actions: &mut Actions, param: &str, v: Value| actions.push(json!({"clip": clip.0, "effect": idx, "param": param, "value": v}));

    // Sleek Tool Header: [ Icon Tool Name ] [ Switch Bypass ]
    let on = e.param(def.param).and_then(|p| p.value.as_bool()).unwrap_or(def.param != "hsl_on");
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{} {}", def.icon, crate::i18n::t(def.id))).strong().size(13.0).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (sw, sw_resp) = ui.allocate_exact_size(vec2(26.0, 14.0), Sense::click());
            ui.painter().rect_filled(sw, 7.0, if on { t.accent } else { Color32::from_rgb(0x55, 0x55, 0x55) });
            ui.painter().circle_filled(pos2(if on { sw.max.x - 7.0 } else { sw.min.x + 7.0 }, sw.center().y), 5.0, Color32::WHITE);
            if sw_resp.clicked() {
                sections.push(def.sec);
            }
            app.auto.add(&format!("lumetri.switch.{}", def.id), sw.expand(3.0), &format!("{} on/off", def.id));
            if def.id.contains("Power Windows") {
                app.auto.add("lumetri.switch.Power Windows", sw.expand(3.0), "Power Windows on/off");
                app.auto.add("lumetri.switch.Masks", sw.expand(3.0), "Masks on/off");
            }
        });
    });
    ui.add_space(4.0);

    match def.id {
        "Basic Correction" => {
            lut_combo(ui, app, e, "input_lut", tl!("Input LUT"), true, lut_actions);
            let primaries_mode_id = egui::Id::new("lumetri_primaries_mode");
            let show_sliders = ui.data(|d| d.get_temp::<bool>(primaries_mode_id).unwrap_or(false));

            ui.horizontal(|ui| {
                sub(ui, &t, tl!("Ajustes Primarios"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.selectable_label(show_sliders, tl!("Sliders")).clicked() {
                        ui.data_mut(|d| d.insert_temp(primaries_mode_id, true));
                    }
                    if ui.selectable_label(!show_sliders, tl!("Perillas")).clicked() {
                        ui.data_mut(|d| d.insert_temp(primaries_mode_id, false));
                    }
                });
            });

            if !show_sliders {
                egui::ScrollArea::horizontal().auto_shrink([false, true]).max_height(78.0).id_salt("primary_knobs").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let r = rotary_knob(ui, e, mt, "temperature", tl!("Temperature"), "TEMP", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(255, 140, 50)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.temperature".into(), r, "Temperature"));

                        let r = rotary_knob(ui, e, mt, "tint", tl!("Tint"), "TINT", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(220, 80, 200)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.tint".into(), r, "Tint"));

                        let r = rotary_knob(ui, e, mt, "exposure", tl!("Exposure"), "EXP", (-5.0, 5.0), 0.0, 0.02, 1, " EV", Some(Color32::from_rgb(240, 220, 100)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.exposure".into(), r, "Exposure"));

                        let r = rotary_knob(ui, e, mt, "contrast", tl!("Contrast"), "CONT", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(180, 190, 210)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.contrast".into(), r, "Contrast"));

                        let r = rotary_knob(ui, e, mt, "highlights", tl!("Highlights"), "HI", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(245, 245, 245)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.highlights".into(), r, "Highlights"));

                        let r = rotary_knob(ui, e, mt, "shadows", tl!("Shadows"), "SHAD", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(90, 100, 120)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.shadows".into(), r, "Shadows"));

                        let r = rotary_knob(ui, e, mt, "whites", tl!("Whites"), "WHITE", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(255, 255, 255)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.whites".into(), r, "Whites"));

                        let r = rotary_knob(ui, e, mt, "blacks", tl!("Blacks"), "BLACK", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(70, 75, 85)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.blacks".into(), r, "Blacks"));

                        let r = rotary_knob(ui, e, mt, "saturation", tl!("Saturation"), "SAT", (0.0, 200.0), 100.0, 0.4, 0, "%", Some(Color32::from_rgb(80, 200, 120)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.saturation".into(), r, "Saturation"));

                        let r = rotary_knob(ui, e, mt, "vibrance", tl!("Vibrance"), "VIB", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(100, 170, 240)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.vibrance".into(), r, "Vibrance"));

                        let r = rotary_knob(ui, e, mt, "sharpen", tl!("Sharpen"), "SHRP", (-100.0, 100.0), 0.0, 0.4, 0, "", Some(Color32::from_rgb(220, 180, 100)), &t, actions, clip, idx);
                        auto_rows.push(("lumetri.knob.sharpen".into(), r, "Sharpen"));
                    });
                });
            } else {
                sub(ui, &t, tl!("Color"));
                slider(ui, e, mt, "temperature", tl!("Temperature"), Some(Gradient::Temp), &t, actions, clip, idx);
                slider(ui, e, mt, "tint", tl!("Tint"), Some(Gradient::Tint), &t, actions, clip, idx);
                slider(ui, e, mt, "saturation", tl!("Saturation"), None, &t, actions, clip, idx);
                sub(ui, &t, tl!("Light"));
                for (id, label) in [
                    ("exposure", tl!("Exposure")),
                    ("contrast", tl!("Contrast")),
                    ("highlights", tl!("Highlights")),
                    ("shadows", tl!("Shadows")),
                    ("whites", tl!("Whites")),
                    ("blacks", tl!("Blacks")),
                ] {
                    slider(ui, e, mt, id, label, None, &t, actions, clip, idx);
                }
            }

            if hdr {
                sub(ui, &t, tl!("HDR"));
                for (id, label) in [("hdr_white", tl!("HDR White")), ("hdr_specular", tl!("HDR Specular"))] {
                    let r = slider(ui, e, mt, id, label, None, &t, actions, clip, idx);
                    auto_rows.push((format!("lumetri.param.{id}"), r, label));
                }
            }
        }
        "Creative" => {
            let look = match e.param("look").map(|p| &p.value) {
                Some(ParamValue::Choice(c)) => *c as usize,
                _ => 0,
            };
            let opts = filmcraft_project::find_effect("lumetri")
                .and_then(|d| d.param("look"))
                .and_then(|p| if let filmcraft_project::ParamKind::Choice(o) = p.kind { Some(o) } else { None })
                .unwrap_or(&[]);
            ui.horizontal(|ui| {
                ui.add_sized(vec2(110.0, 20.0), egui::Label::new(egui::RichText::new(tl!("Look")).color(t.text_dim)));
                let mut sel = look;
                egui::ComboBox::from_id_salt("lumetri-look")
                    .selected_text(opts.get(look).map_or(tl!("None"), |o| crate::i18n::t(o)))
                    .width((ui.available_width() - 40.0).max(60.0))
                    .show_ui(ui, |ui| {
                        for (i, o) in opts.iter().enumerate() {
                            if ui.selectable_value(&mut sel, i, crate::i18n::t(o)).changed() {
                                set(actions, "look", json!(i));
                            }
                        }
                    });
            });
            lut_combo(ui, app, e, "look_lut", tl!("Look LUT"), false, lut_actions);
            slider(ui, e, mt, "look_intensity", tl!("Intensity"), None, &t, actions, clip, idx);
            sub(ui, &t, tl!("Adjustments"));
            for (id, label) in
                [("faded_film", tl!("Faded Film")), ("sharpen", tl!("Sharpen")), ("vibrance", tl!("Vibrance")), ("creative_sat", tl!("Saturation"))]
            {
                slider(ui, e, mt, id, label, None, &t, actions, clip, idx);
            }
        }
        "Curves" => {
            sub(ui, &t, tl!("RGB Curves"));
            let key = egui::Id::new("lumetri-curve-channel");
            let mut ch: usize = ui.data(|d| d.get_temp(key)).unwrap_or(0);
            ui.horizontal(|ui| {
                for (i, c) in
                    [Color32::WHITE, Color32::from_rgb(230, 70, 70), Color32::from_rgb(80, 200, 90), Color32::from_rgb(80, 130, 250)].iter().enumerate()
                {
                    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 6.0, *c);
                    if ch == i {
                        ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, t.hot_text));
                    }
                    if resp.clicked() {
                        ch = i;
                    }
                }
            });
            ui.data_mut(|d| d.insert_temp(key, ch));
            let (id, col) = [
                ("curve_luma", Color32::WHITE),
                ("curve_red", Color32::from_rgb(230, 70, 70)),
                ("curve_green", Color32::from_rgb(80, 200, 90)),
                ("curve_blue", Color32::from_rgb(80, 130, 250)),
            ][ch];
            curve_editor(ui, e, id, false, col, &t, actions, clip, idx);
            if hdr {
                let r = slider(ui, e, mt, "curves_hdr_range", tl!("HDR Range"), None, &t, actions, clip, idx);
                auto_rows.push(("lumetri.param.curves_hdr_range".into(), r, "HDR Range"));
            }
            sub(ui, &t, tl!("Hue Saturation Curves"));
            for (id, label) in [
                ("hue_vs_sat", tl!("Hue vs Sat")),
                ("hue_vs_hue", tl!("Hue vs Hue")),
                ("hue_vs_luma", tl!("Hue vs Luma")),
                ("luma_vs_sat", tl!("Luma vs Sat")),
                ("sat_vs_sat", tl!("Sat vs Sat")),
            ] {
                ui.label(egui::RichText::new(label).color(t.text_dim));
                curve_editor(ui, e, id, true, Color32::WHITE, &t, actions, clip, idx);
            }
        }
        "Color Wheels & Match" => {
            let w = ((ui.available_width() - 320.0) / 3.0).clamp(80.0, 110.0);
            ui.horizontal(|ui| {
                for (id, lid, label) in [
                    ("wheel_shadows", "wheel_shadows_l", tl!("Shadows")),
                    ("wheel_midtones", "wheel_midtones_l", tl!("Midtones")),
                    ("wheel_highlights", "wheel_highlights_l", tl!("Highlights")),
                ] {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(label).color(t.text_dim).small());
                        color_wheel(ui, e, mt, id, lid, w, &t, actions, clip, idx);
                    });
                }
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    match_controls(ui, app, lut_actions);
                });
            });
        }
        "Power Windows / Máscara" => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tl!("Herramientas:")).color(t.text_dim).small());
                let pen_on = app.ui.mask_pen.as_ref().is_some_and(|p| p.clip == clip.0 && p.effect == idx);

                // Linear / Rectangle Window
                let rect_btn = ui.button(egui::RichText::new("⬚ Linear").small()).on_hover_text(tl!("Añadir máscara rectangular (Linear Window)"));
                app.auto.add("lumetri.mask.add_linear", rect_btn.rect, "Añadir Linear Window");
                if rect_btn.clicked() {
                    mask_actions.push(("masks.add".into(), json!({"clip": clip.0, "effect": idx, "shape": "polygon"})));
                }

                // Circle / Ellipse Window
                let circle_btn = ui.button(egui::RichText::new("◯ Círculo").small()).on_hover_text(tl!("Añadir máscara circular (Circle Window)"));
                app.auto.add("lumetri.mask.add_circle", circle_btn.rect, "Añadir Circle Window");
                if circle_btn.clicked() {
                    mask_actions.push(("masks.add".into(), json!({"clip": clip.0, "effect": idx, "shape": "ellipse"})));
                }

                // Pen / Free-draw Curve Window
                let mut pen_btn = egui::Button::new(egui::RichText::new("✑ Pluma").small());
                if pen_on {
                    pen_btn = pen_btn.fill(t.accent.gamma_multiply(0.25)).stroke(egui::Stroke::new(1.0, t.accent));
                }
                let pen_resp = ui.add(pen_btn).on_hover_text(tl!("Dibujo libre de curvas Bézier en monitor"));
                app.auto.add("lumetri.mask.add_pen", pen_resp.rect, "Dibujar Máscara con Pluma");
                if pen_resp.clicked() {
                    app.ui.mask_pen = if pen_on { None } else { Some(crate::state::MaskPenDraft { clip: clip.0, effect: idx, points: vec![] }) };
                    if !pen_on {
                        app.ui.status = tl!("Haz clic en el monitor para trazar la máscara; haz clic en el primer punto para cerrar").into();
                    }
                }
            });

            if e.masks.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tl!("Sin máscaras en este nodo. Haz clic en Linear, Círculo o Pluma para crear una.")).color(t.text_dim).small());
            } else {
                let sel_mask_idx = app.session.state.selected_mask
                    .filter(|s| s.clip == clip && s.effect == idx)
                    .map(|s| s.mask.min(e.masks.len() - 1))
                    .unwrap_or(0);

                let m = &e.masks[sel_mask_idx];
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tl!("Ventana:")).color(t.text_dim).small());
                    let mut chosen_mask = sel_mask_idx;
                    egui::ComboBox::from_id_salt("lumetri_mask_select")
                        .selected_text(format!("{} ({})", m.name, sel_mask_idx + 1))
                        .show_ui(ui, |ui| {
                            for (mi, mk) in e.masks.iter().enumerate() {
                                if ui.selectable_value(&mut chosen_mask, mi, format!("{} ({})", mk.name, mi + 1)).clicked() {
                                    mask_actions.push(("masks.select".into(), json!({"clip": clip.0, "effect": idx, "mask": mi})));
                                }
                            }
                        });

                    // Invert button
                    let mut inv = m.inverted;
                    if ui.selectable_label(inv, tl!("Invertir")).on_hover_text(tl!("Invertir máscara")).clicked() {
                        inv = !inv;
                        mask_actions.push(("masks.set".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx, "inverted": inv})));
                    }

                    // Delete button
                    if ui.button(egui::RichText::new("🗑").color(Color32::from_rgb(240, 100, 100))).on_hover_text(tl!("Eliminar máscara")).clicked() {
                        mask_actions.push(("masks.remove".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx})));
                    }
                });

                // Mask sliders: Feather, Opacity, Expansion
                let cur_feather = m.feather.f64_at(mt);
                let cur_opacity = m.opacity.f64_at(mt);
                let cur_expansion = m.expansion.f64_at(mt);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tl!("Calado:")).small());
                    let mut f_val = cur_feather;
                    if ui.add(egui::DragValue::new(&mut f_val).range(0.0..=500.0).speed(0.5).suffix(" px")).changed() {
                        mask_actions.push(("masks.set".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx, "feather": f_val})));
                    }

                    ui.label(egui::RichText::new(tl!("Opacidad:")).small());
                    let mut op_val = cur_opacity;
                    if ui.add(egui::DragValue::new(&mut op_val).range(0.0..=100.0).speed(0.5).suffix(" %")).changed() {
                        mask_actions.push(("masks.set".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx, "opacity": op_val})));
                    }

                    ui.label(egui::RichText::new(tl!("Expansión:")).small());
                    let mut exp_val = cur_expansion;
                    if ui.add(egui::DragValue::new(&mut exp_val).range(-200.0..=200.0).speed(0.5).suffix(" px")).changed() {
                        mask_actions.push(("masks.set".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx, "expansion": exp_val})));
                    }
                });

                // Quick Mask Tracking (Resolve style)
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tl!("Tracker:")).color(t.text_dim).small());
                    if ui.button(tl!("◀ Track Atrás")).clicked() {
                        mask_actions.push(("masks.select".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx})));
                        mask_actions.push(("masks.track".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx, "direction": "backward"})));
                    }
                    if ui.button(tl!("Track Adelante ▶")).clicked() {
                        mask_actions.push(("masks.select".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx})));
                        mask_actions.push(("masks.track".into(), json!({"clip": clip.0, "effect": idx, "mask": sel_mask_idx, "direction": "forward"})));
                    }
                });
            }
        }
        "HSL Secondary" => {
            sub(ui, &t, tl!("Key"));
            slider(ui, e, mt, "hsl_hue", tl!("Hue"), Some(Gradient::Hue), &t, actions, clip, idx);
            for (id, label) in [
                ("hsl_hue_range", tl!("Hue Range")),
                ("hsl_sat_min", tl!("Saturation Min")),
                ("hsl_luma_min", tl!("Luma Min")),
                ("hsl_luma_max", tl!("Luma Max")),
                ("hsl_soft", tl!("Soften")),
            ] {
                slider(ui, e, mt, id, label, None, &t, actions, clip, idx);
            }
            let mask = match e.param("hsl_show_mask").map(|p| &p.value) {
                Some(ParamValue::Choice(c)) => *c as usize,
                _ => 0,
            };
            ui.horizontal(|ui| {
                ui.add_sized(vec2(110.0, 20.0), egui::Label::new(egui::RichText::new(tl!("Show Mask")).color(t.text_dim)));
                let mut sel = mask;
                let modes = [tl!("Off"), tl!("Color/Gray"), tl!("Color/Black"), tl!("White/Black")];
                egui::ComboBox::from_id_salt("hsl-mask").selected_text(modes[mask.min(3)]).show_ui(ui, |ui| {
                    for (i, o) in modes.iter().enumerate() {
                        if ui.selectable_value(&mut sel, i, *o).changed() {
                            set(actions, "hsl_show_mask", json!(i));
                        }
                    }
                });
            });
            sub(ui, &t, tl!("Refine"));
            for (id, label) in [("hsl_denoise", tl!("Denoise")), ("hsl_blur", tl!("Blur"))] {
                let r = slider(ui, e, mt, id, label, None, &t, actions, clip, idx);
                auto_rows.push((format!("lumetri.param.{id}"), r, label));
            }
            sub(ui, &t, tl!("Correction"));
            slider(ui, e, mt, "hsl_temp", tl!("Temperature"), Some(Gradient::Temp), &t, actions, clip, idx);
            slider(ui, e, mt, "hsl_tint", tl!("Tint"), Some(Gradient::Tint), &t, actions, clip, idx);
            slider(ui, e, mt, "hsl_sat", tl!("Saturation"), None, &t, actions, clip, idx);
            slider(ui, e, mt, "hsl_hue_shift", tl!("Hue Shift"), Some(Gradient::Hue), &t, actions, clip, idx);
        }
        "Vignette" => {
            for (id, label) in [
                ("vignette_amount", tl!("Amount")),
                ("vignette_midpoint", tl!("Midpoint")),
                ("vignette_roundness", tl!("Roundness")),
                ("vignette_feather", tl!("Feather")),
            ] {
                slider(ui, e, mt, id, label, None, &t, actions, clip, idx);
            }
        }
        _ => {}
    }
}

/// LUT menu: None, built-ins, the project's LUT library, Browse… (file dialog → `lut.import`).
fn lut_combo(ui: &mut egui::Ui, app: &mut FilmcraftApp, e: &EffectInstance, param: &str, label: &str, input: bool, out: &mut Vec<(&'static str, Value)>) {
    let t = app.tokens;
    let cur = match e.param(param).map(|p| &p.value) {
        Some(ParamValue::Text(s)) => s.clone(),
        _ => String::new(),
    };
    let cmd: &'static str = if input { "lumetri.setInputLut" } else { "lumetri.setLook" };
    let shown = filmcraft_render::luts::label(Some(&app.session.project), &cur);
    let mut entries: Vec<(String, String)> = vec![(String::new(), tl!("None").into())];
    let builtins: Vec<&filmcraft_render::luts::Builtin> = if input {
        filmcraft_render::luts::input_builtins().collect()
    } else {
        filmcraft_render::luts::builtins().iter().filter(|b| b.id.starts_with("look-")).collect()
    };
    entries.extend(builtins.into_iter().map(|b| (format!("builtin:{}", b.id), b.label.clone())));
    entries.extend(app.session.project.luts.iter().map(|l| (format!("lib:{}", l.id), l.name.clone())));
    let mut rect = Rect::NOTHING;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(110.0, 20.0), egui::Label::new(egui::RichText::new(label).color(t.text_dim)));
        let r = egui::ComboBox::from_id_salt(("lumetri-lut", param)).selected_text(shown).truncate().width((ui.available_width() - 40.0).max(60.0)).show_ui(
            ui,
            |ui| {
                for (r, name) in &entries {
                    if ui.selectable_label(*r == cur, name).clicked() && *r != cur {
                        out.push((cmd, json!({"lut": r})));
                    }
                }
                ui.separator();
                if ui.selectable_label(false, tl!("Browse…")).clicked() {
                    out.push((cmd, json!({"browse": true})));
                }
            },
        );
        rect = r.response.rect;
    });
    app.auto.add(&format!("lumetri.{param}"), rect, label);
}

fn sub(ui: &mut egui::Ui, t: &Tokens, name: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(name).color(t.text).strong().size(12.0));
}

/// Resolve-style rotary knob (perilla) with color-guided dual tracks, indicator pointer, drag interaction and double-click reset.
#[allow(clippy::too_many_arguments)]
fn rotary_knob(
    ui: &mut egui::Ui,
    e: &EffectInstance,
    mt: Tick,
    id: &'static str,
    label: &str,
    short_label: &str,
    range: (f64, f64),
    default: f64,
    speed: f64,
    decimals: usize,
    unit: &str,
    accent_color: Option<Color32>,
    t: &Tokens,
    actions: &mut Actions,
    clip: ClipId,
    idx: usize,
) -> Rect {
    let cur_val = e.param(id).map(|p| p.f64_at(mt)).unwrap_or(default);
    let (item_r, resp) = ui.allocate_exact_size(vec2(54.0, 70.0), Sense::click_and_drag());
    let p = ui.painter();

    // Drag interaction
    let mut new_val = cur_val;
    if resp.dragged() {
        let delta = resp.drag_delta();
        let shift = ui.ctx().input(|i| i.modifiers.shift);
        let factor = if shift { 0.25 } else { 1.0 };
        let step = (delta.x - delta.y) as f64 * speed * factor;
        new_val = (cur_val + step).clamp(range.0, range.1);
    }
    if resp.double_clicked() {
        new_val = default;
    }

    // Color theme configuration based on parameter ID (DaVinci Resolve style color cues)
    let (track_col_neg, track_col_pos, active_col_neg, active_col_pos) = match id {
        "temperature" => (
            Color32::from_rgb(30, 125, 235),  // Cool Cyan/Blue
            Color32::from_rgb(255, 125, 25),  // Warm Orange
            Color32::from_rgb(56, 189, 248),  // Glowing Cyan
            Color32::from_rgb(251, 146, 60),  // Glowing Orange
        ),
        "tint" => (
            Color32::from_rgb(40, 195, 75),   // Green
            Color32::from_rgb(230, 50, 185),  // Magenta
            Color32::from_rgb(74, 222, 128),  // Glowing Green
            Color32::from_rgb(244, 63, 94),   // Glowing Magenta
        ),
        "exposure" => (
            Color32::from_rgb(70, 85, 115),
            Color32::from_rgb(240, 205, 50),
            Color32::from_rgb(120, 150, 190),
            Color32::from_rgb(250, 204, 21),
        ),
        "contrast" => (
            Color32::from_rgb(85, 90, 105),
            Color32::from_rgb(170, 90, 240),
            Color32::from_rgb(130, 140, 160),
            Color32::from_rgb(192, 132, 252),
        ),
        "highlights" => (
            Color32::from_rgb(95, 100, 115),
            Color32::from_rgb(250, 240, 160),
            Color32::from_rgb(130, 135, 150),
            Color32::from_rgb(254, 240, 138),
        ),
        "shadows" => (
            Color32::from_rgb(50, 70, 115),
            Color32::from_rgb(100, 165, 250),
            Color32::from_rgb(59, 130, 246),
            Color32::from_rgb(147, 197, 253),
        ),
        "whites" => (
            Color32::from_rgb(90, 100, 115),
            Color32::from_rgb(245, 250, 255),
            Color32::from_rgb(130, 140, 155),
            Color32::from_rgb(255, 255, 255),
        ),
        "blacks" => (
            Color32::from_rgb(30, 35, 45),
            Color32::from_rgb(130, 145, 165),
            Color32::from_rgb(71, 85, 105),
            Color32::from_rgb(148, 163, 184),
        ),
        "saturation" => (
            Color32::from_rgb(90, 90, 90),
            Color32::from_rgb(50, 210, 120),
            Color32::from_rgb(100, 116, 139),
            Color32::from_rgb(52, 211, 153),
        ),
        "vibrance" => (
            Color32::from_rgb(70, 100, 130),
            Color32::from_rgb(40, 200, 230),
            Color32::from_rgb(94, 234, 212),
            Color32::from_rgb(34, 211, 238),
        ),
        "sharpen" => (
            Color32::from_rgb(80, 90, 100),
            Color32::from_rgb(170, 230, 60),
            Color32::from_rgb(110, 120, 130),
            Color32::from_rgb(163, 230, 53),
        ),
        _ => {
            let col = accent_color.unwrap_or_else(|| Color32::from_rgb(60, 140, 240));
            (col.gamma_multiply(0.4), col, col, col)
        }
    };

    // Top Label in small caps
    let label_pos = pos2(item_r.center().x, item_r.min.y + 7.0);
    p.text(
        label_pos,
        Align2::CENTER_CENTER,
        short_label,
        Tokens::semibold(9.5),
        if resp.hovered() { Color32::from_rgb(255, 185, 60) } else { t.text_dim },
    );

    // Draw intuitive guide dots for Temperature and Tint
    if id == "temperature" || id == "tint" {
        p.circle_filled(pos2(item_r.center().x - 17.0, item_r.min.y + 7.0), 2.5, track_col_neg);
        p.circle_filled(pos2(item_r.center().x + 17.0, item_r.min.y + 7.0), 2.5, track_col_pos);
    }

    // Dial Center & Radius
    let kc = pos2(item_r.center().x, item_r.min.y + 32.0);
    let radius = 15.5;

    // Outer Dial Ring & Background
    let is_active = resp.hovered() || resp.dragged();
    let ring_color = if is_active {
        Color32::from_rgb(255, 175, 40) // Resolve signature gold glow
    } else {
        Color32::from_rgb(45, 48, 58)
    };
    p.circle_filled(kc, radius, Color32::from_rgb(20, 23, 29));
    p.circle_stroke(kc, radius, Stroke::new(1.0, ring_color));

    // Dial Angles Math
    let span = (range.1 - range.0).max(1e-4);
    let norm = ((cur_val - range.0) / span).clamp(0.0, 1.0) as f32;
    let def_norm = ((default - range.0) / span).clamp(0.0, 1.0) as f32;

    let min_ang = -135f32.to_radians();
    let max_ang = 135f32.to_radians();
    let cur_ang = min_ang + norm * (max_ang - min_ang);
    let def_ang = min_ang + def_norm * (max_ang - min_ang);

    // Subtle background guide tracks (negative half & positive half)
    let arc_r = radius - 3.0;
    let track_steps = 8;
    let mut prev = kc + vec2(min_ang.sin(), -min_ang.cos()) * arc_r;
    for s in 1..=track_steps {
        let f = s as f32 / track_steps as f32;
        let a = min_ang + f * (def_ang - min_ang);
        let pt = kc + vec2(a.sin(), -a.cos()) * arc_r;
        p.line_segment([prev, pt], Stroke::new(1.8, track_col_neg.gamma_multiply(0.28)));
        prev = pt;
    }
    prev = kc + vec2(def_ang.sin(), -def_ang.cos()) * arc_r;
    for s in 1..=track_steps {
        let f = s as f32 / track_steps as f32;
        let a = def_ang + f * (max_ang - def_ang);
        let pt = kc + vec2(a.sin(), -a.cos()) * arc_r;
        p.line_segment([prev, pt], Stroke::new(1.8, track_col_pos.gamma_multiply(0.28)));
        prev = pt;
    }

    // Default center tick mark (0° marker)
    let tick_tip = kc + vec2(def_ang.sin(), -def_ang.cos()) * (radius + 1.0);
    let tick_base = kc + vec2(def_ang.sin(), -def_ang.cos()) * (radius - 2.0);
    p.line_segment([tick_base, tick_tip], Stroke::new(1.2, Color32::from_rgb(140, 150, 170)));

    // Active Value Arc
    let is_modified = (cur_val - default).abs() > 1e-4;
    let active_color = if cur_val < default { active_col_neg } else { active_col_pos };
    if is_modified {
        let (a_start, a_end) = if cur_ang >= def_ang { (def_ang, cur_ang) } else { (cur_ang, def_ang) };
        if (a_end - a_start).abs() > 0.02 {
            let arc_steps = 14;
            let mut prev = kc + vec2(a_start.sin(), -a_start.cos()) * arc_r;
            for s in 1..=arc_steps {
                let f = s as f32 / arc_steps as f32;
                let a = a_start + f * (a_end - a_start);
                let pt = kc + vec2(a.sin(), -a.cos()) * arc_r;
                p.line_segment([prev, pt], Stroke::new(2.6, active_color));
                prev = pt;
            }
        }
    }

    // Needle pointer line & tip dot
    let pointer_tip = kc + vec2(cur_ang.sin(), -cur_ang.cos()) * (radius - 2.5);
    let pointer_base = kc + vec2(cur_ang.sin(), -cur_ang.cos()) * 4.0;
    p.line_segment([pointer_base, pointer_tip], Stroke::new(2.0, Color32::WHITE));
    p.circle_filled(pointer_tip, 2.0, if is_modified { active_color } else { Color32::WHITE });

    // Center Hub
    p.circle_filled(kc, 3.0, Color32::from_rgb(45, 50, 62));
    p.circle_stroke(kc, 3.0, Stroke::new(1.0, Color32::from_rgb(85, 95, 115)));

    // Bottom numeric readout
    let val_text = if decimals == 0 {
        format!("{:.0}{unit}", cur_val)
    } else {
        format!("{:.1}{unit}", cur_val)
    };
    let val_pos = pos2(item_r.center().x, item_r.max.y - 8.0);
    let val_color = if is_modified {
        active_color
    } else {
        t.text_dim
    };
    p.text(val_pos, Align2::CENTER_CENTER, val_text, Tokens::ui(9.5), val_color);

    if (new_val - cur_val).abs() > 1e-5 {
        actions.push(json!({"clip": clip.0, "effect": idx, "param": id, "value": new_val}));
    }

    resp.on_hover_text(format!("{label}: {cur_val:.1}{unit} (Arrastra o rueda; doble clic para resetear)"));
    item_r
}

#[derive(Clone, Copy)]
enum Gradient {
    Temp,
    Tint,
    Hue,
}

/// Spectrum slider: 1 pt track (or gradient), hollow ring knob, scrubby value at the right.
#[allow(clippy::too_many_arguments)]
fn slider(
    ui: &mut egui::Ui,
    e: &EffectInstance,
    mt: Tick,
    id: &str,
    label: &str,
    grad: Option<Gradient>,
    t: &Tokens,
    actions: &mut Actions,
    clip: ClipId,
    idx: usize,
) -> Rect {
    let Some(pd) = e.def().and_then(|d| d.param(id)) else { return Rect::NOTHING };
    let filmcraft_project::ParamKind::Float { min, max, soft_min, soft_max, decimals, .. } = pd.kind else { return Rect::NOTHING };
    let v = e.param(id).map(|p| p.f64_at(mt)).unwrap_or(pd.default.as_f64().unwrap_or(0.0));
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(pos2(row.min.x + 4.0, row.center().y), Align2::LEFT_CENTER, label, Tokens::ui(12.0), t.text_dim);
    let track = Rect::from_min_max(pos2(row.min.x + 112.0, row.center().y - 1.0), pos2(row.max.x - 58.0, row.center().y + 1.0));
    match grad {
        Some(g) => {
            let n = 24;
            for i in 0..n {
                let f0 = i as f32 / n as f32;
                let c = match g {
                    Gradient::Temp => lerp_col(Color32::from_rgb(70, 120, 230), Color32::from_rgb(240, 150, 50), f0),
                    Gradient::Tint => lerp_col(Color32::from_rgb(60, 190, 80), Color32::from_rgb(220, 70, 200), f0),
                    Gradient::Hue => {
                        let rgb = filmcraft_color::hsl_to_rgb(f0, 0.8, 0.5);
                        Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8)
                    }
                };
                let x0 = track.min.x + track.width() * f0;
                ui.painter().rect_filled(Rect::from_min_max(pos2(x0, track.min.y - 1.0), pos2(x0 + track.width() / n as f32 + 0.5, track.max.y + 1.0)), 0.0, c);
            }
        }
        None => {
            ui.painter().rect_filled(track, 0.0, Color32::from_rgb(0x6e, 0x6e, 0x6e));
        }
    }
    let f = ((v - soft_min) / (soft_max - soft_min)).clamp(0.0, 1.0) as f32;
    let kx = track.min.x + f * track.width();
    let hit = Rect::from_min_max(pos2(track.min.x - 6.0, row.min.y), pos2(track.max.x + 6.0, row.max.y));
    let resp = ui.interact(hit, egui::Id::new(("lslider", clip.0, id)), Sense::click_and_drag());
    ui.painter().circle_filled(pos2(kx, track.center().y), 5.0, t.panel_bg);
    ui.painter().circle_stroke(
        pos2(kx, track.center().y),
        5.0,
        Stroke::new(1.5, if resp.dragged() { t.hot_text } else { Color32::from_rgb(0xb0, 0xb0, 0xb0) }),
    );
    let mut out = None;
    if (resp.dragged() || resp.clicked())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let nf = ((p.x - track.min.x) / track.width()).clamp(0.0, 1.0) as f64;
        out = Some(soft_min + nf * (soft_max - soft_min));
    }
    if resp.double_clicked() {
        out = pd.default.as_f64();
    }
    let mut vui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(pos2(row.max.x - 52.0, row.min.y + 3.0), pos2(row.max.x, row.max.y - 3.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let (_, nv) =
        crate::widgets::hot_number(&mut vui, egui::Id::new(("lval", clip.0, id)), v, (soft_max - soft_min) / 300.0, (min, max), decimals as usize, "", t);
    if let Some(nv) = out.or(nv) {
        actions.push(json!({"clip": clip.0, "effect": idx, "param": id, "value": (nv * 10f64.powi(decimals as i32)).round() / 10f64.powi(decimals as i32)}));
    }
    row
}

fn lerp_col(a: Color32, b: Color32, f: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Curve editor: drag points, click the curve to add, double-click a point to remove.
#[allow(clippy::too_many_arguments)]
fn curve_editor(ui: &mut egui::Ui, e: &EffectInstance, id: &str, hue: bool, col: Color32, t: &Tokens, actions: &mut Actions, clip: ClipId, idx: usize) {
    let pts: Vec<[f32; 2]> = e.param(id).and_then(|p| p.value.as_curve().map(|c| c.to_vec())).unwrap_or_default();
    let w = ui.available_width().min(260.0);
    let h = if hue { w * 0.45 } else { w };
    let (area, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(area, 2.0, Color32::from_rgb(0x16, 0x16, 0x16));
    for i in 1..4 {
        let f = i as f32 / 4.0;
        p.line_segment(
            [pos2(area.min.x + area.width() * f, area.min.y), pos2(area.min.x + area.width() * f, area.max.y)],
            Stroke::new(1.0, Color32::from_rgb(0x2a, 0x2a, 0x2a)),
        );
        p.line_segment(
            [pos2(area.min.x, area.min.y + area.height() * f), pos2(area.max.x, area.min.y + area.height() * f)],
            Stroke::new(1.0, Color32::from_rgb(0x2a, 0x2a, 0x2a)),
        );
    }
    if hue {
        let n = 36;
        for i in 0..n {
            let f0 = i as f32 / n as f32;
            let rgb = filmcraft_color::hsl_to_rgb(f0, 0.8, 0.5);
            let c = Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
            p.rect_filled(
                Rect::from_min_max(
                    pos2(area.min.x + area.width() * f0, area.max.y - 6.0),
                    pos2(area.min.x + area.width() * (f0 + 1.0 / n as f32) + 0.5, area.max.y),
                ),
                0.0,
                c,
            );
        }
    }
    let to_screen = |q: [f32; 2]| pos2(area.min.x + q[0] * area.width(), area.max.y - q[1] * area.height());
    let from_screen = |s: Pos2| [((s.x - area.min.x) / area.width()).clamp(0.0, 1.0), ((area.max.y - s.y) / area.height()).clamp(0.0, 1.0)];
    // curve polyline
    let draw_pts: Vec<[f32; 2]> = if hue && pts.is_empty() { vec![[0.0, 0.5], [1.0, 0.5]] } else { pts.clone() };
    let lut = if hue && !pts.is_empty() {
        let mut ext = Vec::new();
        for off in [-1.0f32, 0.0, 1.0] {
            for q in &pts {
                ext.push([(q[0] + off + 1.0) / 3.0, q[1]]);
            }
        }
        let l = filmcraft_render::effects::curve_lut(&ext, 3 * 128);
        l[128..256].to_vec()
    } else {
        filmcraft_render::effects::curve_lut(&draw_pts, 128)
    };
    let line: Vec<Pos2> = lut.iter().enumerate().map(|(i, y)| to_screen([i as f32 / 127.0, y.clamp(0.0, 1.0)])).collect();
    p.add(egui::Shape::line(line, Stroke::new(1.5, col)));
    let drag_key = egui::Id::new(("curve-drag", clip.0, id));
    let mut dragging: Option<usize> = ui.data(|d| d.get_temp(drag_key));
    let mut new_pts = pts.clone();
    let mut changed = false;
    for (i, q) in pts.iter().enumerate() {
        let sp = to_screen(*q);
        p.circle_filled(sp, 4.0, if dragging == Some(i) { t.hot_text } else { col });
    }
    if resp.drag_started()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let near =
            pts.iter().enumerate().map(|(i, q)| (i, to_screen(*q).distance(pos))).filter(|(_, d)| *d < 9.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0);
        dragging = near.or_else(|| {
            new_pts.push(from_screen(pos));
            changed = true;
            Some(new_pts.len() - 1)
        });
    }
    if resp.dragged()
        && let (Some(i), Some(pos)) = (dragging, resp.interact_pointer_pos())
        && i < new_pts.len()
    {
        new_pts[i] = from_screen(pos);
        changed = true;
    }
    if resp.drag_stopped() {
        dragging = None;
    }
    if resp.double_clicked()
        && let Some(pos) = resp.interact_pointer_pos()
        && let Some(i) = pts.iter().position(|q| to_screen(*q).distance(pos) < 9.0)
    {
        let endpoint = !hue && (i == 0 || i == pts.len() - 1);
        if !endpoint {
            new_pts.remove(i);
            changed = true;
        }
    } else if resp.double_clicked() && !hue {
        new_pts = vec![[0.0, 0.0], [1.0, 1.0]];
        changed = true;
    }
    ui.data_mut(|d| {
        if let Some(i) = dragging {
            d.insert_temp(drag_key, i);
        } else {
            d.remove::<usize>(drag_key);
        }
    });
    if changed {
        let v: Vec<Value> = new_pts.iter().map(|q| json!([q[0], q[1]])).collect();
        actions.push(json!({"clip": clip.0, "effect": idx, "param": id, "value": v}));
    }
    ui.add_space(6.0);
}

/// Colour wheel: hue ring, draggable puck (offset), lightness slider on the left.
#[allow(clippy::too_many_arguments)]
fn color_wheel(ui: &mut egui::Ui, e: &EffectInstance, mt: Tick, id: &str, lid: &str, size: f32, t: &Tokens, actions: &mut Actions, clip: ClipId, idx: usize) {
    let v = e.param(id).map(|p| p.vec2_at(mt)).unwrap_or_default();
    let (area, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let c = area.center() + vec2(6.0, 0.0);
    let r_out = size / 2.0 - 4.0;
    let r_in = r_out - 10.0;
    let p = ui.painter();
    let n = 72;
    let mut mesh = egui::Mesh::default();
    for i in 0..n {
        let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
        let rgb = filmcraft_color::hsl_to_rgb(1.0 - (i as f32 / n as f32), 0.75, 0.5);
        let col = Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
        let base = mesh.vertices.len() as u32;
        for (a, r) in [(a0, r_in), (a0, r_out), (a1, r_out), (a1, r_in)] {
            mesh.colored_vertex(c + vec2(a.cos(), -a.sin()) * r, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    p.add(mesh);
    p.circle_filled(c, r_in - 1.0, Color32::from_rgb(0x16, 0x16, 0x16));
    p.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], Stroke::new(1.0, t.text_faint));
    p.line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], Stroke::new(1.0, t.text_faint));
    let puck = c + vec2(v.x as f32, -v.y as f32) * (r_in - 4.0);
    p.circle_stroke(puck, 5.0, Stroke::new(1.5, Color32::WHITE));
    if (resp.dragged() || resp.clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let d = (pos - c) / (r_in - 4.0);
        let len = d.length().min(1.0);
        let dir = if d.length() > 1e-6 { d / d.length() } else { d };
        actions.push(json!({"clip": clip.0, "effect": idx, "param": id, "value": [dir.x * len, -dir.y * len]}));
    }
    if resp.double_clicked() {
        actions.push(json!({"clip": clip.0, "effect": idx, "param": id, "value": [0.0, 0.0]}));
    }
    // lightness slider (vertical) at the wheel's left edge
    let l = e.param(lid).map(|p| p.f64_at(mt)).unwrap_or(0.0) as f32;
    let track = Rect::from_min_max(pos2(area.min.x, area.min.y + 8.0), pos2(area.min.x + 4.0, area.max.y - 8.0));
    p.rect_filled(track, 2.0, Color32::from_rgb(0x4b, 0x4b, 0x4b));
    let ky = track.center().y - l / 100.0 * track.height() / 2.0;
    p.circle_stroke(pos2(track.center().x, ky), 4.0, Stroke::new(1.5, Color32::from_rgb(0xb0, 0xb0, 0xb0)));
    let lr = ui.interact(track.expand2(vec2(6.0, 0.0)), egui::Id::new(("wl", clip.0, lid)), Sense::click_and_drag());
    if (lr.dragged() || lr.clicked())
        && let Some(pos) = lr.interact_pointer_pos()
    {
        let nv = ((track.center().y - pos.y) / (track.height() / 2.0) * 100.0).clamp(-100.0, 100.0);
        actions.push(json!({"clip": clip.0, "effect": idx, "param": lid, "value": nv}));
    }
    let _ = StrokeKind::Inside;
}

/// Color Match: reference frame (sequence timecode), Face Detection (skin-tone protection),
/// Apply Match (`lumetri.applyMatch`).
fn match_controls(ui: &mut egui::Ui, app: &mut FilmcraftApp, out: &mut Vec<(&'static str, Value)>) {
    let t = app.tokens;
    let key = egui::Id::new("lumetri-match-ref");
    let fd_key = egui::Id::new("lumetri-match-face");
    let mut tc: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| "00:00:00:00".to_string());
    let mut face: bool = ui.data(|d| d.get_temp(fd_key)).unwrap_or(true);
    let comparing = app.ui.program.display_mode() == Some(crate::state::DisplayMode::Comparison);

    ui.horizontal(|ui| {
        sub(ui, &t, tl!("Color Match"));
        ui.add_space(8.0);
        let b = ui.selectable_label(comparing, tl!("Comparison View"));
        app.auto.add("lumetri.match.comparisonView", b.rect, "Comparison View");
        if b.clicked() {
            out.push(("view.display", json!({"display": if comparing { "composite" } else { "comparison" }})));
        }
    });

    let reference = app.ui.program.compare_ref.filter(|_| comparing).map(filmcraft_time::Tick);
    if let (Some(r), Some(q)) = (reference, app.session.active_sequence()) {
        tc = filmcraft_time::format_time(r, q.settings.frame_rate, q.settings.drop_frame, filmcraft_time::TimeDisplay::Timecode, 48000);
    }

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tl!("Reference")).color(t.text_dim).small());
        let r = ui.add(egui::TextEdit::singleline(&mut tc).desired_width(85.0));
        app.auto.add("lumetri.match.reference", r.rect, "Reference timecode");
        let b = ui.button(tl!("Apply Match"));
        app.auto.add("lumetri.match.apply", b.rect, "Apply Match");
        if b.clicked() {
            let p = match reference {
                Some(r) => json!({"referenceTime": r.0, "faceDetection": face}),
                None => json!({"referenceTimecode": tc, "faceDetection": face}),
            };
            out.push(("lumetri.applyMatch", p));
        }
    });

    let r = ui.checkbox(&mut face, tl!("Face Detection (protect skin tones)"));
    app.auto.add("lumetri.match.faceDetection", r.rect, "Face Detection");

    ui.data_mut(|d| {
        d.insert_temp(key, tc);
        d.insert_temp(fd_key, face);
    });
}
