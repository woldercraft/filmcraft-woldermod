//! Timeline Clip Selector Strip (DaVinci Resolve style).
//!
//! Provides a dedicated horizontal strip of clip cards from the sequence's video tracks,
//! completely replacing the multi-track timeline for colorists. Clicking a clip sets it as the
//! active color grade target and parks the playhead on it.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use filmcraft_project::ClipId;

use crate::FilmcraftApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

#[derive(Clone, Debug)]
pub struct ColorClipInfo {
    pub id: ClipId,
    pub name: String,
    pub track_name: String,
    pub start: filmcraft_time::Tick,
    pub duration: filmcraft_time::Tick,
    pub node_count: usize,
    pub is_graded: bool,
    pub is_active: bool,
}

pub fn collect_video_clips(app: &FilmcraftApp) -> Vec<ColorClipInfo> {
    let Some(seq) = app.session.active_sequence() else {
        return Vec::new();
    };
    let active_clip_id = super::lumetri::active_color_clip(app).map(|(c, _)| c);
    let mut clips = Vec::new();

    // Collect all video tracks in order (V1, V2...)
    for track in &seq.video_tracks {
        if track.locked {
            continue;
        }
        for item in &track.items {
            let node_count = item.effects.iter().filter(|e| e.effect == "lumetri").count();
            let is_graded = node_count > 0;
            let is_active = active_clip_id == Some(item.id);
            clips.push(ColorClipInfo {
                id: item.id,
                name: item.name.clone(),
                track_name: track.name.clone(),
                start: item.start,
                duration: item.duration,
                node_count,
                is_graded,
                is_active,
            });
        }
    }

    // Sort chronologically by timeline start position
    clips.sort_by_key(|c| c.start);
    clips
}

pub fn show(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let p = ui.painter().clone();

    // Background bar
    p.rect_filled(rect, 0.0, Color32::from_rgb(20, 23, 28));
    p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, Color32::from_rgb(34, 38, 46)));
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, Color32::from_rgb(14, 16, 20)));

    let clips = collect_video_clips(app);
    if clips.is_empty() {
        crate::dock::placeholder(ui, rect, &t, tl!("(no video clips in active sequence)"));
        return;
    }

    // Top mini-toolbar: "CLIPS (N)" and quick Prev/Next navigation
    let header_h = 20.0;
    let header_rect = Rect::from_min_size(rect.min, vec2(rect.width(), header_h));
    p.rect_filled(header_rect, 0.0, Color32::from_rgb(24, 28, 34));

    let header_text = format!("CLIPS ({})", clips.len());
    p.text(pos2(header_rect.min.x + 10.0, header_rect.center().y), Align2::LEFT_CENTER, header_text, Tokens::semibold(10.0), Color32::from_rgb(160, 175, 195));

    // Prev / Next clip buttons
    let active_idx = clips.iter().position(|c| c.is_active).unwrap_or(0);
    let mut rx = header_rect.max.x - 6.0;

    // Next Clip button
    rx -= 68.0;
    let next_r = Rect::from_min_size(pos2(rx, header_rect.min.y + 2.0), vec2(64.0, 16.0));
    let next_resp = ui.interact(next_r, egui::Id::unique("color_clips_next"), Sense::click());
    app.auto.add("color_clips.next", next_r, "Siguiente Clip");
    p.rect_filled(next_r, 3.0, if next_resp.hovered() { Color32::from_rgb(45, 52, 65) } else { Color32::from_rgb(32, 36, 44) });
    p.text(next_r.center(), Align2::CENTER_CENTER, "Sig ▶", Tokens::ui(9.5), Color32::WHITE);

    // Prev Clip button
    rx -= 72.0;
    let prev_r = Rect::from_min_size(pos2(rx, header_rect.min.y + 2.0), vec2(64.0, 16.0));
    let prev_resp = ui.interact(prev_r, egui::Id::unique("color_clips_prev"), Sense::click());
    app.auto.add("color_clips.prev", prev_r, "Anterior Clip");
    p.rect_filled(prev_r, 3.0, if prev_resp.hovered() { Color32::from_rgb(45, 52, 65) } else { Color32::from_rgb(32, 36, 44) });
    p.text(prev_r.center(), Align2::CENTER_CENTER, "◀ Ant", Tokens::ui(9.5), Color32::WHITE);

    let mut select_clip = None;
    if prev_resp.clicked() && active_idx > 0 {
        select_clip = Some(clips[active_idx - 1].clone());
    }
    if next_resp.clicked() && active_idx + 1 < clips.len() {
        select_clip = Some(clips[active_idx + 1].clone());
    }

    // Scrollable horizontal clips strip
    let strip_rect = Rect::from_min_max(pos2(rect.min.x, rect.min.y + header_h), rect.max);
    let card_w = 124.0;
    let card_h = (strip_rect.height() - 6.0).clamp(36.0, 70.0);

    let playhead = app.session.playhead();

    let mut scroll_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    egui::ScrollArea::horizontal()
        .auto_shrink([false, false])
        .show(&mut scroll_ui, |ui| {
            ui.add_space(8.0);
            for (i, clip) in clips.iter().enumerate() {
                let (card_r, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
                let cp = ui.painter();

                let is_hovered = resp.hovered();
                let is_selected = clip.is_active;

                // Card background
                let bg_color = if is_selected {
                    Color32::from_rgb(32, 38, 48)
                } else if is_hovered {
                    Color32::from_rgb(28, 33, 42)
                } else {
                    Color32::from_rgb(22, 25, 32)
                };

                // Border: Resolve gold for active, blue on hover, gray default
                let border_stroke = if is_selected {
                    Stroke::new(2.0, Color32::from_rgb(255, 175, 40)) // Resolve signature gold
                } else if is_hovered {
                    Stroke::new(1.0, Color32::from_rgb(70, 140, 230))
                } else {
                    Stroke::new(1.0, Color32::from_rgb(42, 48, 58))
                };

                cp.rect_filled(card_r, 4.0, bg_color);
                cp.rect_stroke(card_r, 4.0, border_stroke, StrokeKind::Inside);

                // Top banner: Number and Track badge
                let banner_h = 16.0;
                let banner_r = Rect::from_min_size(card_r.min, vec2(card_r.width(), banner_h));
                cp.rect_filled(banner_r, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, Color32::from_rgb(18, 20, 26));

                let num_str = format!("{:02}", i + 1);
                cp.text(pos2(banner_r.min.x + 6.0, banner_r.center().y), Align2::LEFT_CENTER, &num_str, Tokens::semibold(10.0), if is_selected { Color32::from_rgb(255, 185, 60) } else { Color32::from_rgb(190, 200, 215) });

                // Track badge (e.g. V1)
                cp.text(pos2(banner_r.max.x - 6.0, banner_r.center().y), Align2::RIGHT_CENTER, &clip.track_name, Tokens::ui(9.0), Color32::from_rgb(120, 135, 155));

                // Middle area: Thumbnail box
                let thumb_r = Rect::from_min_max(pos2(card_r.min.x + 4.0, card_r.min.y + banner_h + 2.0), pos2(card_r.max.x - 4.0, card_r.max.y - 18.0));
                cp.rect_filled(thumb_r, 2.0, Color32::from_rgb(14, 16, 20));

                // Clapperboard watermark
                let icon_r = Rect::from_center_size(thumb_r.center(), vec2(16.0, 16.0));
                icons::paint(cp, icon_r, Icon::Film, Color32::from_rgba_unmultiplied(120, 140, 170, 60));

                // Color node indicator badge (Rainbow dot or node count)
                if clip.is_graded {
                    let badge_r = Rect::from_min_size(thumb_r.min + vec2(3.0, 3.0), vec2(34.0, 12.0));
                    cp.rect_filled(badge_r, 2.0, Color32::from_rgba_unmultiplied(40, 140, 70, 210));
                    cp.text(badge_r.center(), Align2::CENTER_CENTER, format!("{} nod", clip.node_count), Tokens::semibold(8.0), Color32::WHITE);
                }

                // Playhead needle across thumbnail if playhead is inside clip
                if playhead >= clip.start && playhead < clip.start + clip.duration && clip.duration.0 > 0 {
                    let prog = (playhead.0 - clip.start.0) as f32 / clip.duration.0 as f32;
                    let nx = thumb_r.min.x + prog.clamp(0.0, 1.0) * thumb_r.width();
                    cp.line_segment([pos2(nx, thumb_r.min.y), pos2(nx, thumb_r.max.y)], Stroke::new(1.5, Color32::from_rgb(255, 170, 30)));
                }

                // Bottom footer: Clip Name and Duration
                let footer_y = card_r.max.y - 14.0;
                let display_name = if clip.name.len() > 14 {
                    format!("{}…", &clip.name[..12])
                } else {
                    clip.name.clone()
                };
                cp.text(pos2(card_r.min.x + 6.0, footer_y), Align2::LEFT_CENTER, display_name, Tokens::ui(9.5), Color32::from_rgb(210, 220, 235));

                let dur_secs = clip.duration.seconds();
                let dur_str = format!("{:.1}s", dur_secs);
                cp.text(pos2(card_r.max.x - 6.0, footer_y), Align2::RIGHT_CENTER, dur_str, Tokens::ui(8.5), Color32::from_rgb(130, 140, 160));

                // Automation IDs
                app.auto.add(&format!("color_clips.clip.{}", clip.id.0), card_r, &clip.name);
                app.auto.add(&format!("color_clips.index.{i}"), card_r, &format!("Clip {}", i + 1));

                if resp.clicked() {
                    select_clip = Some(clip.clone());
                }

                ui.add_space(6.0);
            }
            ui.add_space(8.0);
        });

    if let Some(target) = select_clip {
        // Select clip in session
        app.session.state.selection = vec![target.id];
        // Park playhead at the start of this clip
        app.session.set_playhead(target.start);
        app.ui.active_color_node = 0;
    }
}
