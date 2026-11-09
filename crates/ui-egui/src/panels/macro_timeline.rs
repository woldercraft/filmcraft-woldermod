//! Ultra-lightweight Macro Timeline panel designed for massive clip volumes (40,000+ clips).
//! Consolidates clips into visual MacroBlocks rendered directly via GPU meshes (`egui::Mesh`),
//! enabling instantaneous selection, bulk movement, and global sequence navigation with zero lag.

use std::sync::Arc;
use egui::{pos2, vec2, Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, StrokeKind};
use filmcraft_time::Tick;
use filmcraft_project::{ClipId, Sequence, TrackId};
use serde_json::json;
use crate::FilmcraftApp;
use crate::theme::Tokens;

#[derive(Clone, Debug)]
pub struct MacroBlock {
    pub start: Tick,
    pub end: Tick,
    pub track_id: TrackId,
    pub track_index: usize,
    pub is_video: bool,
    pub color: Color32,
    pub clips: Arc<Vec<ClipId>>,
}

#[derive(Clone, Debug)]
enum MacroDrag {
    PanViewport {
        last_x: f32,
    },
    MoveBlocks {
        grab_tick: Tick,
        offset: Tick,
        clips: Arc<Vec<ClipId>>,
        block_indices: Vec<usize>,
    },
    Marquee {
        start: Pos2,
        current: Pos2,
    },
    Scrub {
        tick: Tick,
    },
}

fn macro_drag_id() -> egui::Id {
    egui::Id::unique("filmcraft-macro-timeline-drag")
}

fn lighten(c: Color32, factor: f32) -> Color32 {
    let f = (factor.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgba_premultiplied(
        c.r().saturating_add(f),
        c.g().saturating_add(f),
        c.b().saturating_add(f),
        c.a(),
    )
}

/// Aggregates sequence clips into contiguous macro blocks per track.
pub fn build_macro_blocks(app: &FilmcraftApp, seq: &Sequence) -> Vec<MacroBlock> {
    let mut blocks = Vec::new();
    let cluster_gap = Tick::from_seconds_f64(0.05);

    // Video tracks
    for (i, tr) in seq.video_tracks.iter().enumerate() {
        let mut cur_block: Option<MacroBlock> = None;
        for it in &tr.items {
            let color = crate::panels::settings::label_color(app, it.label);
            if let Some(ref mut b) = cur_block {
                if it.start <= b.end + cluster_gap && color == b.color {
                    b.end = b.end.max(it.end());
                    if let Some(clips) = Arc::get_mut(&mut b.clips) {
                        clips.push(it.id);
                    }
                    continue;
                } else if let Some(finished) = cur_block.take() {
                    blocks.push(finished);
                }
            }
            cur_block = Some(MacroBlock {
                start: it.start,
                end: it.end(),
                track_id: tr.id,
                track_index: i,
                is_video: true,
                color,
                clips: Arc::new(vec![it.id]),
            });
        }
        if let Some(b) = cur_block {
            blocks.push(b);
        }
    }

    // Audio tracks
    for (i, tr) in seq.audio_tracks.iter().enumerate() {
        let mut cur_block: Option<MacroBlock> = None;
        for it in &tr.items {
            let color = crate::panels::settings::label_color(app, it.label);
            if let Some(ref mut b) = cur_block {
                if it.start <= b.end + cluster_gap && color == b.color {
                    b.end = b.end.max(it.end());
                    if let Some(clips) = Arc::get_mut(&mut b.clips) {
                        clips.push(it.id);
                    }
                    continue;
                } else if let Some(finished) = cur_block.take() {
                    blocks.push(finished);
                }
            }
            cur_block = Some(MacroBlock {
                start: it.start,
                end: it.end(),
                track_id: tr.id,
                track_index: i,
                is_video: false,
                color,
                clips: Arc::new(vec![it.id]),
            });
        }
        if let Some(b) = cur_block {
            blocks.push(b);
        }
    }

    blocks
}

/// Standalone panel entry point for PanelKind::MacroTimeline.
pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    render_macro_timeline(app, ui, rect, false);
}

/// Strip entry point for embedding inside the main timeline.
pub fn render_strip(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    render_macro_timeline(app, ui, rect, true);
}

fn format_timecode_simple(t: Tick, fps: f64) -> String {
    let s = t.seconds().max(0.0);
    let hrs = (s / 3600.0) as u32;
    let mins = ((s % 3600.0) / 60.0) as u32;
    let secs = (s % 60.0) as u32;
    let safe_fps = if fps > 1.0 { fps } else { 30.0 };
    let frames = ((s.fract() * safe_fps).floor() as u32).min(safe_fps.round() as u32 - 1);
    format!("{hrs:02}:{mins:02}:{secs:02}:{frames:02}")
}

fn render_macro_timeline(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect, in_timeline: bool) {
    let p = ui.painter().clone();
    let t = app.tokens.clone();

    // Background
    p.rect_filled(rect, 0.0, if in_timeline { t.panel_bg } else { t.app_bg });

    let Some(seq_id) = app.session.state.active_sequence else {
        p.text(rect.center(), Align2::CENTER_CENTER, "No active sequence", Tokens::ui(12.0), t.text_faint);
        return;
    };
    let project = app.session.project.clone();
    let Some(seq) = project.sequence(seq_id) else {
        p.text(rect.center(), Align2::CENTER_CENTER, "No active sequence", Tokens::ui(12.0), t.text_faint);
        return;
    };

    let total_clips: usize = seq.all_tracks().map(|tr| tr.items.len()).sum();
    let dur_ticks = seq.duration().max(Tick::from_seconds_f64(10.0));
    let dur_secs = dur_ticks.seconds().max(1.0);
    let fps = seq.settings.frame_rate.as_f64().max(1.0);
    let selection: std::collections::HashSet<ClipId> = app.session.state.selection.iter().copied().collect();
    let sel_count = selection.len();

    // Header bar (24 pt inside timeline strip, 28 pt in standalone panel)
    let header_h = if in_timeline { 24.0 } else { 28.0 };
    let header_rect = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + header_h));
    p.rect_filled(header_rect, 0.0, t.header_bg);
    p.line_segment([pos2(rect.min.x, header_rect.max.y), pos2(rect.max.x, header_rect.max.y)], Stroke::new(1.0, t.separator));

    let title_text = if sel_count > 0 {
        format!("⚡ Macro · {total_clips} clips ({sel_count} sel.)")
    } else {
        format!("⚡ Macro · {total_clips} clips")
    };
    p.text(
        pos2(header_rect.min.x + 8.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        &title_text,
        Tokens::semibold(11.0),
        Color32::from_rgb(245, 195, 55),
    );

    // Right cluster: Close Button (if strip) and Fast Mode Toggle
    let mut rx = header_rect.max.x - 6.0;
    if in_timeline {
        let close_w = 22.0;
        rx -= close_w;
        let close_rect = Rect::from_min_size(pos2(rx, header_rect.min.y + 3.0), vec2(close_w, header_h - 6.0));
        let close_resp = ui.interact(close_rect, egui::Id::unique("macro-tl-close-btn"), Sense::click())
            .on_hover_text("Hide Macro Timeline");
        if close_resp.hovered() {
            p.rect_filled(close_rect, 3.0, t.hover);
        }
        p.text(close_rect.center(), Align2::CENTER_CENTER, "✕", Tokens::ui(11.0), t.text_dim);
        if close_resp.clicked() {
            app.ui.timeline.show_macro_timeline = false;
        }
        rx -= 6.0;
    }

    // Fast Mode Toggle Button
    let fast_mode = app.ui.timeline.fast_selection_mode;
    let fast_btn_label = if fast_mode { "⚡ Fast Mode: ON" } else { "Fast Mode: OFF" };
    let fast_btn_w = 105.0;
    rx -= fast_btn_w;
    let fast_btn_rect = Rect::from_min_size(pos2(rx, header_rect.min.y + 3.0), vec2(fast_btn_w, header_h - 6.0));
    let fast_resp = ui.interact(fast_btn_rect, egui::Id::unique("macro-tl-fast-toggle"), Sense::click());
    if fast_resp.clicked() {
        app.ui.timeline.fast_selection_mode = !app.ui.timeline.fast_selection_mode;
    }
    p.rect_filled(
        fast_btn_rect,
        3.0,
        if fast_mode { Color32::from_rgba_unmultiplied(220, 160, 30, 200) } else { t.panel_bg },
    );
    p.text(
        fast_btn_rect.center(),
        Align2::CENTER_CENTER,
        fast_btn_label,
        Tokens::ui(10.0),
        if fast_mode { Color32::BLACK } else { t.text },
    );

    // Middle Quick Action Nudge & Alignment Buttons
    let mut mid_x = rx - 8.0;
    let nudge_btns = [
        ("+5s ▶▶", Tick::from_seconds_f64(5.0), 44.0),
        ("+1s ▶", Tick::from_seconds_f64(1.0), 38.0),
        ("◀ -1s", Tick::from_seconds_f64(-1.0), 38.0),
        ("◀◀ -5s", Tick::from_seconds_f64(-5.0), 44.0),
    ];

    for &(label, offset, bw) in &nudge_btns {
        mid_x -= bw + 4.0;
        let btn_r = Rect::from_min_size(pos2(mid_x, header_rect.min.y + 3.0), vec2(bw, header_h - 6.0));
        let bresp = ui.interact(btn_r, egui::Id::unique(("macro-tl-nudge", label)), Sense::click());
        p.rect_filled(btn_r, 3.0, if bresp.hovered() { t.hover } else { t.panel_bg });
        p.text(btn_r.center(), Align2::CENTER_CENTER, label, Tokens::ui(10.0), t.text);
        if bresp.clicked() {
            let target_clips: Vec<u64> = if !selection.is_empty() {
                selection.iter().map(|c| c.0).collect()
            } else {
                seq.all_tracks().flat_map(|tr| tr.items.iter().map(|it| it.id.0)).collect()
            };
            if !target_clips.is_empty() {
                let _ = app.session.execute("timeline.move", json!({
                    "clips": target_clips,
                    "offset": offset.0,
                    "trackDelta": 0
                }));
            }
        }
    }

    // Align to playhead button
    let align_w = 95.0;
    mid_x -= align_w + 6.0;
    let align_r = Rect::from_min_size(pos2(mid_x, header_rect.min.y + 3.0), vec2(align_w, header_h - 6.0));
    let align_resp = ui.interact(align_r, egui::Id::unique("macro-tl-align-playhead"), Sense::click());
    p.rect_filled(align_r, 3.0, if align_resp.hovered() { t.hover } else { t.panel_bg });
    p.text(align_r.center(), Align2::CENTER_CENTER, "⏱ Al Cabezal", Tokens::ui(10.0), Color32::from_rgb(100, 200, 255));
    if align_resp.clicked() {
        let ph = app.session.playhead();
        let target_clips: Vec<ClipId> = if !selection.is_empty() {
            selection.iter().copied().collect()
        } else {
            seq.all_tracks().flat_map(|tr| tr.items.iter().map(|it| it.id)).collect()
        };
        // Find earliest start among target clips
        let earliest = seq.all_tracks()
            .flat_map(|tr| tr.items.iter())
            .filter(|it| target_clips.contains(&it.id))
            .map(|it| it.start)
            .min();
        if let Some(min_t) = earliest {
            let offset = ph - min_t;
            if offset != Tick::ZERO {
                let clip_nums: Vec<u64> = target_clips.iter().map(|c| c.0).collect();
                let _ = app.session.execute("timeline.move", json!({
                    "clips": clip_nums,
                    "offset": offset.0,
                    "trackDelta": 0
                }));
            }
        }
    }

    // Canvas layout
    let canvas = Rect::from_min_max(
        pos2(rect.min.x + 6.0, header_rect.max.y + 2.0),
        pos2(rect.max.x - 6.0, rect.max.y - 2.0),
    );
    if canvas.width() <= 20.0 || canvas.height() <= 16.0 {
        return;
    }

    // Top Time Ruler (height 18 pt)
    let ruler_h = 18.0;
    let ruler_rect = Rect::from_min_max(canvas.min, pos2(canvas.max.x, canvas.min.y + ruler_h));
    let tracks_canvas = Rect::from_min_max(pos2(canvas.min.x, ruler_rect.max.y), canvas.max);

    p.rect_filled(ruler_rect, 0.0, t.header_bg);
    p.line_segment([ruler_rect.left_bottom(), ruler_rect.right_bottom()], Stroke::new(1.0, t.separator));

    p.rect_filled(tracks_canvas, 2.0, t.tl_bg);
    p.rect_stroke(tracks_canvas, 2.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);

    let nv = seq.video_tracks.len();
    let na = seq.audio_tracks.len();
    let total_tracks = (nv + na).max(1);
    let row_height = (tracks_canvas.height() / total_tracks as f32).clamp(4.0, 36.0);

    let time_to_x = |t: Tick| -> f32 {
        canvas.min.x + ((t.seconds() / dur_secs) as f32 * canvas.width()).clamp(0.0, canvas.width())
    };
    let x_to_tick = |x: f32| -> Tick {
        let ratio = ((x - canvas.min.x) / canvas.width()).clamp(0.0, 1.0) as f64;
        Tick::from_seconds_f64(ratio * dur_secs)
    };

    let blocks = build_macro_blocks(app, seq);
    let mut current_drag: Option<MacroDrag> = ui.ctx().data(|d| d.get_temp::<MacroDrag>(macro_drag_id()));
    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
    let mods = ui.ctx().input(|i| i.modifiers);

    // Main Timeline Viewport bounds (if available)
    let viewport_rect = app.tl.layout.as_ref().map(|main_tl| {
        let start_t = main_tl.tick_at(main_tl.content.min.x);
        let end_t = main_tl.tick_at(main_tl.content.max.x);
        let vx0 = time_to_x(start_t).clamp(tracks_canvas.min.x, tracks_canvas.max.x);
        let vx1 = time_to_x(end_t).clamp(tracks_canvas.min.x, tracks_canvas.max.x).max(vx0 + 4.0);
        Rect::from_min_max(pos2(vx0, tracks_canvas.min.y), pos2(vx1, tracks_canvas.max.y))
    });

    // 1. Time Ruler Graduation & Interaction
    let step_secs = if dur_secs <= 60.0 {
        10.0
    } else if dur_secs <= 300.0 {
        30.0
    } else if dur_secs <= 1200.0 {
        60.0
    } else if dur_secs <= 3600.0 {
        300.0
    } else {
        600.0
    };

    let mut cur_s = 0.0;
    while cur_s <= dur_secs {
        let mark_tick = Tick::from_seconds_f64(cur_s);
        let mx = time_to_x(mark_tick);
        if mx >= ruler_rect.min.x && mx <= ruler_rect.max.x {
            p.line_segment([pos2(mx, ruler_rect.max.y - 5.0), pos2(mx, ruler_rect.max.y)], Stroke::new(1.0, t.separator));
            let label = format_timecode_simple(mark_tick, fps);
            p.text(pos2(mx + 3.0, ruler_rect.min.y + 4.0), Align2::LEFT_TOP, label, Tokens::ui(9.0), t.text_faint);
        }
        cur_s += step_secs;
    }

    let ruler_resp = ui.interact(ruler_rect, egui::Id::unique("macro-timeline-ruler"), Sense::click_and_drag());
    if (ruler_resp.clicked() || ruler_resp.dragged()) && let Some(p_pos) = pointer_pos {
        let target_tick = x_to_tick(p_pos.x);
        let _ = app.session.execute("playhead.set", json!({ "time": target_tick.0 }));
        if let Some(main_tl) = app.tl.layout.as_ref() {
            let view_dur = main_tl.content.width() as f64 / app.ui.timeline.pps;
            app.ui.timeline.target_scroll = (target_tick.seconds() - view_dur * 0.5).max(0.0);
            app.ui.timeline.scroll = app.ui.timeline.target_scroll;
        }
    }

    // 2. Tracks Canvas Interaction
    let tracks_resp = ui.interact(tracks_canvas, egui::Id::unique("macro-timeline-tracks"), Sense::click_and_drag());

    // Hit test: user clicked on any MacroBlock
    let hit_block = pointer_pos.and_then(|p_pos| {
        blocks.iter().enumerate().find(|(_, b)| {
            let row_idx = if b.is_video {
                (nv.saturating_sub(1)).saturating_sub(b.track_index)
            } else {
                nv + b.track_index
            };
            let y0 = tracks_canvas.min.y + row_idx as f32 * row_height;
            let y1 = y0 + row_height;
            let x0 = time_to_x(b.start);
            let x1 = time_to_x(b.end).max(x0 + 4.0);
            Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)).contains(p_pos)
        })
    });

    let is_drag_initiating = tracks_resp.drag_started() || (tracks_resp.dragged() && current_drag.is_none());

    if is_drag_initiating && let Some(p_pos) = pointer_pos {
        if let Some((hit_idx, hit_b)) = hit_block {
            // User grabbed a block: ALWAYS prioritize block movement!
            let any_selected = hit_b.clips.iter().any(|c| selection.contains(c));
            let target_indices: Vec<usize> = if any_selected {
                blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.clips.iter().any(|c| selection.contains(c)))
                    .map(|(i, _)| i)
                    .collect()
            } else {
                vec![hit_idx]
            };

            let all_target_clips: Vec<ClipId> = target_indices
                .iter()
                .flat_map(|&idx| blocks.get(idx).map(|b| b.clips.as_ref().clone()).unwrap_or_default())
                .collect();

            // Select them in the engine so visual highlight is immediate
            let clip_nums: Vec<u64> = all_target_clips.iter().map(|c| c.0).collect();
            let _ = app.session.execute("timeline.select", json!({ "clips": clip_nums, "add": mods.shift }));

            let grab_t = x_to_tick(p_pos.x);
            current_drag = Some(MacroDrag::MoveBlocks {
                grab_tick: grab_t,
                offset: Tick::ZERO,
                clips: Arc::new(all_target_clips),
                block_indices: target_indices,
            });
        } else if mods.alt {
            let t = x_to_tick(p_pos.x);
            let _ = app.session.execute("playhead.set", json!({ "time": t.0 }));
            current_drag = Some(MacroDrag::Scrub { tick: t });
        } else if viewport_rect.is_some_and(|vr| vr.contains(p_pos)) {
            current_drag = Some(MacroDrag::PanViewport { last_x: p_pos.x });
        } else {
            // Empty space: marquee box selection
            current_drag = Some(MacroDrag::Marquee {
                start: p_pos,
                current: p_pos,
            });
        }
    }

    // Handle Active Dragging
    if tracks_resp.dragged() && let Some(p_pos) = pointer_pos {
        match current_drag {
            Some(MacroDrag::PanViewport { ref mut last_x }) => {
                let dx = p_pos.x - *last_x;
                *last_x = p_pos.x;
                let dt = (dx / canvas.width()) as f64 * dur_secs;
                app.ui.timeline.target_scroll = (app.ui.timeline.target_scroll + dt).max(0.0);
                app.ui.timeline.scroll = app.ui.timeline.target_scroll;
            }
            Some(MacroDrag::MoveBlocks { grab_tick, ref mut offset, .. }) => {
                let t_here = x_to_tick(p_pos.x);
                *offset = t_here - grab_tick;
            }
            Some(MacroDrag::Marquee { ref mut current, .. }) => {
                *current = p_pos;
            }
            Some(MacroDrag::Scrub { ref mut tick }) => {
                *tick = x_to_tick(p_pos.x);
                let _ = app.session.execute("playhead.set", json!({"time": tick.0}));
            }
            None => {}
        }
    }

    // Handle Drag Release / Commit
    if tracks_resp.drag_stopped() {
        if let Some(drag) = current_drag.take() {
            match drag {
                MacroDrag::MoveBlocks { offset, clips, .. } if offset != Tick::ZERO => {
                    if app.ui.timeline.fast_selection_mode {
                        // In fast selection mode, register as pending move
                        app.tl.pending_move = Some(crate::panels::timeline::PendingMove {
                            clips: clips.clone(),
                            offset,
                            track_delta: 0,
                            new_track: None,
                            copy: false,
                            bounds: (Tick::ZERO, seq.duration()),
                            all_unlocked: true,
                        });
                    } else {
                        let clip_nums: Vec<u64> = clips.iter().map(|c| c.0).collect();
                        let _ = app.session.execute("timeline.move", json!({
                            "clips": clip_nums,
                            "offset": offset.0,
                            "trackDelta": 0
                        }));
                    }
                }
                MacroDrag::Marquee { start, current } => {
                    let box_rect = Rect::from_two_pos(start, current);
                    let mut selected_clips = Vec::new();
                    for b in &blocks {
                        let row_idx = if b.is_video {
                            (nv.saturating_sub(1)).saturating_sub(b.track_index)
                        } else {
                            nv + b.track_index
                        };
                        let y0 = tracks_canvas.min.y + row_idx as f32 * row_height;
                        let y1 = y0 + row_height;
                        let x0 = time_to_x(b.start);
                        let x1 = time_to_x(b.end).max(x0 + 4.0);
                        let b_rect = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
                        if box_rect.intersects(b_rect) {
                            selected_clips.extend(b.clips.iter().copied());
                        }
                    }
                    let ids: Vec<u64> = selected_clips.iter().map(|c| c.0).collect();
                    let _ = app.session.execute("timeline.select", json!({ "clips": ids, "add": mods.shift }));
                }
                _ => {}
            }
        }
    }

    // Handle Plain Click (select or jump playhead)
    if tracks_resp.clicked() && let Some(p_pos) = pointer_pos {
        if let Some((_, b)) = hit_block {
            let ids: Vec<u64> = b.clips.iter().map(|c| c.0).collect();
            let _ = app.session.execute("timeline.select", json!({ "clips": ids, "add": mods.shift }));
        } else {
            let target_tick = x_to_tick(p_pos.x);
            let _ = app.session.execute("playhead.set", json!({ "time": target_tick.0 }));
            if let Some(main_tl) = app.tl.layout.as_ref() {
                let view_dur = main_tl.content.width() as f64 / app.ui.timeline.pps;
                app.ui.timeline.target_scroll = (target_tick.seconds() - view_dur * 0.5).max(0.0);
                app.ui.timeline.scroll = app.ui.timeline.target_scroll;
            }
        }
    }

    // Save drag state back
    ui.ctx().data_mut(|d| d.insert_temp(macro_drag_id(), current_drag.clone()));

    // ---- GPU Mesh Rendering of MacroBlocks ----
    let mut mesh = egui::Mesh::default();
    let mut selected_mesh = egui::Mesh::default();

    let move_offset = match &current_drag {
        Some(MacroDrag::MoveBlocks { offset, .. }) => *offset,
        _ => Tick::ZERO,
    };
    let moving_indices: &[usize] = match &current_drag {
        Some(MacroDrag::MoveBlocks { block_indices, .. }) => block_indices.as_slice(),
        _ => &[],
    };

    let mut first_moving_start: Option<Tick> = None;

    for (idx, b) in blocks.iter().enumerate() {
        let row_idx = if b.is_video {
            (nv.saturating_sub(1)).saturating_sub(b.track_index)
        } else {
            nv + b.track_index
        };
        let y0 = tracks_canvas.min.y + row_idx as f32 * row_height;
        let y1 = y0 + row_height;

        let is_moving = moving_indices.contains(&idx);
        let eff_offset = if is_moving { move_offset } else { Tick::ZERO };
        let draw_start = (b.start + eff_offset).max(Tick::ZERO);
        let draw_end = (b.end + eff_offset).max(Tick::ZERO);

        if is_moving && first_moving_start.is_none() {
            first_moving_start = Some(draw_start);
        }

        let x0 = time_to_x(draw_start);
        let x1 = time_to_x(draw_end).max(x0 + 2.0);
        let block_rect = Rect::from_min_max(pos2(x0, y0 + 1.0), pos2(x1, y1 - 1.0));

        let is_selected = is_moving || b.clips.iter().any(|c| selection.contains(c));
        if is_selected {
            selected_mesh.add_colored_rect(block_rect, t.clip_selected_border);
            if block_rect.width() > 3.0 && block_rect.height() > 3.0 {
                selected_mesh.add_colored_rect(block_rect.shrink(1.0), lighten(b.color, 0.15));
            }
        } else {
            mesh.add_colored_rect(block_rect, b.color);
        }
    }

    if !mesh.is_empty() {
        p.add(mesh);
    }
    if !selected_mesh.is_empty() {
        p.add(selected_mesh);
    }

    // Video/Audio Track Divider
    if nv > 0 && na > 0 {
        let div_y = tracks_canvas.min.y + nv as f32 * row_height;
        p.line_segment([pos2(tracks_canvas.min.x, div_y), pos2(tracks_canvas.max.x, div_y)], Stroke::new(1.0, t.app_bg));
    }

    // Viewport Minimap Overlay
    if let Some(vr) = viewport_rect {
        p.rect_filled(vr, 2.0, Color32::from_rgba_unmultiplied(60, 140, 240, 35));
        p.rect_stroke(vr, 2.0, Stroke::new(1.0, Color32::from_rgb(100, 180, 255)), StrokeKind::Inside);
    }

    // Marquee Box
    if let Some(MacroDrag::Marquee { start, current }) = &current_drag {
        let mr = Rect::from_two_pos(*start, *current).intersect(tracks_canvas);
        if mr.width() > 0.0 && mr.height() > 0.0 {
            p.rect_filled(mr, 0.0, Color32::from_white_alpha(20));
            p.rect_stroke(mr, 0.0, Stroke::new(1.0, Color32::from_white_alpha(150)), StrokeKind::Inside);
        }
    }

    // Playhead line and diamond across ruler & tracks
    let ph_tick = app.session.playhead();
    let ph_x = time_to_x(ph_tick);
    if ph_x >= canvas.min.x && ph_x <= canvas.max.x {
        p.line_segment([pos2(ph_x, ruler_rect.min.y), pos2(ph_x, tracks_canvas.max.y)], Stroke::new(1.5, t.playhead));
        let tip = pos2(ph_x, ruler_rect.min.y + 6.0);
        let diamond = [pos2(ph_x - 5.0, ruler_rect.min.y), pos2(ph_x + 5.0, ruler_rect.min.y), tip];
        p.add(egui::Shape::convex_polygon(diamond.to_vec(), t.playhead, Stroke::NONE));
    }

    // 3. Hover Time Guide & Label
    if current_drag.is_none() && let Some(p_pos) = pointer_pos && canvas.contains(p_pos) {
        let h_tick = x_to_tick(p_pos.x);
        p.line_segment([pos2(p_pos.x, ruler_rect.min.y), pos2(p_pos.x, tracks_canvas.max.y)], Stroke::new(1.0, Color32::from_white_alpha(70)));
        let h_label = format!("⏱ {}", format_timecode_simple(h_tick, fps));
        let gal = p.layout_no_wrap(h_label, Tokens::ui(9.5), Color32::WHITE);
        let bx = (p_pos.x - gal.size().x * 0.5).clamp(canvas.min.x + 2.0, canvas.max.x - gal.size().x - 6.0);
        let br = Rect::from_min_size(pos2(bx, ruler_rect.min.y + 1.0), gal.size() + vec2(6.0, 2.0));
        p.rect_filled(br, 3.0, Color32::from_rgb(20, 24, 30));
        p.rect_stroke(br, 3.0, Stroke::new(1.0, Color32::from_white_alpha(120)), StrokeKind::Inside);
        p.galley(pos2(bx + 3.0, ruler_rect.min.y + 2.0), gal, Color32::WHITE);
    }

    // 4. Destination Marker on Ruler & Floating HUD while Moving Blocks
    if let Some(MacroDrag::MoveBlocks { offset, clips, .. }) = &current_drag && *offset != Tick::ZERO {
        if let Some(target_start) = first_moving_start {
            let tx = time_to_x(target_start);
            let amber = Color32::from_rgb(245, 195, 55);
            p.line_segment([pos2(tx, ruler_rect.min.y), pos2(tx, tracks_canvas.max.y)], Stroke::new(1.5, amber));

            let dest_label = format!("🎯 Destino: {}", format_timecode_simple(target_start, fps));
            let gal = p.layout_no_wrap(dest_label, Tokens::semibold(10.0), Color32::BLACK);
            let bx = (tx - gal.size().x * 0.5).clamp(canvas.min.x + 2.0, canvas.max.x - gal.size().x - 8.0);
            let br = Rect::from_min_size(pos2(bx, ruler_rect.min.y + 1.0), gal.size() + vec2(8.0, 3.0));
            p.rect_filled(br, 3.0, amber);
            p.galley(pos2(bx + 4.0, ruler_rect.min.y + 2.0), gal, Color32::BLACK);
        }

        if let Some(p_pos) = pointer_pos {
            let delta_sec = offset.seconds();
            let sign = if *offset >= Tick::ZERO { "+" } else { "−" };
            let frames = (offset.0 as f64 / seq.settings.frame_rate.frame_duration().0 as f64).round() as i64;
            let text = format!("⚡ Macro Move: {} clips · {sign}{:.2}s ({sign}{} f)", clips.len(), delta_sec.abs(), frames);
            let gal = p.layout_no_wrap(text, Tokens::ui(11.5), Color32::WHITE);
            let b_size = gal.size() + vec2(16.0, 8.0);
            let b_pos = pos2((p_pos.x + 12.0).min(rect.max.x - b_size.x - 4.0), (p_pos.y + 12.0).min(rect.max.y - b_size.y - 4.0));
            let b_rect = Rect::from_min_size(b_pos, b_size);
            p.rect_filled(b_rect, 4.0, Color32::from_rgba_unmultiplied(20, 24, 30, 240));
            p.rect_stroke(b_rect, 4.0, Stroke::new(1.0, Color32::from_rgb(245, 195, 55)), StrokeKind::Inside);
            p.galley(b_pos + vec2(8.0, 4.0), gal, Color32::WHITE);
        }
    }

    if tracks_resp.hovered() {
        if hit_block.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        } else {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use filmcraft_engine::Session;
    use serde_json::json;

    #[test]
    fn test_macro_blocks_clustering_with_demo_project() {
        let mut session = Session::default();
        session.execute("file.openDemoProject", json!({})).expect("open demo project");
        let app = FilmcraftApp::new(session);

        let seq_id = app.session.state.active_sequence.expect("active sequence in demo project");
        let seq = app.session.project.sequence(seq_id).expect("sequence exists");

        let blocks = build_macro_blocks(&app, seq);
        assert!(!blocks.is_empty(), "macro blocks should be created for demo project");

        let total_clips_in_blocks: usize = blocks.iter().map(|b| b.clips.len()).sum();
        let total_clips_in_seq: usize = seq.all_tracks().map(|t| t.items.len()).sum();
        assert_eq!(total_clips_in_blocks, total_clips_in_seq, "all clips must be represented in macro blocks");
    }
}


