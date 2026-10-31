//! DaVinci Resolve-style Project Manager interface.
//! Allows users to browse, search, organize into folders, create, and manage projects
//! with cinematic cards, responsive grid/list views, and quick actions upon launching FilmCraft.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, Id, Rect, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, Ui,
};
use filmcraft_engine::autosave::default_data_dir;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::FilmcraftApp;
use crate::icons::{self, Icon};
use crate::state::Mode;
use crate::theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortBy {
    #[default]
    DateModified,
    Name,
    Size,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectManagerUi {
    pub search: String,
    pub selected_path: Option<String>,
    #[serde(default)]
    pub selected_paths: HashSet<String>,
    pub current_folder: Option<String>,
    pub view_mode: ViewMode,
    pub sort_by: SortBy,
    pub card_size: f32,
    pub sidebar_open: bool,
    pub new_project_modal: bool,
    pub new_folder_modal: bool,
    pub rename_modal: Option<String>,
    pub modal_input: String,
    pub folders: Vec<String>,
    pub project_folders: HashMap<String, String>,
    #[serde(skip)]
    pub dragging_projects: Option<Vec<String>>,
    #[serde(skip)]
    pub history_back: Vec<Option<String>>,
    #[serde(skip)]
    pub history_forward: Vec<Option<String>>,
}

impl Default for ProjectManagerUi {
    fn default() -> Self {
        Self {
            search: String::new(),
            selected_path: None,
            selected_paths: HashSet::new(),
            current_folder: None,
            view_mode: ViewMode::Grid,
            sort_by: SortBy::DateModified,
            card_size: 210.0,
            sidebar_open: true,
            new_project_modal: false,
            new_folder_modal: false,
            rename_modal: None,
            modal_input: String::new(),
            folders: vec!["YouTube".into(), "Clientes".into(), "Pruebas".into()],
            project_folders: HashMap::new(),
            dragging_projects: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
        }
    }
}
impl ProjectManagerUi {
    pub fn navigate_to(&mut self, folder: Option<String>) {
        if self.current_folder == folder {
            return;
        }
        self.history_back.push(self.current_folder.clone());
        self.current_folder = folder;
        self.history_forward.clear();
        self.clear_selection();
    }

    pub fn can_go_back(&self) -> bool {
        !self.history_back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.history_forward.is_empty()
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.history_back.pop() {
            self.history_forward.push(self.current_folder.clone());
            self.current_folder = prev;
            self.clear_selection();
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(next) = self.history_forward.pop() {
            self.history_back.push(self.current_folder.clone());
            self.current_folder = next;
            self.clear_selection();
        }
    }

    pub fn go_up(&mut self) {
        if self.current_folder.is_some() {
            self.navigate_to(None);
        }
    }
    pub fn is_selected(&self, path: &str) -> bool {
        self.selected_paths.contains(path) || self.selected_path.as_deref() == Some(path)
    }

    pub fn select_single(&mut self, path: String) {
        self.selected_paths.clear();
        self.selected_paths.insert(path.clone());
        self.selected_path = Some(path);
    }

    pub fn toggle_selection(&mut self, path: String) {
        if self.selected_paths.contains(&path) {
            self.selected_paths.remove(&path);
            if self.selected_path.as_deref() == Some(&path) {
                self.selected_path = self.selected_paths.iter().next().cloned();
            }
        } else {
            self.selected_paths.insert(path.clone());
            self.selected_path = Some(path);
        }
    }

    pub fn select_range(&mut self, path: String, all_paths: &[String]) {
        if let Some(anchor) = &self.selected_path {
            let p1 = all_paths.iter().position(|p| p == anchor);
            let p2 = all_paths.iter().position(|p| p == &path);
            if let (Some(a), Some(b)) = (p1, p2) {
                let (start, end) = if a <= b { (a, b) } else { (b, a) };
                for p in &all_paths[start..=end] {
                    self.selected_paths.insert(p.clone());
                }
                self.selected_path = Some(path);
                return;
            }
        }
        self.select_single(path);
    }

    pub fn clear_selection(&mut self) {
        self.selected_paths.clear();
        self.selected_path = None;
    }

    pub fn selected_count(&self) -> usize {
        let count = self.selected_paths.len();
        if count == 0 && self.selected_path.is_some() {
            1
        } else {
            count
        }
    }

    pub fn get_all_selected(&self) -> Vec<String> {
        let mut set = self.selected_paths.clone();
        if let Some(p) = &self.selected_path {
            set.insert(p.clone());
        }
        set.into_iter().collect()
    }
}

#[derive(Clone, Debug)]
pub struct ProjectCard {
    pub name: String,
    pub path: String,
    pub modified: SystemTime,
    pub size_bytes: u64,
    pub folder: Option<String>,
    pub is_current: bool,
    pub is_demo: bool,
}

fn format_modified(time: SystemTime) -> String {
    let now = SystemTime::now();
    let elapsed = now.duration_since(time).unwrap_or_default();
    let secs = elapsed.as_secs();
    if secs < 60 {
        "Hace un momento".into()
    } else if secs < 3600 {
        format!("Hace {} min", (secs / 60).max(1))
    } else if secs < 86400 {
        let hrs = secs / 3600;
        if hrs == 1 { "Hace 1 hora".into() } else { format!("Hace {hrs} horas") }
    } else if secs < 86400 * 7 {
        let days = secs / 86400;
        if days == 1 { "Ayer".into() } else { format!("Hace {days} días") }
    } else {
        let days = secs / 86400;
        format!("Hace {} días", days)
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn projects_storage_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let proyectos = home.join("Proyectos");
        if proyectos.is_dir() {
            return proyectos.join("FilmCraft");
        }
        let docs = home.join("Documentos");
        if docs.is_dir() {
            return docs.join("FilmCraft");
        }
        let eng_docs = home.join("Documents");
        if eng_docs.is_dir() {
            return eng_docs.join("FilmCraft Projects");
        }
        return home.join("Proyectos").join("FilmCraft");
    }
    default_data_dir()
        .map(|d| d.join("Projects"))
        .unwrap_or_else(|| PathBuf::from("Projects"))
}

fn load_persisted_manager(app: &mut FilmcraftApp) {
    let storage = projects_storage_dir();
    let data_file = storage.join("project_manager.json");
    let fallback = default_data_dir().map(|d| d.join("Projects").join("project_manager.json"));

    let content = fs::read_to_string(&data_file)
        .ok()
        .or_else(|| fallback.and_then(|f| fs::read_to_string(f).ok()));

    if let Some(c) = content {
        if let Ok(saved) = serde_json::from_str::<ProjectManagerUi>(&c) {
            app.ui.project_browser.folders = saved.folders;
            app.ui.project_browser.project_folders = saved.project_folders;
            app.ui.project_browser.view_mode = saved.view_mode;
            app.ui.project_browser.card_size = saved.card_size;
        }
    }
}

fn save_persisted_manager(app: &FilmcraftApp) {
    let dir = projects_storage_dir();
    let _ = fs::create_dir_all(&dir);
    let data_file = dir.join("project_manager.json");
    if let Ok(json_str) = serde_json::to_string_pretty(&app.ui.project_browser) {
        let _ = fs::write(data_file, json_str);
    }
}

pub fn discover_projects(app: &FilmcraftApp) -> Vec<ProjectCard> {
    let mut projects = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    let storage_dir = projects_storage_dir();
    let _ = fs::create_dir_all(&storage_dir);

    let mut scan_dirs = vec![storage_dir.clone()];
    if let Some(old_storage) = default_data_dir().map(|d| d.join("Projects")) {
        if old_storage != storage_dir && old_storage.exists() {
            scan_dirs.push(old_storage);
        }
    }

    for dir in scan_dirs {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let folder_name = entry.file_name().to_string_lossy().to_string();
                    if let Ok(sub_entries) = fs::read_dir(&path) {
                        for sub in sub_entries.flatten() {
                            let sub_path = sub.path();
                            if sub_path.extension().is_some_and(|e| e == "fcproj") {
                                let path_str = sub_path.to_string_lossy().to_string();
                                seen_paths.insert(path_str.clone());
                                let name = sub_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                let meta = sub.metadata().ok();
                                let modified = meta.as_ref().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
                                let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                                let is_current = app.session.path.as_deref() == Some(&path_str);
                                projects.push(ProjectCard {
                                    name,
                                    path: path_str,
                                    modified,
                                    size_bytes,
                                    folder: Some(folder_name.clone()),
                                    is_current,
                                    is_demo: false,
                                });
                            }
                        }
                    }
                } else if path.extension().is_some_and(|e| e == "fcproj") {
                    let path_str = path.to_string_lossy().to_string();
                    seen_paths.insert(path_str.clone());
                    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    let meta = entry.metadata().ok();
                    let modified = meta.as_ref().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
                    let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    let folder = app.ui.project_browser.project_folders.get(&path_str).cloned();
                    let is_current = app.session.path.as_deref() == Some(&path_str);
                    projects.push(ProjectCard {
                        name,
                        path: path_str,
                        modified,
                        size_bytes,
                        folder,
                        is_current,
                        is_demo: false,
                    });
                }
            }
        }
    }

    // Recent projects from user preferences
    for p_str in &app.session.prefs.general.recent_projects {
        if seen_paths.contains(p_str) {
            continue;
        }
        let path = Path::new(p_str);
        if path.exists() {
            seen_paths.insert(p_str.clone());
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p_str.clone());
            let meta = fs::metadata(path).ok();
            let modified = meta.as_ref().and_then(|m| m.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
            let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let folder = app.ui.project_browser.project_folders.get(p_str).cloned();
            let is_current = app.session.path.as_deref() == Some(p_str);
            projects.push(ProjectCard {
                name,
                path: p_str.clone(),
                modified,
                size_bytes,
                folder,
                is_current,
                is_demo: false,
            });
        }
    }

    // Also include currently loaded project if not yet on disk or saved
    if app.session.path.is_none() && !app.session.project.items.is_empty() {
        projects.push(ProjectCard {
            name: app.session.project.name.clone(),
            path: "[En memoria]".into(),
            modified: SystemTime::now(),
            size_bytes: 4096,
            folder: None,
            is_current: true,
            is_demo: false,
        });
    }

    // Demo Project Card
    projects.push(ProjectCard {
        name: "Proyecto Demo (Desert Dunes)".into(),
        path: "builtin:demo".into(),
        modified: SystemTime::now(),
        size_bytes: 1024 * 1024 * 45,
        folder: None,
        is_current: false,
        is_demo: true,
    });

    projects
}

pub fn show(app: &mut FilmcraftApp, ui: &mut Ui, rect: Rect) {
    let p = ui.painter().clone();
    let bg_color = Color32::from_rgb(18, 20, 24);
    p.rect_filled(rect, 0.0, bg_color);

    // Initial load check
    let first_run_id = Id::unique("project-manager-first-run");
    if ui.ctx().data(|d| d.get_temp::<bool>(first_run_id)).is_none() {
        load_persisted_manager(app);
        ui.ctx().data_mut(|d| d.insert_temp(first_run_id, true));
    }

    // Mouse side buttons & keyboard navigation (Back / Forward)
    let mut nav_back = false;
    let mut nav_fwd = false;
    ui.input(|i| {
        if i.pointer.button_pressed(egui::PointerButton::Extra1) {
            nav_back = true;
        }
        if i.pointer.button_pressed(egui::PointerButton::Extra2) {
            nav_fwd = true;
        }
        for ev in &i.events {
            if let egui::Event::PointerButton { button, pressed: true, .. } = ev {
                if *button == egui::PointerButton::Extra1 {
                    nav_back = true;
                } else if *button == egui::PointerButton::Extra2 {
                    nav_fwd = true;
                }
            }
        }
        if i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft) {
            nav_back = true;
        }
        if i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight) {
            nav_fwd = true;
        }
    });
    let no_modals = !app.ui.project_browser.new_project_modal
        && !app.ui.project_browser.new_folder_modal
        && app.ui.project_browser.rename_modal.is_none();
    if no_modals && !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
        nav_back = true;
    }
    if nav_back {
        app.ui.project_browser.go_back();
    } else if nav_fwd {
        app.ui.project_browser.go_forward();
    }

    let top_bar_h = 52.0;
    let bottom_bar_h = 44.0;
    let header_rect = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + top_bar_h));
    let bottom_rect = Rect::from_min_max(pos2(rect.min.x, rect.max.y - bottom_bar_h), rect.max);
    let body_rect = Rect::from_min_max(pos2(rect.min.x, header_rect.max.y), pos2(rect.max.x, bottom_rect.min.y));

    // Fill entire body background with DaVinci Resolve dark charcoal
    ui.painter().rect_filled(body_rect, 0.0, Color32::from_rgb(16, 18, 22));

    let all_projects = discover_projects(app);

    // Collect all available folders
    let mut folder_counts: HashMap<String, usize> = HashMap::new();
    for prj in &all_projects {
        if let Some(f) = &prj.folder {
            *folder_counts.entry(f.clone()).or_insert(0) += 1;
        }
    }
    for custom_f in &app.ui.project_browser.folders {
        folder_counts.entry(custom_f.clone()).or_insert(0);
    }

    // 1. TOP BAR
    render_top_bar(app, ui, header_rect, all_projects.len());

    // 2. MAIN LAYOUT (Sidebar + Content)
    let sidebar_w = if app.ui.project_browser.sidebar_open { 210.0 } else { 0.0 };
    let sidebar_rect = Rect::from_min_max(body_rect.min, pos2(body_rect.min.x + sidebar_w, body_rect.max.y));
    let content_rect = Rect::from_min_max(pos2(sidebar_rect.max.x, body_rect.min.y), body_rect.max);

    if app.ui.project_browser.sidebar_open {
        render_sidebar(app, ui, sidebar_rect, all_projects.len(), &folder_counts);
    }

    // Filter projects and folders based on current folder and search
    let search_lower = app.ui.project_browser.search.trim().to_lowercase();
    let current_folder = app.ui.project_browser.current_folder.clone();

    let visible_folders: Vec<String> = if search_lower.is_empty() {
        if current_folder.is_none() {
            app.ui.project_browser.folders.clone()
        } else {
            Vec::new()
        }
    } else {
        app.ui.project_browser.folders
            .iter()
            .filter(|f| f.to_lowercase().contains(&search_lower))
            .cloned()
            .collect()
    };

    let mut filtered_projects: Vec<ProjectCard> = all_projects
        .into_iter()
        .filter(|prj| {
            if search_lower.is_empty() {
                if let Some(ref cf) = current_folder {
                    if prj.folder.as_ref() != Some(cf) && !prj.is_demo {
                        return false;
                    }
                } else if prj.folder.is_some() && !prj.is_demo {
                    return false;
                }
            } else if !prj.name.to_lowercase().contains(&search_lower) {
                return false;
            }
            true
        })
        .collect();

    // Sorting
    match app.ui.project_browser.sort_by {
        SortBy::DateModified => {
            filtered_projects.sort_by(|a, b| b.modified.cmp(&a.modified));
        }
        SortBy::Name => {
            filtered_projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }
        SortBy::Size => {
            filtered_projects.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        }
    }

    render_content(app, ui, content_rect, &filtered_projects, &visible_folders, &folder_counts);

    // 3. BOTTOM BAR
    let total_displayed = filtered_projects.len() + visible_folders.len();
    render_bottom_bar(app, ui, bottom_rect, total_displayed);

    // 4. MODALS (New Project / New Folder / Rename)
    render_modals(app, ui, rect);

    // Floating drag ghost preview when dragging projects
    if let Some(ref dragged) = app.ui.project_browser.dragging_projects {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(pos) = ui.ctx().pointer_latest_pos() {
            let count = dragged.len();
            let label = if count == 1 {
                let name = Path::new(&dragged[0])
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "1 proyecto".into());
                if name.len() > 22 {
                    format!("📁 {}…", &name[..20])
                } else {
                    format!("📁 {name}")
                }
            } else {
                format!("📁 Mover {count} proyectos")
            };
            let font = Tokens::semibold(12.0);
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("pm-drag-tooltip")));
            let galley = painter.layout_no_wrap(label, font, Color32::WHITE);
            let badge_r = Rect::from_min_size(pos + vec2(16.0, 16.0), galley.size() + vec2(20.0, 12.0));
            painter.rect_filled(badge_r, 5.0, Color32::from_rgb(32, 95, 190));
            painter.rect_stroke(badge_r, 5.0, Stroke::new(1.5, Color32::from_rgb(90, 175, 255)), StrokeKind::Inside);
            painter.galley(badge_r.min + vec2(10.0, 6.0), galley, Color32::WHITE);
        }

        // Release anywhere outside of a drop target clears drag state
        if ui.input(|i| i.pointer.any_released()) {
            app.ui.project_browser.dragging_projects = None;
        }
    }
}

fn render_top_bar(app: &mut FilmcraftApp, ui: &mut Ui, rect: Rect, total_count: usize) {
    let p = ui.painter().clone();
    let t = app.tokens;

    p.rect_filled(rect, 0.0, Color32::from_rgb(22, 25, 30));
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, Color32::from_rgb(36, 40, 48)));

    // Logo & Title
    let mut x = rect.min.x + 16.0;
    let icon_rect = Rect::from_center_size(pos2(x + 12.0, rect.center().y), vec2(22.0, 22.0));
    icons::paint(&p, icon_rect, Icon::Film, Color32::from_rgb(50, 140, 250));
    x += 32.0;

    let title_galley = p.layout_no_wrap("ADMINISTRADOR DE PROYECTOS".to_string(), Tokens::semibold(12.5), Color32::WHITE);
    p.galley(pos2(x, rect.center().y - title_galley.size().y * 0.5), title_galley, Color32::WHITE);
    x += 210.0;

    // Navigation buttons: Back (<), Forward (>), Up (^)
    let can_back = app.ui.project_browser.can_go_back();
    let back_r = Rect::from_min_size(pos2(x, rect.center().y - 13.0), vec2(26.0, 26.0));
    let back_resp = ui.interact(back_r, Id::unique("pm-nav-back"), Sense::click())
        .on_hover_text("Atrás (Botón lateral del ratón / Alt+Izquierda)");
    if back_resp.hovered() && can_back {
        p.rect_filled(back_r, 4.0, t.hover);
    }
    icons::paint(&p, back_r.shrink(6.0), Icon::ChevronLeft, if can_back { if back_resp.hovered() { Color32::WHITE } else { t.text } } else { Color32::from_rgb(60, 65, 75) });
    if back_resp.clicked() && can_back {
        app.ui.project_browser.go_back();
    }
    x += 28.0;

    let can_fwd = app.ui.project_browser.can_go_forward();
    let fwd_r = Rect::from_min_size(pos2(x, rect.center().y - 13.0), vec2(26.0, 26.0));
    let fwd_resp = ui.interact(fwd_r, Id::unique("pm-nav-fwd"), Sense::click())
        .on_hover_text("Adelante (Botón lateral del ratón / Alt+Derecha)");
    if fwd_resp.hovered() && can_fwd {
        p.rect_filled(fwd_r, 4.0, t.hover);
    }
    icons::paint(&p, fwd_r.shrink(6.0), Icon::ChevronRight, if can_fwd { if fwd_resp.hovered() { Color32::WHITE } else { t.text } } else { Color32::from_rgb(60, 65, 75) });
    if fwd_resp.clicked() && can_fwd {
        app.ui.project_browser.go_forward();
    }
    x += 28.0;

    let can_up = app.ui.project_browser.current_folder.is_some();
    let up_r = Rect::from_min_size(pos2(x, rect.center().y - 13.0), vec2(26.0, 26.0));
    let up_resp = ui.interact(up_r, Id::unique("pm-nav-up"), Sense::click())
        .on_hover_text("Subir un nivel (Carpeta superior)");
    if up_resp.hovered() && can_up {
        p.rect_filled(up_r, 4.0, t.hover);
    }
    icons::paint(&p, up_r.shrink(6.0), Icon::ArrowUp, if can_up { if up_resp.hovered() { Color32::WHITE } else { t.text } } else { Color32::from_rgb(60, 65, 75) });
    if up_resp.clicked() && can_up {
        app.ui.project_browser.go_up();
    }
    x += 32.0;

    // Breadcrumb navigation
    let breadcrumb_r = Rect::from_min_size(pos2(x, rect.center().y - 12.0), vec2(220.0, 24.0));
    let mut b_text = "Proyectos".to_string();
    if let Some(cf) = &app.ui.project_browser.current_folder {
        b_text.push_str(&format!(" > {cf}"));
    }
    let b_resp = ui.interact(breadcrumb_r, Id::unique("pm-breadcrumb"), Sense::click());
    icons::paint(&p, Rect::from_center_size(pos2(breadcrumb_r.min.x + 8.0, breadcrumb_r.center().y), vec2(13.0, 13.0)), Icon::Home, if b_resp.hovered() { Color32::from_rgb(100, 180, 255) } else { t.text_dim });
    p.text(pos2(breadcrumb_r.min.x + 22.0, breadcrumb_r.center().y), Align2::LEFT_CENTER, &b_text, Tokens::ui(11.5), if b_resp.hovered() { Color32::from_rgb(100, 180, 255) } else { t.text_dim });
    if b_resp.clicked() {
        app.ui.project_browser.navigate_to(None);
    }

    // Right Controls Cluster
    let mut rx = rect.max.x - 16.0;

    // "Continuar edición" button (if session has sequence / items)
    let has_session = !app.session.project.items.is_empty();
    if has_session {
        let resume_w = 135.0;
        rx -= resume_w;
        let resume_r = Rect::from_min_size(pos2(rx, rect.center().y - 14.0), vec2(resume_w, 28.0));
        let resp = ui.interact(resume_r, Id::unique("pm-resume-btn"), Sense::click());
        p.rect_filled(resume_r, 4.0, if resp.hovered() { Color32::from_rgb(45, 125, 235) } else { Color32::from_rgb(35, 105, 210) });
        p.text(resume_r.center(), Align2::CENTER_CENTER, "Continuar edición", Tokens::semibold(11.5), Color32::WHITE);
        if resp.clicked() {
            app.ui.mode = Mode::Edit;
        }
        rx -= 14.0;
    }

    // Zoom Slider
    let slider_w = 70.0;
    rx -= slider_w;
    let slider_rect = Rect::from_min_size(pos2(rx, rect.center().y - 10.0), vec2(slider_w, 20.0));
    let mut sz = app.ui.project_browser.card_size;
    let mut slider_ui = ui.new_child(egui::UiBuilder::new().max_rect(slider_rect));
    if slider_ui.add(egui::Slider::new(&mut sz, 160.0..=320.0).show_value(false)).changed() {
        app.ui.project_browser.card_size = sz;
        save_persisted_manager(app);
    }
    p.text(pos2(rx - 14.0, rect.center().y), Align2::RIGHT_CENTER, "Zoom", Tokens::ui(10.5), t.text_dim);
    rx -= 50.0;

    // View Switcher (Grid / List)
    let is_grid = app.ui.project_browser.view_mode == ViewMode::Grid;
    let view_btn_w = 32.0;
    rx -= view_btn_w;
    let list_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(view_btn_w, 26.0));
    let l_resp = ui.interact(list_r, Id::unique("pm-view-list"), Sense::click()).on_hover_text("Vista de lista");
    p.rect_filled(list_r, 3.0, if !is_grid { Color32::from_rgb(40, 48, 60) } else if l_resp.hovered() { t.hover } else { Color32::TRANSPARENT });
    icons::paint(&p, list_r.shrink(6.0), Icon::ListView, if !is_grid { Color32::from_rgb(100, 180, 255) } else { t.text_dim });
    if l_resp.clicked() {
        app.ui.project_browser.view_mode = ViewMode::List;
        save_persisted_manager(app);
    }

    rx -= view_btn_w + 2.0;
    let grid_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(view_btn_w, 26.0));
    let g_resp = ui.interact(grid_r, Id::unique("pm-view-grid"), Sense::click()).on_hover_text("Vista de cuadrícula");
    p.rect_filled(grid_r, 3.0, if is_grid { Color32::from_rgb(40, 48, 60) } else if g_resp.hovered() { t.hover } else { Color32::TRANSPARENT });
    icons::paint(&p, grid_r.shrink(6.0), Icon::Grid, if is_grid { Color32::from_rgb(100, 180, 255) } else { t.text_dim });
    if g_resp.clicked() {
        app.ui.project_browser.view_mode = ViewMode::Grid;
        save_persisted_manager(app);
    }
    rx -= 16.0;

    // Sort Toggle Button
    let sort_w = 80.0;
    rx -= sort_w;
    let sort_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(sort_w, 26.0));
    let sort_label = match app.ui.project_browser.sort_by {
        SortBy::DateModified => "Fecha",
        SortBy::Name => "Nombre",
        SortBy::Size => "Tamaño",
    };
    let s_resp = ui.interact(sort_r, Id::unique("pm-sort-btn"), Sense::click());
    p.rect_filled(sort_r, 3.0, if s_resp.hovered() { t.hover } else { Color32::from_rgb(28, 32, 40) });
    p.text(sort_r.center(), Align2::CENTER_CENTER, sort_label, Tokens::ui(11.0), t.text);
    if s_resp.clicked() {
        app.ui.project_browser.sort_by = match app.ui.project_browser.sort_by {
            SortBy::DateModified => SortBy::Name,
            SortBy::Name => SortBy::Size,
            SortBy::Size => SortBy::DateModified,
        };
    }
    rx -= 16.0;

    // Search bar
    let search_w = 210.0;
    rx -= search_w;
    let search_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(search_w, 26.0));
    p.rect_filled(search_r, 4.0, Color32::from_rgb(26, 30, 36));
    p.rect_stroke(search_r, 4.0, Stroke::new(1.0, Color32::from_rgb(44, 50, 60)), StrokeKind::Inside);
    let mut search_val = app.ui.project_browser.search.clone();
    let mut s_ui = ui.new_child(egui::UiBuilder::new().max_rect(search_r.shrink2(vec2(6.0, 2.0))));
    s_ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    s_ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    let edit = TextEdit::singleline(&mut search_val).hint_text("Buscar proyectos...");
    if s_ui.add(edit).changed() {
        app.ui.project_browser.search = search_val;
    }
    rx -= 14.0;

    // Toggle Sidebar Button
    let side_btn_w = 95.0;
    rx -= side_btn_w;
    let side_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(side_btn_w, 26.0));
    let side_resp = ui.interact(side_r, Id::unique("pm-toggle-sidebar"), Sense::click());
    p.rect_filled(side_r, 3.0, if side_resp.hovered() { t.hover } else { Color32::from_rgb(28, 32, 40) });
    let side_label = if app.ui.project_browser.sidebar_open { "Ocultar panel" } else { "Bibliotecas" };
    p.text(side_r.center(), Align2::CENTER_CENTER, side_label, Tokens::ui(11.0), t.text_dim);
    if side_resp.clicked() {
        app.ui.project_browser.sidebar_open = !app.ui.project_browser.sidebar_open;
    }

    let _ = total_count;
}

fn render_sidebar(app: &mut FilmcraftApp, ui: &mut Ui, rect: Rect, total_count: usize, folder_counts: &HashMap<String, usize>) {
    let p = ui.painter().clone();
    let t = app.tokens;

    p.rect_filled(rect, 0.0, Color32::from_rgb(20, 22, 26));
    p.line_segment([pos2(rect.max.x, rect.min.y), pos2(rect.max.x, rect.max.y)], Stroke::new(1.0, Color32::from_rgb(32, 36, 44)));

    let mut y = rect.min.y + 14.0;

    // Section 1: BIBLIOTECA DE PROYECTOS
    p.text(pos2(rect.min.x + 14.0, y), Align2::LEFT_TOP, "BIBLIOTECAS DE PROYECTOS", Tokens::semibold(9.5), Color32::from_rgb(110, 120, 135));
    y += 24.0;

    // Local Library tile
    let lib_r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2(rect.width() - 16.0, 30.0));
    let lib_resp = ui.interact(lib_r, Id::unique("pm-side-lib-local"), Sense::click())
        .on_hover_text("Abrir carpeta de proyectos en el explorador de archivos");
    p.rect_filled(lib_r, 4.0, if lib_resp.hovered() { Color32::from_rgb(38, 48, 62) } else { Color32::from_rgb(28, 34, 44) });
    p.rect_stroke(lib_r, 4.0, Stroke::new(1.0, Color32::from_rgb(45, 60, 85)), StrokeKind::Inside);
    icons::paint(&p, Rect::from_center_size(pos2(lib_r.min.x + 18.0, lib_r.center().y), vec2(14.0, 14.0)), Icon::Home, Color32::from_rgb(100, 180, 255));
    p.text(pos2(lib_r.min.x + 32.0, lib_r.center().y), Align2::LEFT_CENTER, "Esta Máquina (Local)", Tokens::semibold(11.0), Color32::WHITE);
    icons::paint(&p, Rect::from_center_size(pos2(lib_r.max.x - 14.0, lib_r.center().y), vec2(12.0, 12.0)), Icon::Folder, Color32::from_rgb(130, 140, 155));
    if lib_resp.clicked() {
        let storage = projects_storage_dir();
        let _ = fs::create_dir_all(&storage);
        let ctx = ui.ctx().clone();
        let _ = crate::panels::menu_dialogs::open_path(app, &ctx, &storage.to_string_lossy(), false);
    }
    y += 40.0;

    // Cloud / Network item
    let cloud_r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2(rect.width() - 16.0, 26.0));
    icons::paint(&p, Rect::from_center_size(pos2(cloud_r.min.x + 18.0, cloud_r.center().y), vec2(14.0, 14.0)), Icon::Globe, t.text_faint);
    p.text(pos2(cloud_r.min.x + 32.0, cloud_r.center().y), Align2::LEFT_CENTER, "Red / Cloud", Tokens::ui(11.0), t.text_faint);
    p.text(pos2(cloud_r.max.x - 8.0, cloud_r.center().y), Align2::RIGHT_CENTER, "Pronto", Tokens::ui(9.0), Color32::from_rgb(140, 150, 165));
    y += 38.0;

    p.line_segment([pos2(rect.min.x + 10.0, y), pos2(rect.max.x - 10.0, y)], Stroke::new(1.0, Color32::from_rgb(32, 36, 44)));
    y += 12.0;

    // Section 2: CARPETAS
    p.text(pos2(rect.min.x + 14.0, y), Align2::LEFT_TOP, "ORGANIZACIÓN", Tokens::semibold(9.5), Color32::from_rgb(110, 120, 135));
    y += 24.0;

    // "Todos los proyectos"
    let all_active = app.ui.project_browser.current_folder.is_none();
    let all_r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2(rect.width() - 16.0, 26.0));
    let all_resp = ui.interact(all_r, Id::unique("pm-folder-all"), Sense::click());
    let is_dragging = app.ui.project_browser.dragging_projects.is_some();
    let drop_on_all = is_dragging && all_resp.hovered();
    if drop_on_all {
        p.rect_filled(all_r, 4.0, Color32::from_rgb(40, 85, 140));
        p.rect_stroke(all_r, 4.0, Stroke::new(1.5, Color32::from_rgb(80, 170, 255)), StrokeKind::Inside);
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(paths) = app.ui.project_browser.dragging_projects.take() {
                for path in paths {
                    app.ui.project_browser.project_folders.remove(&path);
                }
                save_persisted_manager(app);
            }
        }
    } else if all_active {
        p.rect_filled(all_r, 4.0, Color32::from_rgb(34, 45, 60));
    } else if all_resp.hovered() {
        p.rect_filled(all_r, 4.0, t.hover);
    }
    icons::paint(&p, Rect::from_center_size(pos2(all_r.min.x + 18.0, all_r.center().y), vec2(13.0, 13.0)), Icon::Folder, if all_active { Color32::from_rgb(100, 180, 255) } else { t.text_dim });
    p.text(pos2(all_r.min.x + 32.0, all_r.center().y), Align2::LEFT_CENTER, "Todos los proyectos", Tokens::ui(11.5), if all_active { Color32::WHITE } else { t.text });
    p.text(pos2(all_r.max.x - 10.0, all_r.center().y), Align2::RIGHT_CENTER, format!("{total_count}"), Tokens::ui(10.5), t.text_dim);
    if all_resp.clicked() {
        app.ui.project_browser.navigate_to(None);
    }
    y += 30.0;

    // Custom folders
    let mut folder_to_delete = None;
    let mut folder_to_navigate = None;
    for f in &app.ui.project_browser.folders {
        let is_sel = app.ui.project_browser.current_folder.as_ref() == Some(f);
        let f_r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2(rect.width() - 16.0, 26.0));
        let f_resp = ui.interact(f_r, Id::unique(("pm-folder", f)), Sense::click());
        let drop_on_f = is_dragging && f_resp.hovered();
        if drop_on_f {
            p.rect_filled(f_r, 4.0, Color32::from_rgb(40, 85, 140));
            p.rect_stroke(f_r, 4.0, Stroke::new(1.5, Color32::from_rgb(80, 170, 255)), StrokeKind::Inside);
            if ui.input(|i| i.pointer.any_released()) {
                if let Some(paths) = app.ui.project_browser.dragging_projects.take() {
                    for path in paths {
                        app.ui.project_browser.project_folders.insert(path, f.clone());
                    }
                    save_persisted_manager(app);
                }
            }
        } else if is_sel {
            p.rect_filled(f_r, 4.0, Color32::from_rgb(34, 45, 60));
        } else if f_resp.hovered() {
            p.rect_filled(f_r, 4.0, t.hover);
        }
        icons::paint(&p, Rect::from_center_size(pos2(f_r.min.x + 18.0, f_r.center().y), vec2(13.0, 13.0)), Icon::Folder, if is_sel { Color32::from_rgb(100, 180, 255) } else { t.text_dim });
        let count = folder_counts.get(f).copied().unwrap_or(0);
        p.text(pos2(f_r.min.x + 32.0, f_r.center().y), Align2::LEFT_CENTER, f.as_str(), Tokens::ui(11.0), if is_sel { Color32::WHITE } else { t.text });
        p.text(pos2(f_r.max.x - 10.0, f_r.center().y), Align2::RIGHT_CENTER, format!("{count}"), Tokens::ui(10.5), t.text_dim);
        if f_resp.clicked() {
            folder_to_navigate = Some(f.clone());
        }

        f_resp.context_menu(|ui| {
            if ui.button("Eliminar carpeta").clicked() {
                folder_to_delete = Some(f.clone());
                ui.close();
            }
        });

        y += 28.0;
    }

    if let Some(nav_f) = folder_to_navigate {
        app.ui.project_browser.navigate_to(Some(nav_f));
    }

    if let Some(del_f) = folder_to_delete {
        app.ui.project_browser.folders.retain(|f| f != &del_f);
        if app.ui.project_browser.current_folder.as_deref() == Some(&del_f) {
            app.ui.project_browser.navigate_to(None);
        }
        app.ui.project_browser.project_folders.retain(|_, f| f != &del_f);
        save_persisted_manager(app);
    }

    // New Folder button in sidebar
    y += 8.0;
    let add_f_r = Rect::from_min_size(pos2(rect.min.x + 12.0, y), vec2(rect.width() - 24.0, 24.0));
    let add_resp = ui.interact(add_f_r, Id::unique("pm-side-add-folder"), Sense::click());
    p.rect_stroke(add_f_r, 3.0, Stroke::new(1.0, if add_resp.hovered() { Color32::from_rgb(60, 140, 240) } else { Color32::from_rgb(45, 52, 64) }), StrokeKind::Inside);
    p.text(add_f_r.center(), Align2::CENTER_CENTER, "+ Nueva carpeta", Tokens::ui(10.5), if add_resp.hovered() { Color32::WHITE } else { t.text_dim });
    if add_resp.clicked() {
        app.ui.project_browser.new_folder_modal = true;
        app.ui.project_browser.modal_input.clear();
    }
}

fn render_content(
    app: &mut FilmcraftApp,
    ui: &mut Ui,
    rect: Rect,
    projects: &[ProjectCard],
    visible_folders: &[String],
    folder_counts: &HashMap<String, usize>,
) {
    // Generous breathing margins: 32px on the left (clear separation from sidebar line),
    // 24px on the right, 20px top and bottom
    let pad_left = 32.0;
    let pad_right = 24.0;
    let pad_y = 20.0;
    let padded_rect = Rect::from_min_max(
        pos2(rect.min.x + pad_left, rect.min.y + pad_y),
        pos2(rect.max.x - pad_right, rect.max.y - pad_y),
    );

    let mut content_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(padded_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );

    // Shortcut: Delete removes all selected projects from recent list
    let no_modals = !app.ui.project_browser.new_project_modal
        && !app.ui.project_browser.new_folder_modal
        && app.ui.project_browser.rename_modal.is_none();
    if no_modals && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
        let all_sel = app.ui.project_browser.get_all_selected();
        if !all_sel.is_empty() {
            let set: HashSet<String> = all_sel.into_iter().collect();
            app.session.prefs.general.recent_projects.retain(|p| !set.contains(p));
            for p in &set {
                app.ui.project_browser.project_folders.remove(p);
                app.ui.project_browser.selected_paths.remove(p);
            }
            if let Some(sp) = &app.ui.project_browser.selected_path && set.contains(sp) {
                app.ui.project_browser.selected_path = None;
            }
            save_persisted_manager(app);
        }
    }

    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut content_ui, |ui| {
            if app.ui.project_browser.view_mode == ViewMode::Grid {
                render_grid_view(app, ui, projects, visible_folders, folder_counts);
            } else {
                render_list_view(app, ui, projects, visible_folders, folder_counts);
            }
        });
}

fn render_grid_view(
    app: &mut FilmcraftApp,
    ui: &mut Ui,
    projects: &[ProjectCard],
    visible_folders: &[String],
    folder_counts: &HashMap<String, usize>,
) {
    let card_w = app.ui.project_browser.card_size;
    let card_h = card_w * (9.0 / 16.0) + 52.0;

    let avail_w = ui.available_width();
    let cols = ((avail_w / (card_w + 16.0)).floor() as usize).max(1);

    let all_paths: Vec<String> = projects.iter().map(|p| p.path.clone()).collect();
    let mut folder_to_open = None;
    let mut folder_to_rename = None;
    let mut folder_to_delete = None;

    ui.add_space(4.0);
    egui::Grid::new("pm-project-grid")
        .spacing(vec2(16.0, 16.0))
        .min_col_width(card_w)
        .show(ui, |ui| {
            let mut col = 0;

            // Tile 1: "Nuevo Proyecto"
            render_action_card(
                ui,
                card_w,
                card_h,
                Icon::Plus,
                "Nuevo Proyecto",
                "Crear proyecto en blanco",
                || {
                    app.ui.project_browser.new_project_modal = true;
                    app.ui.project_browser.modal_input = "Sin título".into();
                },
            );
            col += 1;
            if col >= cols {
                ui.end_row();
                col = 0;
            }

            // Tile 2: "Nueva Carpeta"
            render_action_card(
                ui,
                card_w,
                card_h,
                Icon::Folder,
                "Nueva Carpeta",
                "Organizar proyectos",
                || {
                    app.ui.project_browser.new_folder_modal = true;
                    app.ui.project_browser.modal_input.clear();
                },
            );
            col += 1;
            if col >= cols {
                ui.end_row();
                col = 0;
            }

            // Up Directory Tile (if inside a folder)
            if app.ui.project_browser.current_folder.is_some() {
                let (up_rect, up_resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
                let is_dragging = app.ui.project_browser.dragging_projects.is_some();
                let drop_on_up = is_dragging && up_resp.hovered();
                let p_up = ui.painter().clone();
                p_up.rect_filled(up_rect, 6.0, Color32::from_rgb(26, 30, 36));
                let up_thumb_h = card_w * (9.0 / 16.0);
                let up_thumb_rect = Rect::from_min_max(up_rect.min, pos2(up_rect.max.x, up_rect.min.y + up_thumb_h));
                p_up.rect_filled(up_thumb_rect, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, if drop_on_up { Color32::from_rgb(35, 75, 125) } else { Color32::from_rgb(20, 24, 30) });
                icons::paint(&p_up, Rect::from_center_size(up_thumb_rect.center(), vec2(36.0, 36.0)), Icon::ArrowUp, if drop_on_up { Color32::from_rgb(100, 190, 255) } else { Color32::from_rgb(80, 150, 240) });
                let up_meta_rect = Rect::from_min_max(pos2(up_rect.min.x, up_rect.min.y + up_thumb_h), up_rect.max);
                p_up.text(pos2(up_meta_rect.min.x + 12.0, up_meta_rect.min.y + 16.0), Align2::LEFT_CENTER, ".. (Subir nivel)", Tokens::semibold(12.0), Color32::WHITE);
                p_up.text(pos2(up_meta_rect.min.x + 12.0, up_meta_rect.min.y + 34.0), Align2::LEFT_CENTER, "Volver a Proyectos", Tokens::ui(10.5), app.tokens.text_dim);
                if drop_on_up {
                    p_up.rect_stroke(up_rect, 6.0, Stroke::new(2.0, Color32::from_rgb(80, 175, 255)), StrokeKind::Inside);
                    if ui.input(|i| i.pointer.any_released()) {
                        if let Some(paths) = app.ui.project_browser.dragging_projects.take() {
                            for path in paths {
                                app.ui.project_browser.project_folders.remove(&path);
                            }
                            save_persisted_manager(app);
                        }
                    }
                } else if up_resp.hovered() {
                    p_up.rect_stroke(up_rect, 6.0, Stroke::new(1.5, Color32::from_rgb(60, 120, 200)), StrokeKind::Inside);
                } else {
                    p_up.rect_stroke(up_rect, 6.0, Stroke::new(1.0, Color32::from_rgb(38, 44, 54)), StrokeKind::Inside);
                }
                if up_resp.clicked() {
                    app.ui.project_browser.go_up();
                }
                col += 1;
                if col >= cols {
                    ui.end_row();
                    col = 0;
                }
            }

            // Visible Folders (rendered as Folder Cards)
            for f in visible_folders {
                let (f_rect, f_resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
                let is_dragging = app.ui.project_browser.dragging_projects.is_some();
                let drop_on_f = is_dragging && f_resp.hovered();
                let p_f = ui.painter().clone();
                p_f.rect_filled(f_rect, 6.0, Color32::from_rgb(26, 30, 36));
                let f_thumb_h = card_w * (9.0 / 16.0);
                let f_thumb_rect = Rect::from_min_max(f_rect.min, pos2(f_rect.max.x, f_rect.min.y + f_thumb_h));
                p_f.rect_filled(f_thumb_rect, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, if drop_on_f { Color32::from_rgb(35, 75, 125) } else { Color32::from_rgb(20, 24, 30) });
                let f_icon_color = if drop_on_f {
                    Color32::from_rgb(100, 190, 255)
                } else if f_resp.hovered() {
                    Color32::from_rgb(255, 205, 75)
                } else {
                    Color32::from_rgb(240, 180, 50)
                };
                icons::paint(&p_f, Rect::from_center_size(f_thumb_rect.center(), vec2(42.0, 42.0)), Icon::Folder, f_icon_color);

                let count = folder_counts.get(f).copied().unwrap_or(0);
                let f_meta_rect = Rect::from_min_max(pos2(f_rect.min.x, f_rect.min.y + f_thumb_h), f_rect.max);
                p_f.text(pos2(f_meta_rect.min.x + 12.0, f_meta_rect.min.y + 16.0), Align2::LEFT_CENTER, f.as_str(), Tokens::semibold(12.5), Color32::WHITE);
                p_f.text(pos2(f_meta_rect.min.x + 12.0, f_meta_rect.min.y + 34.0), Align2::LEFT_CENTER, format!("{count} proyectos"), Tokens::ui(10.5), app.tokens.text_dim);

                if drop_on_f {
                    p_f.rect_stroke(f_rect, 6.0, Stroke::new(2.0, Color32::from_rgb(80, 175, 255)), StrokeKind::Inside);
                    if ui.input(|i| i.pointer.any_released()) {
                        if let Some(paths) = app.ui.project_browser.dragging_projects.take() {
                            for path in paths {
                                app.ui.project_browser.project_folders.insert(path, f.clone());
                            }
                            save_persisted_manager(app);
                        }
                    }
                } else if f_resp.hovered() {
                    p_f.rect_stroke(f_rect, 6.0, Stroke::new(1.5, Color32::from_rgb(70, 130, 210)), StrokeKind::Inside);
                } else {
                    p_f.rect_stroke(f_rect, 6.0, Stroke::new(1.0, Color32::from_rgb(38, 44, 54)), StrokeKind::Inside);
                }

                if f_resp.double_clicked() {
                    folder_to_open = Some(f.clone());
                }

                f_resp.context_menu(|ui| {
                    if ui.button("Abrir carpeta").clicked() {
                        folder_to_open = Some(f.clone());
                        ui.close();
                    }
                    if ui.button("Renombrar...").clicked() {
                        folder_to_rename = Some(f.clone());
                        ui.close();
                    }
                    if ui.button("Eliminar carpeta").clicked() {
                        folder_to_delete = Some(f.clone());
                        ui.close();
                    }
                });

                col += 1;
                if col >= cols {
                    ui.end_row();
                    col = 0;
                }
            }

            // Project Cards
            let mut project_to_open = None;
            let mut project_to_rename = None;
            let mut projects_to_delete: Option<Vec<String>> = None;
            let mut move_projects_to_folder: Option<(Vec<String>, String)> = None;
            let mut move_projects_to_storage: Option<Vec<String>> = None;

            for prj in projects {
                let is_selected = app.ui.project_browser.is_selected(&prj.path);
                let (rect, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click_and_drag());
                let p = ui.painter().clone();

                let is_hovered = resp.hovered();

                // Card frame
                let bg_card = Color32::from_rgb(26, 30, 36);
                p.rect_filled(rect, 6.0, bg_card);

                // 16:9 Thumbnail Area
                let thumb_h = card_w * (9.0 / 16.0);
                let thumb_rect = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + thumb_h));
                let thumb_bg = if prj.is_demo {
                    Color32::from_rgb(32, 48, 64)
                } else {
                    Color32::from_rgb(18, 22, 28)
                };
                p.rect_filled(thumb_rect, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, thumb_bg);

                // Thumbnail icon / clapperboard watermark
                let icon_size = (thumb_h * 0.45).clamp(24.0, 56.0);
                let icon_rect = Rect::from_center_size(thumb_rect.center(), vec2(icon_size, icon_size));
                icons::paint(&p, icon_rect, if prj.is_demo { Icon::Sequence } else { Icon::Film }, Color32::from_rgba_unmultiplied(120, 140, 180, 55));

                // Active project green badge
                if prj.is_current {
                    let badge_r = Rect::from_min_size(thumb_rect.min + vec2(8.0, 8.0), vec2(78.0, 20.0));
                    p.rect_filled(badge_r, 3.0, Color32::from_rgba_unmultiplied(35, 140, 65, 220));
                    p.text(badge_r.center(), Align2::CENTER_CENTER, "En edición", Tokens::semibold(9.5), Color32::WHITE);
                }

                // Format resolution badge (1080p / 4K)
                let fmt_r = Rect::from_min_size(pos2(thumb_rect.max.x - 48.0, thumb_rect.max.y - 20.0), vec2(42.0, 16.0));
                p.rect_filled(fmt_r, 3.0, Color32::from_rgba_unmultiplied(15, 18, 24, 180));
                p.text(fmt_r.center(), Align2::CENTER_CENTER, "1080p", Tokens::ui(9.0), Color32::from_rgb(180, 190, 205));

                // Bottom Meta Area
                let meta_y = thumb_rect.max.y + 8.0;
                let title_text = if prj.name.len() > 24 {
                    format!("{}…", &prj.name[..22])
                } else {
                    prj.name.clone()
                };
                p.text(pos2(rect.min.x + 10.0, meta_y), Align2::LEFT_TOP, title_text, Tokens::semibold(12.0), Color32::WHITE);

                let sub_text = format!("{} · {}", format_modified(prj.modified), format_size(prj.size_bytes));
                p.text(pos2(rect.min.x + 10.0, meta_y + 18.0), Align2::LEFT_TOP, sub_text, Tokens::ui(10.0), Color32::from_rgb(140, 150, 165));

                if let Some(ref f) = prj.folder {
                    let f_badge = f.clone();
                    p.text(pos2(rect.max.x - 10.0, meta_y + 18.0), Align2::RIGHT_TOP, f_badge, Tokens::ui(9.5), Color32::from_rgb(100, 160, 220));
                }

                // Border & Selection highlight — drawn on top of the whole card
                let border_col = if is_selected {
                    Color32::from_rgb(50, 140, 250)
                } else if is_hovered {
                    Color32::from_rgb(70, 120, 190)
                } else {
                    Color32::from_rgb(38, 44, 54)
                };
                let stroke_w = if is_selected { 2.0 } else { 1.0 };
                p.rect_stroke(rect, 6.0, Stroke::new(stroke_w, border_col), StrokeKind::Inside);

                if is_selected {
                    p.rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(45, 130, 245, 20));

                    // Corner checkmark badge
                    let chk_r = Rect::from_center_size(pos2(thumb_rect.max.x - 14.0, thumb_rect.min.y + 14.0), vec2(18.0, 18.0));
                    p.rect_filled(chk_r, 9.0, Color32::from_rgb(45, 130, 245));
                    let p1 = pos2(chk_r.min.x + 4.5, chk_r.center().y);
                    let p2 = pos2(chk_r.min.x + 7.5, chk_r.max.y - 5.0);
                    let p3 = pos2(chk_r.max.x - 4.5, chk_r.min.y + 5.0);
                    p.line_segment([p1, p2], Stroke::new(2.0, Color32::WHITE));
                    p.line_segment([p2, p3], Stroke::new(2.0, Color32::WHITE));
                }

                // Drag to folder
                if resp.drag_started() {
                    if is_selected {
                        app.ui.project_browser.dragging_projects = Some(app.ui.project_browser.get_all_selected());
                    } else {
                        app.ui.project_browser.select_single(prj.path.clone());
                        app.ui.project_browser.dragging_projects = Some(vec![prj.path.clone()]);
                    }
                }

                // Click and Multi-Selection
                if resp.clicked() {
                    let is_ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                    let is_shift = ui.input(|i| i.modifiers.shift);
                    if is_ctrl {
                        app.ui.project_browser.toggle_selection(prj.path.clone());
                    } else if is_shift {
                        app.ui.project_browser.select_range(prj.path.clone(), &all_paths);
                    } else {
                        app.ui.project_browser.select_single(prj.path.clone());
                    }
                }
                if resp.secondary_clicked() && !is_selected {
                    app.ui.project_browser.select_single(prj.path.clone());
                }
                if resp.double_clicked() {
                    project_to_open = Some(prj.clone());
                }

                // Context Menu
                let path_clone = prj.path.clone();
                let name_clone = prj.name.clone();
                let is_demo_val = prj.is_demo;
                let folders_list = app.ui.project_browser.folders.clone();
                let sel_count = app.ui.project_browser.selected_count();
                let all_selected = app.ui.project_browser.get_all_selected();
                let is_in_multisel = sel_count > 1 && all_selected.contains(&path_clone);

                resp.context_menu(|ui| {
                    if is_in_multisel {
                        ui.label(format!("{sel_count} proyectos seleccionados"));
                        ui.separator();
                        ui.menu_button("Mover seleccionados a carpeta", |ui| {
                            if ui.button("(Sin carpeta)").clicked() {
                                move_projects_to_folder = Some((all_selected.clone(), String::new()));
                                ui.close();
                            }
                            for f in &folders_list {
                                if ui.button(f).clicked() {
                                    move_projects_to_folder = Some((all_selected.clone(), f.clone()));
                                    ui.close();
                                }
                            }
                        });
                        let storage_dir = projects_storage_dir();
                        let any_not_in_storage = all_selected.iter().any(|p| !Path::new(p).starts_with(&storage_dir));
                        if any_not_in_storage && ui.button("Mover a carpeta de proyectos").clicked() {
                            move_projects_to_storage = Some(all_selected.clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(format!("Eliminar {sel_count} proyectos de la lista")).clicked() {
                            projects_to_delete = Some(all_selected.clone());
                            ui.close();
                        }
                    } else {
                        if ui.button("Abrir proyecto").clicked() {
                            project_to_open = Some(prj.clone());
                            ui.close();
                        }
                        if !is_demo_val {
                            if ui.button("Renombrar").clicked() {
                                project_to_rename = Some((path_clone.clone(), name_clone.clone()));
                                ui.close();
                            }
                            if ui.button("Duplicar").clicked() {
                                duplicate_project_file(&path_clone);
                                ui.close();
                            }
                            ui.menu_button("Mover a carpeta", |ui| {
                                if ui.button("(Sin carpeta)").clicked() {
                                    move_projects_to_folder = Some((vec![path_clone.clone()], String::new()));
                                    ui.close();
                                }
                                for f in &folders_list {
                                    if ui.button(f).clicked() {
                                        move_projects_to_folder = Some((vec![path_clone.clone()], f.clone()));
                                        ui.close();
                                    }
                                }
                            });
                            let storage_dir = projects_storage_dir();
                            let is_in_storage = Path::new(&path_clone).starts_with(&storage_dir);
                            if !is_in_storage {
                                if ui.button("Mover a carpeta de proyectos").clicked() {
                                    move_projects_to_storage = Some(vec![path_clone.clone()]);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("Eliminar de la lista").clicked() {
                                projects_to_delete = Some(vec![path_clone.clone()]);
                                ui.close();
                            }
                        }
                    }
                });

                col += 1;
                if col >= cols {
                    ui.end_row();
                    col = 0;
                }
            }

            // Handle actions triggered inside loops
            if let Some(prj) = project_to_open {
                open_project_entry(app, &prj);
            }
            if let Some((path, name)) = project_to_rename {
                app.ui.project_browser.rename_modal = Some(path);
                app.ui.project_browser.modal_input = name;
            }
            if let Some(paths) = move_projects_to_storage {
                for path in paths {
                    move_project_to_storage(&path, app);
                }
            }
            if let Some((paths, f)) = move_projects_to_folder {
                for path in paths {
                    if f.is_empty() {
                        app.ui.project_browser.project_folders.remove(&path);
                    } else {
                        app.ui.project_browser.project_folders.insert(path, f.clone());
                    }
                }
                save_persisted_manager(app);
            }
            if let Some(paths) = projects_to_delete {
                let set: HashSet<String> = paths.into_iter().collect();
                app.session.prefs.general.recent_projects.retain(|p| !set.contains(p));
                for p in &set {
                    app.ui.project_browser.project_folders.remove(p);
                    app.ui.project_browser.selected_paths.remove(p);
                }
                if let Some(sp) = &app.ui.project_browser.selected_path && set.contains(sp) {
                    app.ui.project_browser.selected_path = None;
                }
                save_persisted_manager(app);
            }
        });

    if let Some(open_f) = folder_to_open {
        app.ui.project_browser.navigate_to(Some(open_f));
    }
    if let Some(rf) = folder_to_rename {
        app.ui.project_browser.rename_modal = Some(rf.clone());
        app.ui.project_browser.modal_input = rf;
    }
    if let Some(del_f) = folder_to_delete {
        app.ui.project_browser.folders.retain(|f| f != &del_f);
        if app.ui.project_browser.current_folder.as_deref() == Some(&del_f) {
            app.ui.project_browser.navigate_to(None);
        }
        app.ui.project_browser.project_folders.retain(|_, f| f != &del_f);
        save_persisted_manager(app);
    }
}

fn render_action_card(
    ui: &mut Ui,
    card_w: f32,
    card_h: f32,
    icon: Icon,
    title: &str,
    sub: &str,
    on_click: impl FnOnce(),
) {
    let (rect, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
    let p = ui.painter().clone();
    let is_hovered = resp.hovered();

    let stroke_col = if is_hovered {
        Color32::from_rgb(70, 140, 240)
    } else {
        Color32::from_rgb(45, 52, 64)
    };
    p.rect_filled(rect, 6.0, if is_hovered { Color32::from_rgb(26, 32, 42) } else { Color32::from_rgb(22, 25, 30) });
    p.rect_stroke(rect, 6.0, Stroke::new(1.0, stroke_col), StrokeKind::Inside);

    let center = rect.center();
    let icon_rect = Rect::from_center_size(pos2(center.x, center.y - 18.0), vec2(28.0, 28.0));
    icons::paint(&p, icon_rect, icon, if is_hovered { Color32::WHITE } else { Color32::from_rgb(140, 170, 220) });
    p.text(pos2(center.x, center.y + 12.0), Align2::CENTER_CENTER, title, Tokens::semibold(12.5), Color32::WHITE);
    p.text(pos2(center.x, center.y + 28.0), Align2::CENTER_CENTER, sub, Tokens::ui(10.0), Color32::from_rgb(120, 130, 145));

    if resp.clicked() {
        on_click();
    }
}

fn render_list_view(
    app: &mut FilmcraftApp,
    ui: &mut Ui,
    projects: &[ProjectCard],
    visible_folders: &[String],
    folder_counts: &HashMap<String, usize>,
) {
    let row_h = 32.0;
    let header_h = 26.0;

    let avail_w = ui.available_width();
    let col_name_w = (avail_w * 0.35).max(180.0);
    let col_folder_w = 120.0;
    let col_time_w = 140.0;
    let col_size_w = 90.0;
    let _col_path_w = (avail_w - col_name_w - col_folder_w - col_time_w - col_size_w).max(120.0);

    // Header Row
    let (h_rect, _) = ui.allocate_exact_size(vec2(avail_w, header_h), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(h_rect, 0.0, Color32::from_rgb(24, 28, 34));
    p.line_segment([h_rect.left_bottom(), h_rect.right_bottom()], Stroke::new(1.0, Color32::from_rgb(38, 44, 52)));

    let mut hx = h_rect.min.x + 12.0;
    p.text(pos2(hx, h_rect.center().y), Align2::LEFT_CENTER, "NOMBRE", Tokens::semibold(10.0), Color32::from_rgb(130, 140, 155));
    hx += col_name_w;
    p.text(pos2(hx, h_rect.center().y), Align2::LEFT_CENTER, "CARPETA", Tokens::semibold(10.0), Color32::from_rgb(130, 140, 155));
    hx += col_folder_w;
    p.text(pos2(hx, h_rect.center().y), Align2::LEFT_CENTER, "MODIFICADO", Tokens::semibold(10.0), Color32::from_rgb(130, 140, 155));
    hx += col_time_w;
    p.text(pos2(hx, h_rect.center().y), Align2::LEFT_CENTER, "TAMAÑO", Tokens::semibold(10.0), Color32::from_rgb(130, 140, 155));
    hx += col_size_w;
    p.text(pos2(hx, h_rect.center().y), Align2::LEFT_CENTER, "UBICACIÓN", Tokens::semibold(10.0), Color32::from_rgb(130, 140, 155));

    let all_paths: Vec<String> = projects.iter().map(|p| p.path.clone()).collect();
    let mut folder_to_open = None;
    let mut folder_to_rename = None;
    let mut folder_to_delete = None;
    let mut project_to_open = None;
    let mut project_to_rename = None;
    let mut projects_to_delete: Option<Vec<String>> = None;
    let mut move_projects_to_folder: Option<(Vec<String>, String)> = None;
    let mut move_projects_to_storage: Option<Vec<String>> = None;

    // Up Directory Row (if inside a folder)
    if app.ui.project_browser.current_folder.is_some() {
        let (row_r, resp) = ui.allocate_exact_size(vec2(avail_w, row_h), Sense::click());
        let is_dragging = app.ui.project_browser.dragging_projects.is_some();
        let drop_on_up = is_dragging && resp.hovered();
        let is_hovered = resp.hovered();

        let bg = if drop_on_up {
            Color32::from_rgb(35, 75, 125)
        } else if is_hovered {
            Color32::from_rgb(28, 34, 42)
        } else {
            Color32::from_rgb(20, 22, 26)
        };
        p.rect_filled(row_r, 0.0, bg);
        if drop_on_up {
            p.rect_stroke(row_r, 0.0, Stroke::new(1.5, Color32::from_rgb(80, 170, 255)), StrokeKind::Inside);
            if ui.input(|i| i.pointer.any_released()) {
                if let Some(paths) = app.ui.project_browser.dragging_projects.take() {
                    for path in paths {
                        app.ui.project_browser.project_folders.remove(&path);
                    }
                    save_persisted_manager(app);
                }
            }
        }

        let mut rx = row_r.min.x + 12.0;
        icons::paint(&p, Rect::from_center_size(pos2(rx + 6.0, row_r.center().y), vec2(14.0, 14.0)), Icon::ArrowUp, Color32::from_rgb(80, 160, 255));
        p.text(pos2(rx + 20.0, row_r.center().y), Align2::LEFT_CENTER, ".. (Subir un nivel)", Tokens::semibold(11.5), Color32::WHITE);
        rx += col_name_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, "Carpeta superior", Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_folder_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, "-", Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_time_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, "-", Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_size_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, "Volver a Proyectos", Tokens::ui(10.0), Color32::from_rgb(100, 110, 125));

        if resp.clicked() {
            app.ui.project_browser.go_up();
        }
    }

    // Visible Folders Rows
    for (i, f) in visible_folders.iter().enumerate() {
        let (row_r, resp) = ui.allocate_exact_size(vec2(avail_w, row_h), Sense::click());
        let is_dragging = app.ui.project_browser.dragging_projects.is_some();
        let drop_on_f = is_dragging && resp.hovered();
        let is_hovered = resp.hovered();

        let bg = if drop_on_f {
            Color32::from_rgb(35, 75, 125)
        } else if is_hovered {
            Color32::from_rgb(28, 34, 42)
        } else if i % 2 == 0 {
            Color32::from_rgb(20, 22, 26)
        } else {
            Color32::from_rgb(22, 25, 30)
        };
        p.rect_filled(row_r, 0.0, bg);
        if drop_on_f {
            p.rect_stroke(row_r, 0.0, Stroke::new(1.5, Color32::from_rgb(80, 170, 255)), StrokeKind::Inside);
            if ui.input(|i| i.pointer.any_released()) {
                if let Some(paths) = app.ui.project_browser.dragging_projects.take() {
                    for path in paths {
                        app.ui.project_browser.project_folders.insert(path, f.clone());
                    }
                    save_persisted_manager(app);
                }
            }
        }

        let mut rx = row_r.min.x + 12.0;
        let f_icon_color = if drop_on_f {
            Color32::from_rgb(100, 190, 255)
        } else if resp.hovered() {
            Color32::from_rgb(255, 205, 75)
        } else {
            Color32::from_rgb(240, 180, 50)
        };
        icons::paint(&p, Rect::from_center_size(pos2(rx + 6.0, row_r.center().y), vec2(14.0, 14.0)), Icon::Folder, f_icon_color);
        p.text(pos2(rx + 20.0, row_r.center().y), Align2::LEFT_CENTER, f, Tokens::semibold(11.5), Color32::WHITE);
        rx += col_name_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, "Carpeta de proyectos", Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_folder_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, "-", Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_time_w;

        let count = folder_counts.get(f).copied().unwrap_or(0);
        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, format!("{count} proyectos"), Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_size_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, f, Tokens::ui(10.0), Color32::from_rgb(100, 110, 125));

        if resp.double_clicked() {
            folder_to_open = Some(f.clone());
        }

        resp.context_menu(|ui| {
            if ui.button("Abrir carpeta").clicked() {
                folder_to_open = Some(f.clone());
                ui.close();
            }
            if ui.button("Renombrar...").clicked() {
                folder_to_rename = Some(f.clone());
                ui.close();
            }
            if ui.button("Eliminar carpeta").clicked() {
                folder_to_delete = Some(f.clone());
                ui.close();
            }
        });
    }

    for (i, prj) in projects.iter().enumerate() {
        let is_selected = app.ui.project_browser.is_selected(&prj.path);
        let (row_r, resp) = ui.allocate_exact_size(vec2(avail_w, row_h), Sense::click_and_drag());
        let is_hovered = resp.hovered();

        let bg = if is_selected {
            Color32::from_rgb(30, 52, 85)
        } else if is_hovered {
            Color32::from_rgb(28, 34, 42)
        } else if i % 2 == 0 {
            Color32::from_rgb(20, 22, 26)
        } else {
            Color32::from_rgb(22, 25, 30)
        };
        p.rect_filled(row_r, 0.0, bg);
        if is_selected {
            p.rect_stroke(row_r, 0.0, Stroke::new(1.0, Color32::from_rgb(50, 140, 250)), StrokeKind::Inside);
            p.rect_filled(Rect::from_min_size(row_r.min, vec2(3.0, row_r.height())), 0.0, Color32::from_rgb(50, 140, 250));
        }

        let mut rx = row_r.min.x + 12.0;
        icons::paint(&p, Rect::from_center_size(pos2(rx + 6.0, row_r.center().y), vec2(13.0, 13.0)), if prj.is_demo { Icon::Sequence } else { Icon::Film }, if is_selected { Color32::WHITE } else { Color32::from_rgb(140, 170, 220) });
        p.text(pos2(rx + 20.0, row_r.center().y), Align2::LEFT_CENTER, &prj.name, Tokens::semibold(11.5), if is_selected { Color32::WHITE } else { Color32::from_rgb(220, 225, 235) });
        rx += col_name_w;

        let folder_str = prj.folder.as_deref().unwrap_or("-");
        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, folder_str, Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_folder_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, format_modified(prj.modified), Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_time_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, format_size(prj.size_bytes), Tokens::ui(11.0), Color32::from_rgb(140, 150, 165));
        rx += col_size_w;

        p.text(pos2(rx, row_r.center().y), Align2::LEFT_CENTER, &prj.path, Tokens::ui(10.0), Color32::from_rgb(100, 110, 125));

        // Drag to folder
        if resp.drag_started() {
            if is_selected {
                app.ui.project_browser.dragging_projects = Some(app.ui.project_browser.get_all_selected());
            } else {
                app.ui.project_browser.select_single(prj.path.clone());
                app.ui.project_browser.dragging_projects = Some(vec![prj.path.clone()]);
            }
        }

        // Click and Multi-Selection
        if resp.clicked() {
            let is_ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
            let is_shift = ui.input(|i| i.modifiers.shift);
            if is_ctrl {
                app.ui.project_browser.toggle_selection(prj.path.clone());
            } else if is_shift {
                app.ui.project_browser.select_range(prj.path.clone(), &all_paths);
            } else {
                app.ui.project_browser.select_single(prj.path.clone());
            }
        }
        if resp.secondary_clicked() && !is_selected {
            app.ui.project_browser.select_single(prj.path.clone());
        }
        if resp.double_clicked() {
            project_to_open = Some(prj.clone());
        }

        // Context menu in list view
        let path_clone = prj.path.clone();
        let name_clone = prj.name.clone();
        let is_demo_val = prj.is_demo;
        let folders_list = app.ui.project_browser.folders.clone();
        let sel_count = app.ui.project_browser.selected_count();
        let all_selected = app.ui.project_browser.get_all_selected();
        let is_in_multisel = sel_count > 1 && all_selected.contains(&path_clone);

        resp.context_menu(|ui| {
            if is_in_multisel {
                ui.label(format!("{sel_count} proyectos seleccionados"));
                ui.separator();
                ui.menu_button("Mover seleccionados a carpeta", |ui| {
                    if ui.button("(Sin carpeta)").clicked() {
                        move_projects_to_folder = Some((all_selected.clone(), String::new()));
                        ui.close();
                    }
                    for f in &folders_list {
                        if ui.button(f).clicked() {
                            move_projects_to_folder = Some((all_selected.clone(), f.clone()));
                            ui.close();
                        }
                    }
                });
                let storage_dir = projects_storage_dir();
                let any_not_in_storage = all_selected.iter().any(|p| !Path::new(p).starts_with(&storage_dir));
                if any_not_in_storage && ui.button("Mover a carpeta de proyectos").clicked() {
                    move_projects_to_storage = Some(all_selected.clone());
                    ui.close();
                }
                ui.separator();
                if ui.button(format!("Eliminar {sel_count} proyectos de la lista")).clicked() {
                    projects_to_delete = Some(all_selected.clone());
                    ui.close();
                }
            } else {
                if ui.button("Abrir proyecto").clicked() {
                    project_to_open = Some(prj.clone());
                    ui.close();
                }
                if !is_demo_val {
                    if ui.button("Renombrar").clicked() {
                        project_to_rename = Some((path_clone.clone(), name_clone.clone()));
                        ui.close();
                    }
                    if ui.button("Duplicar").clicked() {
                        duplicate_project_file(&path_clone);
                        ui.close();
                    }
                    ui.menu_button("Mover a carpeta", |ui| {
                        if ui.button("(Sin carpeta)").clicked() {
                            move_projects_to_folder = Some((vec![path_clone.clone()], String::new()));
                            ui.close();
                        }
                        for f in &folders_list {
                            if ui.button(f).clicked() {
                                move_projects_to_folder = Some((vec![path_clone.clone()], f.clone()));
                                ui.close();
                            }
                        }
                    });
                    let storage_dir = projects_storage_dir();
                    let is_in_storage = Path::new(&path_clone).starts_with(&storage_dir);
                    if !is_in_storage {
                        if ui.button("Mover a carpeta de proyectos").clicked() {
                            move_projects_to_storage = Some(vec![path_clone.clone()]);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Eliminar de la lista").clicked() {
                        projects_to_delete = Some(vec![path_clone.clone()]);
                        ui.close();
                    }
                }
            }
        });
    }

    if let Some(prj) = project_to_open {
        open_project_entry(app, &prj);
    }
    if let Some((path, name)) = project_to_rename {
        app.ui.project_browser.rename_modal = Some(path);
        app.ui.project_browser.modal_input = name;
    }
    if let Some(paths) = move_projects_to_storage {
        for path in paths {
            move_project_to_storage(&path, app);
        }
    }
    if let Some((paths, f)) = move_projects_to_folder {
        for path in paths {
            if f.is_empty() {
                app.ui.project_browser.project_folders.remove(&path);
            } else {
                app.ui.project_browser.project_folders.insert(path, f.clone());
            }
        }
        save_persisted_manager(app);
    }
    if let Some(paths) = projects_to_delete {
        let set: HashSet<String> = paths.into_iter().collect();
        app.session.prefs.general.recent_projects.retain(|p| !set.contains(p));
        for p in &set {
            app.ui.project_browser.project_folders.remove(p);
            app.ui.project_browser.selected_paths.remove(p);
        }
        if let Some(sp) = &app.ui.project_browser.selected_path && set.contains(sp) {
            app.ui.project_browser.selected_path = None;
        }
        save_persisted_manager(app);
    }

    if let Some(open_f) = folder_to_open {
        app.ui.project_browser.navigate_to(Some(open_f));
    }
    if let Some(rf) = folder_to_rename {
        app.ui.project_browser.rename_modal = Some(rf.clone());
        app.ui.project_browser.modal_input = rf;
    }
    if let Some(del_f) = folder_to_delete {
        app.ui.project_browser.folders.retain(|f| f != &del_f);
        if app.ui.project_browser.current_folder.as_deref() == Some(&del_f) {
            app.ui.project_browser.navigate_to(None);
        }
        app.ui.project_browser.project_folders.retain(|_, f| f != &del_f);
        save_persisted_manager(app);
    }
}

fn render_bottom_bar(app: &mut FilmcraftApp, ui: &mut Ui, rect: Rect, count: usize) {
    let p = ui.painter().clone();
    let t = app.tokens;

    p.rect_filled(rect, 0.0, Color32::from_rgb(22, 25, 30));
    p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, Color32::from_rgb(36, 40, 48)));

    // Left Action Buttons
    let mut x = rect.min.x + 16.0;

    let f_btn_w = 120.0;
    let f_r = Rect::from_min_size(pos2(x, rect.center().y - 13.0), vec2(f_btn_w, 26.0));
    let f_resp = ui.interact(f_r, Id::unique("pm-bot-folder"), Sense::click());
    p.rect_filled(f_r, 4.0, if f_resp.hovered() { t.hover } else { Color32::from_rgb(30, 35, 42) });
    p.text(f_r.center(), Align2::CENTER_CENTER, "Nueva Carpeta", Tokens::ui(11.0), t.text);
    if f_resp.clicked() {
        app.ui.project_browser.new_folder_modal = true;
        app.ui.project_browser.modal_input.clear();
    }
    x += f_btn_w + 10.0;

    let p_btn_w = 120.0;
    let p_r = Rect::from_min_size(pos2(x, rect.center().y - 13.0), vec2(p_btn_w, 26.0));
    let p_resp = ui.interact(p_r, Id::unique("pm-bot-proj"), Sense::click());
    p.rect_filled(p_r, 4.0, if p_resp.hovered() { Color32::from_rgb(45, 125, 235) } else { Color32::from_rgb(35, 105, 210) });
    p.text(p_r.center(), Align2::CENTER_CENTER, "Nuevo Proyecto", Tokens::semibold(11.5), Color32::WHITE);
    if p_resp.clicked() {
        app.ui.project_browser.new_project_modal = true;
        app.ui.project_browser.modal_input = "Sin título".into();
    }

    // Center info
    let sel_count = app.ui.project_browser.selected_count();
    let info = if sel_count > 1 {
        format!("Mostrando {count} proyectos · {sel_count} seleccionados")
    } else {
        format!("Mostrando {count} proyectos")
    };
    p.text(rect.center(), Align2::CENTER_CENTER, info, Tokens::ui(11.0), Color32::from_rgb(120, 130, 145));

    // Right Action Buttons
    let mut rx = rect.max.x - 16.0;

    // Open Selected Project (first selected)
    let has_sel = sel_count > 0;
    let open_w = 100.0;
    rx -= open_w;
    let open_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(open_w, 26.0));
    let open_resp = ui.interact(open_r, Id::unique("pm-bot-open"), Sense::click());
    let open_bg = if has_sel {
        if open_resp.hovered() { Color32::from_rgb(45, 125, 235) } else { Color32::from_rgb(35, 105, 210) }
    } else {
        Color32::from_rgb(28, 32, 40)
    };
    p.rect_filled(open_r, 4.0, open_bg);
    p.text(open_r.center(), Align2::CENTER_CENTER, "Abrir", Tokens::semibold(11.5), if has_sel { Color32::WHITE } else { t.text_faint });
    if has_sel && open_resp.clicked() {
        let first_path = app.ui.project_browser.selected_path.clone().or_else(|| app.ui.project_browser.selected_paths.iter().next().cloned());
        if let Some(path) = first_path {
            let prj_card = ProjectCard {
                name: Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                path: path.clone(),
                modified: SystemTime::now(),
                size_bytes: 0,
                folder: None,
                is_current: false,
                is_demo: path == "builtin:demo",
            };
            open_project_entry(app, &prj_card);
        }
    }

    // If multiple selected, show Delete button on bottom bar
    if sel_count > 1 {
        rx -= 10.0;
        let del_w = 130.0;
        rx -= del_w;
        let del_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(del_w, 26.0));
        let del_resp = ui.interact(del_r, Id::unique("pm-bot-del-multi"), Sense::click());
        let del_bg = if del_resp.hovered() { Color32::from_rgb(175, 45, 50) } else { Color32::from_rgb(120, 32, 38) };
        p.rect_filled(del_r, 4.0, del_bg);
        p.text(del_r.center(), Align2::CENTER_CENTER, format!("Eliminar ({sel_count})"), Tokens::semibold(11.0), Color32::WHITE);
        if del_resp.clicked() {
            let all_sel = app.ui.project_browser.get_all_selected();
            let set: HashSet<String> = all_sel.into_iter().collect();
            app.session.prefs.general.recent_projects.retain(|p| !set.contains(p));
            for p in &set {
                app.ui.project_browser.project_folders.remove(p);
                app.ui.project_browser.selected_paths.remove(p);
            }
            if let Some(sp) = &app.ui.project_browser.selected_path && set.contains(sp) {
                app.ui.project_browser.selected_path = None;
            }
            save_persisted_manager(app);
        }
    }

    rx -= 12.0;
    let imp_w = 140.0;
    rx -= imp_w;
    let imp_r = Rect::from_min_size(pos2(rx, rect.center().y - 13.0), vec2(imp_w, 26.0));
    let imp_resp = ui.interact(imp_r, Id::unique("pm-bot-import"), Sense::click());
    p.rect_filled(imp_r, 4.0, if imp_resp.hovered() { t.hover } else { Color32::from_rgb(30, 35, 42) });
    p.text(imp_r.center(), Align2::CENTER_CENTER, "Importar Proyecto...", Tokens::ui(11.0), t.text);
    if imp_resp.clicked() {
        let ctx = ui.ctx().clone();
        if crate::menus::invoke(app, &ctx, "file.open", json!({})).is_ok() {
            app.ui.mode = Mode::Edit;
        }
    }
}
fn render_modals(app: &mut FilmcraftApp, ui: &mut Ui, screen_rect: Rect) {
    let p = ui.painter().clone();

    // 1. New Project Modal
    if app.ui.project_browser.new_project_modal {
        p.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(150));

        let modal_w = 360.0;
        let modal_h = 160.0;
        let m_rect = Rect::from_center_size(screen_rect.center(), vec2(modal_w, modal_h));
        p.rect_filled(m_rect, 8.0, Color32::from_rgb(26, 30, 36));
        p.rect_stroke(m_rect, 8.0, Stroke::new(1.0, Color32::from_rgb(50, 60, 75)), StrokeKind::Inside);

        p.text(pos2(m_rect.min.x + 20.0, m_rect.min.y + 24.0), Align2::LEFT_CENTER, "Crear Nuevo Proyecto", Tokens::semibold(14.0), Color32::WHITE);
        p.text(pos2(m_rect.min.x + 20.0, m_rect.min.y + 54.0), Align2::LEFT_CENTER, "Nombre del proyecto:", Tokens::ui(11.0), Color32::from_rgb(160, 170, 185));

        let input_r = Rect::from_min_size(pos2(m_rect.min.x + 20.0, m_rect.min.y + 70.0), vec2(modal_w - 40.0, 28.0));
        let mut m_ui = ui.new_child(egui::UiBuilder::new().max_rect(input_r));
        let mut text = app.ui.project_browser.modal_input.clone();
        let edit = TextEdit::singleline(&mut text);
        let resp = m_ui.add(edit);
        app.ui.project_browser.modal_input = text;
        resp.request_focus();

        let mut create = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

        // Buttons
        let btn_y = m_rect.max.y - 36.0;
        let can_r = Rect::from_min_size(pos2(m_rect.max.x - 170.0, btn_y), vec2(70.0, 26.0));
        let can_resp = ui.interact(can_r, Id::unique("pm-modal-proj-cancel"), Sense::click());
        p.rect_filled(can_r, 4.0, if can_resp.hovered() { Color32::from_rgb(45, 52, 62) } else { Color32::from_rgb(34, 40, 48) });
        p.text(can_r.center(), Align2::CENTER_CENTER, "Cancelar", Tokens::ui(11.0), Color32::WHITE);
        if can_resp.clicked() || cancel {
            app.ui.project_browser.new_project_modal = false;
        }

        let cr_r = Rect::from_min_size(pos2(m_rect.max.x - 90.0, btn_y), vec2(70.0, 26.0));
        let cr_resp = ui.interact(cr_r, Id::unique("pm-modal-proj-create"), Sense::click());
        p.rect_filled(cr_r, 4.0, if cr_resp.hovered() { Color32::from_rgb(45, 125, 235) } else { Color32::from_rgb(35, 105, 210) });
        p.text(cr_r.center(), Align2::CENTER_CENTER, "Crear", Tokens::semibold(11.0), Color32::WHITE);
        if cr_resp.clicked() {
            create = true;
        }

        if create {
            let name = app.ui.project_browser.modal_input.trim();
            if !name.is_empty() {
                let _ = app.session.execute("file.newProject", json!({ "name": name }));
                let storage = projects_storage_dir();
                let dir = if let Some(cf) = &app.ui.project_browser.current_folder {
                    storage.join(cf)
                } else {
                    storage
                };
                let _ = fs::create_dir_all(&dir);
                let path = dir.join(format!("{name}.fcproj"));
                app.session.path = Some(path.to_string_lossy().to_string());
                let _ = app.session.execute("file.save", json!({}));
                app.ui.mode = Mode::Edit;
            }
            app.ui.project_browser.new_project_modal = false;
        }
    }

    // 2. New Folder Modal
    if app.ui.project_browser.new_folder_modal {
        p.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(150));

        let modal_w = 340.0;
        let modal_h = 150.0;
        let m_rect = Rect::from_center_size(screen_rect.center(), vec2(modal_w, modal_h));
        p.rect_filled(m_rect, 8.0, Color32::from_rgb(26, 30, 36));
        p.rect_stroke(m_rect, 8.0, Stroke::new(1.0, Color32::from_rgb(50, 60, 75)), StrokeKind::Inside);

        p.text(pos2(m_rect.min.x + 20.0, m_rect.min.y + 24.0), Align2::LEFT_CENTER, "Nueva Carpeta", Tokens::semibold(14.0), Color32::WHITE);
        p.text(pos2(m_rect.min.x + 20.0, m_rect.min.y + 52.0), Align2::LEFT_CENTER, "Nombre de la carpeta:", Tokens::ui(11.0), Color32::from_rgb(160, 170, 185));

        let input_r = Rect::from_min_size(pos2(m_rect.min.x + 20.0, m_rect.min.y + 68.0), vec2(modal_w - 40.0, 28.0));
        let mut m_ui = ui.new_child(egui::UiBuilder::new().max_rect(input_r));
        let mut text = app.ui.project_browser.modal_input.clone();
        let edit = TextEdit::singleline(&mut text);
        let resp = m_ui.add(edit);
        app.ui.project_browser.modal_input = text;
        resp.request_focus();

        let mut create = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

        let btn_y = m_rect.max.y - 36.0;
        let can_r = Rect::from_min_size(pos2(m_rect.max.x - 170.0, btn_y), vec2(70.0, 26.0));
        let can_resp = ui.interact(can_r, Id::unique("pm-modal-f-cancel"), Sense::click());
        p.rect_filled(can_r, 4.0, if can_resp.hovered() { Color32::from_rgb(45, 52, 62) } else { Color32::from_rgb(34, 40, 48) });
        p.text(can_r.center(), Align2::CENTER_CENTER, "Cancelar", Tokens::ui(11.0), Color32::WHITE);
        if can_resp.clicked() || cancel {
            app.ui.project_browser.new_folder_modal = false;
        }

        let cr_r = Rect::from_min_size(pos2(m_rect.max.x - 90.0, btn_y), vec2(70.0, 26.0));
        let cr_resp = ui.interact(cr_r, Id::unique("pm-modal-f-create"), Sense::click());
        p.rect_filled(cr_r, 4.0, if cr_resp.hovered() { Color32::from_rgb(45, 125, 235) } else { Color32::from_rgb(35, 105, 210) });
        p.text(cr_r.center(), Align2::CENTER_CENTER, "Crear", Tokens::semibold(11.0), Color32::WHITE);
        if cr_resp.clicked() {
            create = true;
        }

        if create {
            let f_name = app.ui.project_browser.modal_input.trim();
            if !f_name.is_empty() && !app.ui.project_browser.folders.contains(&f_name.to_string()) {
                app.ui.project_browser.folders.push(f_name.to_string());
                let _ = fs::create_dir_all(projects_storage_dir().join(f_name));
                save_persisted_manager(app);
            }
            app.ui.project_browser.new_folder_modal = false;
        }
    }

    // 3. Rename Modal
    if let Some(target_path) = app.ui.project_browser.rename_modal.clone() {
        p.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(150));

        let modal_w = 340.0;
        let modal_h = 150.0;
        let m_rect = Rect::from_center_size(screen_rect.center(), vec2(modal_w, modal_h));
        p.rect_filled(m_rect, 8.0, Color32::from_rgb(26, 30, 36));
        p.rect_stroke(m_rect, 8.0, Stroke::new(1.0, Color32::from_rgb(50, 60, 75)), StrokeKind::Inside);

        p.text(pos2(m_rect.min.x + 20.0, m_rect.min.y + 24.0), Align2::LEFT_CENTER, "Renombrar Proyecto", Tokens::semibold(14.0), Color32::WHITE);
        p.text(pos2(m_rect.min.x + 20.0, m_rect.min.y + 52.0), Align2::LEFT_CENTER, "Nuevo nombre:", Tokens::ui(11.0), Color32::from_rgb(160, 170, 185));

        let input_r = Rect::from_min_size(pos2(m_rect.min.x + 20.0, m_rect.min.y + 68.0), vec2(modal_w - 40.0, 28.0));
        let mut m_ui = ui.new_child(egui::UiBuilder::new().max_rect(input_r));
        let mut text = app.ui.project_browser.modal_input.clone();
        let edit = TextEdit::singleline(&mut text);
        let resp = m_ui.add(edit);
        app.ui.project_browser.modal_input = text;
        resp.request_focus();

        let mut save = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

        let btn_y = m_rect.max.y - 36.0;
        let can_r = Rect::from_min_size(pos2(m_rect.max.x - 170.0, btn_y), vec2(70.0, 26.0));
        let can_resp = ui.interact(can_r, Id::unique("pm-modal-ren-cancel"), Sense::click());
        p.rect_filled(can_r, 4.0, if can_resp.hovered() { Color32::from_rgb(45, 52, 62) } else { Color32::from_rgb(34, 40, 48) });
        p.text(can_r.center(), Align2::CENTER_CENTER, "Cancelar", Tokens::ui(11.0), Color32::WHITE);
        if can_resp.clicked() || cancel {
            app.ui.project_browser.rename_modal = None;
        }

        let sv_r = Rect::from_min_size(pos2(m_rect.max.x - 90.0, btn_y), vec2(70.0, 26.0));
        let sv_resp = ui.interact(sv_r, Id::unique("pm-modal-ren-save"), Sense::click());
        p.rect_filled(sv_r, 4.0, if sv_resp.hovered() { Color32::from_rgb(45, 125, 235) } else { Color32::from_rgb(35, 105, 210) });
        p.text(sv_r.center(), Align2::CENTER_CENTER, "Guardar", Tokens::semibold(11.0), Color32::WHITE);
        if sv_resp.clicked() {
            save = true;
        }

        if save {
            let new_name = app.ui.project_browser.modal_input.trim().to_string();
            if !new_name.is_empty() {
                rename_project_file(&target_path, &new_name, app);
            }
            app.ui.project_browser.rename_modal = None;
        }
    }
}

fn open_project_entry(app: &mut FilmcraftApp, prj: &ProjectCard) {
    if prj.is_demo {
        let _ = app.session.execute("file.openDemoProject", json!({}));
    } else if prj.path == "[En memoria]" {
        // already active in memory!
    } else {
        let _ = app.session.execute("file.open", json!({ "path": prj.path }));
    }
    app.ui.mode = Mode::Edit;
}

fn duplicate_project_file(orig_path: &str) {
    let p = Path::new(orig_path);
    if let Some(parent) = p.parent() {
        if let Some(stem) = p.file_stem() {
            let new_name = format!("{} Copia.fcproj", stem.to_string_lossy());
            let dest = parent.join(new_name);
            let _ = fs::copy(p, dest);
        }
    }
}

fn rename_project_file(orig_path: &str, new_name: &str, app: &mut FilmcraftApp) {
    let p = Path::new(orig_path);
    if let Some(parent) = p.parent() {
        let new_file_name = format!("{new_name}.fcproj");
        let dest = parent.join(new_file_name);
        if fs::rename(p, &dest).is_ok() {
            let dest_str = dest.to_string_lossy().to_string();
            for rec in &mut app.session.prefs.general.recent_projects {
                if rec == orig_path {
                    *rec = dest_str.clone();
                }
            }
            if let Some(folder) = app.ui.project_browser.project_folders.remove(orig_path) {
                app.ui.project_browser.project_folders.insert(dest_str, folder);
            }
            save_persisted_manager(app);
        }
    }
}

fn move_project_to_storage(orig_path: &str, app: &mut FilmcraftApp) {
    let p = Path::new(orig_path);
    if p.exists() {
        let storage = projects_storage_dir();
        let _ = fs::create_dir_all(&storage);
        if let Some(file_name) = p.file_name() {
            let dest = storage.join(file_name);
            if fs::copy(p, &dest).is_ok() {
                let dest_str = dest.to_string_lossy().to_string();
                for rec in &mut app.session.prefs.general.recent_projects {
                    if rec == orig_path {
                        *rec = dest_str.clone();
                    }
                }
                if let Some(folder) = app.ui.project_browser.project_folders.remove(orig_path) {
                    app.ui.project_browser.project_folders.insert(dest_str, folder);
                }
                save_persisted_manager(app);
            }
        }
    }
}
