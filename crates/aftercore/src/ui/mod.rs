//! Launcher UI (eframe / egui), fully custom-styled.

mod pages;
pub mod theme;
pub mod widgets;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ac_core::config::{Config, CpuMode, Priority, UiSettings};
use ac_core::hotkey::HotkeyAction;
use ac_core::process::ProcInfo;
use ac_overlay::fonts::FontEntry;
use ac_telemetry::etw::EtwState;
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, Layout, Margin, Rect, Sense, Stroke};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::core::Core;
use crate::Args;
use theme::{alpha, bold, body, icon, mix, Pal};
use widgets::ico;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Overlay,
    Cores,
    Profiles,
    Bench,
    Hotkeys,
    Settings,
}

impl Page {
    pub const ALL: [Page; 7] =
        [Page::Dashboard, Page::Overlay, Page::Cores, Page::Profiles, Page::Bench, Page::Hotkeys, Page::Settings];
    fn title(self) -> &'static str {
        match self {
            Page::Dashboard => "Обзор",
            Page::Overlay => "Оверлей",
            Page::Cores => "Ядра CPU",
            Page::Profiles => "Профили",
            Page::Bench => "Бенчмарк",
            Page::Hotkeys => "Бинды",
            Page::Settings => "Настройки",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Page::Dashboard => ico::HOME,
            Page::Overlay => ico::OVERLAY,
            Page::Cores => ico::CPU,
            Page::Profiles => ico::PROFILES,
            Page::Bench => ico::BENCH,
            Page::Hotkeys => ico::KEYBOARD,
            Page::Settings => ico::SETTINGS,
        }
    }
    fn from_arg(s: &str) -> Option<Self> {
        Some(match s {
            "dashboard" => Page::Dashboard,
            "overlay" => Page::Overlay,
            "cores" => Page::Cores,
            "profiles" => Page::Profiles,
            "bench" => Page::Bench,
            "hotkeys" => Page::Hotkeys,
            "settings" => Page::Settings,
            _ => return None,
        })
    }
}

pub struct OverlayState {
    pub renderer: ac_overlay::Renderer,
    pub tex: Option<egui::TextureHandle>,
    pub new_style_name: String,
    pub font_filter: String,
    pub edit_profile: String,
}

pub struct CoresState {
    pub sel: Vec<u32>,
    pub filter: String,
    pub pid: Option<u32>,
    pub mode: CpuMode,
    pub priority: Option<Priority>,
    pub set_name: String,
    pub procs: Vec<ProcInfo>,
    pub procs_at: Option<Instant>,
}

#[derive(Default)]
pub struct ProfilesState {
    pub rename: Option<(usize, String)>,
    pub new_exe: String,
    pub imports: Vec<std::path::PathBuf>,
}

#[derive(Default)]
pub struct SettingsState {
    pub font_filter: String,
    pub new_black: String,
    pub confirm_reset: bool,
}

pub struct App {
    pub core: Arc<Core>,
    pub cfg: Config,
    ext_gen: u64,
    pub pal: Pal,
    styled: Option<(UiSettings, String)>,
    pub page: Page,
    first_frame: bool,
    start_hidden: bool,
    dirty_since: Option<Instant>,
    _tray: Option<tray_icon::TrayIcon>,
    logo: Option<egui::TextureHandle>,
    pub ov: OverlayState,
    pub cores: CoresState,
    pub prof: ProfilesState,
    pub capturing: Option<HotkeyAction>,
    pub settings: SettingsState,
    pub fonts: Option<Vec<FontEntry>>,
    toasts: Vec<(String, Color32, Instant)>,
    screenshot: Option<String>,
    frame_no: u64,
    maximized: bool,
    started: Instant,
    shot_requested: bool,
    pub dash_locked: bool,
    pub dash_order: Vec<u32>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, core: Arc<Core>, start_hidden: bool, args: Args) -> Self {
        let _ = core.egui.set(cc.egui_ctx.clone());
        if let Ok(h) = cc.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                let hwnd = w.hwnd.get();
                core.hwnd.store(hwnd, Ordering::SeqCst);
                round_corners(hwnd);
            }
        }
        let cfg = core.cfg.read().clone();
        // Fonts must be registered before the first frame (styles reference the "bold" family).
        theme::setup_fonts(&cc.egui_ctx, &cfg.ui.font_path);
        let pal = Pal::new(&cfg.ui);
        theme::apply(&cc.egui_ctx, &pal, &cfg.ui);
        let tray = crate::tray::create(core.clone());
        let page = args.page.as_deref().and_then(Page::from_arg).unwrap_or(Page::Dashboard);
        let all = core.topo.all();
        Self {
            ext_gen: core.cfg_ext_gen.load(Ordering::SeqCst),
            pal,
            styled: Some((cfg.ui.clone(), cfg.ui.font_path.clone())),
            page,
            first_frame: true,
            start_hidden: start_hidden && args.screenshot.is_none(),
            dirty_since: None,
            _tray: tray,
            logo: None,
            ov: OverlayState {
                renderer: ac_overlay::Renderer::new(),
                tex: None,
                new_style_name: String::new(),
                font_filter: String::new(),
                edit_profile: cfg.active_profile.clone(),
            },
            cores: CoresState {
                sel: all,
                filter: String::new(),
                pid: None,
                mode: CpuMode::Affinity,
                priority: None,
                set_name: String::new(),
                procs: Vec::new(),
                procs_at: None,
            },
            prof: ProfilesState::default(),
            capturing: None,
            settings: SettingsState::default(),
            fonts: None,
            toasts: Vec::new(),
            screenshot: args.screenshot,
            frame_no: 0,
            maximized: false,
            started: Instant::now(),
            shot_requested: false,
            dash_locked: false,
            dash_order: Vec::new(),
            cfg,
            core,
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, color: Color32) {
        self.toasts.push((text.into(), color, Instant::now()));
    }

    pub fn fonts(&mut self) -> &Vec<FontEntry> {
        self.fonts.get_or_insert_with(|| {
            let dir = ac_core::paths::user_fonts_dir();
            ac_overlay::fonts::list_fonts(Some(&dir))
        })
    }

    fn sync_style(&mut self, ctx: &egui::Context) {
        let key = (self.cfg.ui.clone(), self.cfg.ui.font_path.clone());
        if self.styled.as_ref() == Some(&key) {
            return;
        }
        let font_changed = self.styled.as_ref().map(|s| s.1 != key.1).unwrap_or(true);
        if font_changed {
            theme::setup_fonts(ctx, &self.cfg.ui.font_path);
        }
        self.pal = Pal::new(&self.cfg.ui);
        theme::apply(ctx, &self.pal, &self.cfg.ui);
        self.styled = Some(key);
    }

    fn on_config_changed(&mut self, before: &Config) {
        *self.core.cfg.write() = self.cfg.clone();
        if before.hotkeys != self.cfg.hotkeys {
            self.core.hotkey_reload.store(true, Ordering::SeqCst);
        }
        if before.general.sensor_interval_ms != self.cfg.general.sensor_interval_ms {
            self.core.sensors.interval_ms.store(self.cfg.general.sensor_interval_ms, Ordering::Relaxed);
        }
        if before.game_rules != self.cfg.game_rules || before.core_sets != self.cfg.core_sets {
            self.core.forget_applied();
        }
        if before.general.autostart != self.cfg.general.autostart {
            match ac_core::process::set_autostart(self.cfg.general.autostart) {
                Ok(()) => {
                    let t = if self.cfg.general.autostart { "Автозапуск включён" } else { "Автозапуск выключен" };
                    let c = self.pal.good;
                    self.toast(t, c);
                }
                Err(e) => {
                    let c = self.pal.bad;
                    self.toast(format!("Автозапуск: {e}"), c);
                }
            }
        }
        self.dirty_since = Some(Instant::now());
    }

    fn handle_resize_edges(&self, ctx: &egui::Context) {
        if self.maximized {
            return;
        }
        let screen = ctx.screen_rect();
        let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let m = 6.0;
        let (l, r, t, b) =
            (p.x <= screen.left() + m, p.x >= screen.right() - m, p.y <= screen.top() + m, p.y >= screen.bottom() - m);
        use egui::viewport::ResizeDirection as D;
        let dir = match (l, r, t, b) {
            (true, _, true, _) => Some(D::NorthWest),
            (_, true, true, _) => Some(D::NorthEast),
            (true, _, _, true) => Some(D::SouthWest),
            (_, true, _, true) => Some(D::SouthEast),
            (true, _, _, _) => Some(D::West),
            (_, true, _, _) => Some(D::East),
            (_, _, true, _) => Some(D::North),
            (_, _, _, true) => Some(D::South),
            _ => None,
        };
        let Some(dir) = dir else { return };
        let cursor = match dir {
            D::North | D::South => egui::CursorIcon::ResizeVertical,
            D::East | D::West => egui::CursorIcon::ResizeHorizontal,
            D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
            D::NorthEast | D::SouthWest => egui::CursorIcon::ResizeNeSw,
        };
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    }

    fn titlebar(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        egui::TopBottomPanel::top("titlebar")
            .exact_height(48.0)
            .frame(egui::Frame::new().fill(pal.sidebar).inner_margin(Margin { left: 16, right: 0, top: 0, bottom: 0 }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let logo = self
                        .logo
                        .get_or_insert_with(|| {
                            let px = ac_overlay::logo::logo_pixmap(64);
                            let img = egui::ColorImage::from_rgba_premultiplied([64, 64], px.data());
                            ctx.load_texture("logo", img, egui::TextureOptions::LINEAR)
                        })
                        .clone();
                    ui.add(egui::Image::new(&logo).fit_to_exact_size(vec2(24.0, 24.0)));
                    ui.label(egui::RichText::new("AfterCore").font(bold(16.0)).color(pal.text));
                    ui.label(egui::RichText::new(format!("v{}", ac_core::APP_VERSION)).font(body(12.0)).color(pal.faint));
                    ui.add_space(14.0);

                    let state = self.core.etw_state.read().clone();
                    match state {
                        EtwState::Running { .. } => {
                            widgets::pill(ui, &pal, "Счётчик FPS активен", pal.good)
                                .on_hover_text("Кадры считаются через ETW - без внедрения в игры");
                        }
                        EtwState::NeedsAdmin => {
                            if widgets::pill(ui, &pal, "FPS: нужны права администратора", pal.warn)
                                .interact(Sense::click())
                                .on_hover_text("Нажмите, чтобы перезапустить AfterCore от имени администратора")
                                .clicked()
                            {
                                restart_elevated(&self.core);
                            }
                        }
                        EtwState::Failed(e) => {
                            widgets::pill(ui, &pal, "FPS: ошибка ETW", pal.bad).on_hover_text(e);
                        }
                        EtwState::Stopped => {}
                    }
                    if let Some((_, secs)) = self.core.frames.is_recording() {
                        let s = secs as u64;
                        widgets::pill(ui, &pal, &format!("Запись {:02}:{:02}", s / 60, s % 60), pal.bad);
                    }
                    if self.core.overlay_hidden.load(Ordering::Relaxed) {
                        widgets::pill(ui, &pal, "Оверлей скрыт", pal.muted);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if window_button(ui, &pal, ico::CLOSE, true).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let max_icon = if self.maximized { ico::RESTORE } else { ico::MAX };
                        if window_button(ui, &pal, max_icon, false).clicked() {
                            self.maximized = !self.maximized;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
                        }
                        if window_button(ui, &pal, ico::MIN, false).clicked() {
                            if self.cfg.general.minimize_to_tray {
                                self.core.hide_launcher();
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                        }
                        // Remaining space = drag area.
                        let rest = ui.available_rect_before_wrap();
                        let resp = ui.interact(rest, ui.id().with("drag"), Sense::click_and_drag());
                        if resp.drag_started() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        if resp.double_clicked() {
                            self.maximized = !self.maximized;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.maximized));
                        }
                    });
                });
            });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        egui::SidePanel::left("nav")
            .exact_width(240.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(pal.sidebar).inner_margin(Margin { left: 14, right: 14, top: 10, bottom: 16 }))
            .show(ctx, |ui| {
                egui::TopBottomPanel::bottom("nav_bottom")
                    .frame(egui::Frame::NONE)
                    .show_separator_line(false)
                    .show_inside(ui, |ui| {
                    egui::Frame::new()
                        .fill(pal.surface)
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .corner_radius(14)
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                          {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new("АКТИВНЫЙ ПРОФИЛЬ").font(bold(10.5)).color(pal.faint));
                            let names: Vec<String> = self.cfg.profiles.iter().map(|p| p.name.clone()).collect();
                            egui::ComboBox::from_id_salt("side_profile")
                                .selected_text(self.cfg.active_profile.clone())
                                .width(ui.available_width())
                                .show_ui(ui, |ui| {
                                    for n in names {
                                        ui.selectable_value(&mut self.cfg.active_profile, n.clone(), n);
                                    }
                                });
                            ui.add_space(4.0);
                            let mut on = self.cfg.active().overlay.enabled;
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Оверлей").font(body(13.5)).color(pal.text));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if widgets::toggle(ui, &pal, &mut on).changed() {
                                        self.cfg.active_mut().overlay.enabled = on;
                                    }
                                });
                            });
                            let mut preview = self.core.preview_on_desktop.load(Ordering::Relaxed);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("На рабочем столе").font(body(13.5)).color(pal.text))
                                    .on_hover_text("Показывает оверлей поверх всех окон - удобно настраивать вид");
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if widgets::toggle(ui, &pal, &mut preview).changed() {
                                        self.core.preview_on_desktop.store(preview, Ordering::Relaxed);
                                    }
                                });
                            });
                          }
                        });
                });

                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(egui::RichText::new("МЕНЮ").font(bold(11.0)).color(pal.faint));
                ui.add_space(2.0);
                for p in Page::ALL {
                    if nav_item(ui, &pal, p.icon(), p.title(), self.page == p).clicked() {
                        self.page = p;
                    }
                }
            });
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| t.2.elapsed() < Duration::from_millis(3200));
        if self.toasts.is_empty() {
            return;
        }
        let pal = self.pal;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("toasts"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-24.0, -24.0))
            .show(ctx, |ui| {
                ui.set_max_width(screen.width() * 0.4);
                for (text, color, at) in self.toasts.iter() {
                    let fade = (1.0 - (at.elapsed().as_secs_f32() - 2.7).max(0.0) / 0.5).clamp(0.0, 1.0);
                    egui::Frame::new()
                        .fill(alpha(pal.surface3, (250.0 * fade) as u8))
                        .stroke(Stroke::new(1.0_f32, alpha(*color, (160.0 * fade) as u8)))
                        .corner_radius(12)
                        .inner_margin(Margin::symmetric(16, 10))
                        .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha((90.0 * fade) as u8) })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(ico::INFO).font(icon(14.0)).color(alpha(*color, (255.0 * fade) as u8)));
                                ui.label(egui::RichText::new(text).font(body(14.0)).color(alpha(pal.text, (255.0 * fade) as u8)));
                            });
                        });
                }
            });
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        if self.frame_no > 5 && self.started.elapsed() > Duration::from_millis(2600) && !self.shot_requested {
            self.shot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            if let Some(mut pm) = tiny_skia::Pixmap::new(w as u32, h as u32) {
                for (d, s) in pm.data_mut().chunks_exact_mut(4).zip(img.pixels.iter()) {
                    let [r, g, b, a] = s.to_array();
                    d.copy_from_slice(&[r, g, b, a]);
                }
                let _ = pm.save_png(&path);
            }
            self.core.shutdown();
            std::process::exit(0);
        }
        ctx.request_repaint();
    }
}

fn round_corners(hwnd: isize) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2 (Windows 11; ignored elsewhere)
    let pref: u32 = 2;
    unsafe { DwmSetWindowAttribute(hwnd as _, 33, (&pref as *const u32).cast(), 4) };
}

pub fn restart_elevated(core: &Core) {
    if ac_core::process::relaunch_elevated("--elevated-attempt") {
        core.shutdown();
        std::process::exit(0);
    }
}

fn window_button(ui: &mut egui::Ui, pal: &Pal, ic: &str, close: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.08);
    let bg = if close { Color32::from_rgb(232, 17, 35) } else { pal.surface3 };
    ui.painter().rect_filled(rect, 0, alpha(bg, (h * 255.0) as u8));
    let fg = if close { mix(pal.muted, Color32::WHITE, h) } else { mix(pal.muted, pal.text, h) };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, ic, icon(10.0), fg);
    resp
}

fn nav_item(ui: &mut egui::Ui, pal: &Pal, ic: &str, title: &str, selected: bool) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 42.0), Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, selected, 0.18);
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
    let p = ui.painter();
    if h > 0.0 && t < 1.0 {
        p.rect_filled(rect, 11, alpha(pal.surface3, (h * (1.0 - t) * 255.0) as u8));
    }
    if t > 0.0 {
        p.rect_filled(rect, 11, alpha(pal.accent, (t * if pal.dark { 34.0 } else { 26.0 }) as u8));
        let bar = Rect::from_center_size(pos2(rect.left() + 2.0, rect.center().y), vec2(3.0, 18.0 * t));
        p.rect_filled(bar, 2, pal.accent);
    }
    let icol = mix(pal.muted, pal.accent, t);
    let tcol = mix(mix(pal.muted, pal.text, h.max(0.4)), pal.text, t);
    p.text(pos2(rect.left() + 26.0, rect.center().y), Align2::CENTER_CENTER, ic, icon(16.0), icol);
    p.text(pos2(rect.left() + 46.0, rect.center().y), Align2::LEFT_CENTER, title, if t > 0.5 { bold(14.5) } else { body(14.5) }, tcol);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = self.pal.bg;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_no += 1;
        // eframe makes the window visible right after the first painted frame,
        // so a tray start hides it again during the next few frames.
        if self.start_hidden {
            if self.frame_no <= 4 {
                self.core.hide_launcher();
                ctx.request_repaint();
            } else {
                self.start_hidden = false;
            }
        }
        self.first_frame = false;

        // Config changed by hotkeys / tray?
        let g = self.core.cfg_ext_gen.load(Ordering::SeqCst);
        if g != self.ext_gen {
            self.ext_gen = g;
            self.cfg = self.core.cfg.read().clone();
        }

        self.sync_style(ctx);
        let before = self.cfg.clone();

        if ctx.input(|i| i.viewport().close_requested()) && self.cfg.general.close_to_tray && self.screenshot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.core.hide_launcher();
        }

        self.handle_resize_edges(ctx);
        self.titlebar(ctx);
        self.sidebar(ctx);

        let pal = self.pal;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal.bg).inner_margin(Margin { left: 28, right: 20, top: 18, bottom: 0 }))
            .show(ctx, |ui| {
                let mut area = egui::ScrollArea::vertical().id_salt(("page", self.page as u8)).auto_shrink([false, false]);
                // Screenshot helper: AC_SCROLL=<px> scrolls the page before capture.
                if self.screenshot.is_some() {
                    if let Some(y) = std::env::var("AC_SCROLL").ok().and_then(|v| v.parse::<f32>().ok()) {
                        area = area.vertical_scroll_offset(y);
                    }
                }
                area.show(ui, |ui| {
                        ui.set_max_width(ui.available_width() - 8.0);
                        match self.page {
                            Page::Dashboard => self.page_dashboard(ui),
                            Page::Overlay => self.page_overlay(ui),
                            Page::Cores => self.page_cores(ui),
                            Page::Profiles => self.page_profiles(ui),
                            Page::Bench => self.page_bench(ui),
                            Page::Hotkeys => self.page_hotkeys(ui),
                            Page::Settings => self.page_settings(ui),
                        }
                        ui.add_space(28.0);
                    });
            });

        self.draw_toasts(ctx);

        if self.cfg != before {
            self.cfg.normalize();
            self.on_config_changed(&before);
        }
        if let Some(t) = self.dirty_since {
            if t.elapsed() > Duration::from_millis(700) {
                self.dirty_since = None;
                self.core.save();
            }
        }
        self.handle_screenshot(ctx);
        ctx.request_repaint_after(Duration::from_millis(if self.page == Page::Dashboard { 100 } else { 250 }));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        *self.core.cfg.write() = self.cfg.clone();
        self.core.shutdown();
    }
}
