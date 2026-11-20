use ac_core::config::{Config, UiThemeKind, ACCENTS};
use ac_telemetry::etw::EtwState;
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke, StrokeKind};

use super::overlay::font_picker;
use crate::ui::theme::{alpha, bold, body, mix, to_rgba, Pal};
use crate::ui::widgets::{self, ico};
use crate::ui::App;

impl App {
    pub fn page_settings(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        widgets::page_title(ui, &pal, "Настройки", "Интерфейс, поведение, мониторинг и данные");
        let _ = self.fonts();
        let fonts = self.fonts.clone().unwrap_or_default();

        widgets::section(ui, &pal, ico::COLOR, "Интерфейс", "Тема, акцентный цвет, шрифт и масштаб", |ui| {
            ui.label(egui::RichText::new("Тема").font(bold(13.0)).color(pal.muted));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for kind in UiThemeKind::ALL {
                    if theme_card(ui, &pal, kind, self.cfg.ui.accent, self.cfg.ui.theme == kind).clicked() {
                        self.cfg.ui.theme = kind;
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Акцентный цвет").font(bold(13.0)).color(pal.muted));
            ui.horizontal_wrapped(|ui| {
                for (name, c) in ACCENTS {
                    let col = Color32::from_rgb(c[0], c[1], c[2]);
                    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
                    let sel = self.cfg.ui.accent[..3] == c[..3];
                    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered() || sel, 0.1);
                    ui.painter().circle_filled(r.center(), 12.0 + h * 2.0, col);
                    if sel {
                        ui.painter().circle_stroke(r.center(), 16.5, Stroke::new(2.0_f32, col));
                    }
                    if resp.on_hover_text(name).clicked() {
                        self.cfg.ui.accent = c;
                    }
                }
                ui.add_space(6.0);
                let mut col = Color32::from_rgb(self.cfg.ui.accent[0], self.cfg.ui.accent[1], self.cfg.ui.accent[2]);
                if egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::Opaque).changed() {
                    self.cfg.ui.accent = to_rgba(col);
                }
                widgets::muted(ui, &pal, "свой");
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Шрифт интерфейса").font(bold(13.0)).color(pal.muted));
            font_picker(
                ui,
                &pal,
                "ui_font",
                &fonts,
                &mut self.settings.font_filter,
                &mut self.cfg.ui.font_name,
                &mut self.cfg.ui.font_path,
            );
            let mut pct = (self.cfg.ui.scale * 100.0).round() as i32;
            if widgets::slider(ui, &pal, "Масштаб интерфейса", &mut pct, 75..=150, " %") {
                self.cfg.ui.scale = pct as f32 / 100.0;
            }
            widgets::toggle_row(ui, &pal, "Анимации", "Плавные переходы и отклик элементов", &mut self.cfg.ui.animations);
            widgets::toggle_row(ui, &pal, "Свечение", "Мягкое свечение акцентных элементов", &mut self.cfg.ui.glow);
        });
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::SETTINGS, "Поведение", "Запуск, трей и права", |ui| {
            let g = &mut self.cfg.general;
            widgets::toggle_row(
                ui,
                &pal,
                "Запускать вместе с Windows",
                if self.core.elevated {
                    "Через Планировщик задач с наивысшими правами - без запроса UAC"
                } else {
                    "Через реестр (HKCU\\Run). Запустите AfterCore от администратора, чтобы стартовать без UAC"
                },
                &mut g.autostart,
            );
            widgets::toggle_row(ui, &pal, "Запускать свёрнутым в трей", "Окно не появится при старте", &mut g.start_minimized);
            widgets::toggle_row(ui, &pal, "Крестик сворачивает в трей", "Выход - через меню значка в трее", &mut g.close_to_tray);
            widgets::toggle_row(ui, &pal, "Сворачивать в трей", "Кнопка «свернуть» прячет окно в трей", &mut g.minimize_to_tray);
            widgets::toggle_row(
                ui,
                &pal,
                "Запрашивать права администратора",
                "Нужны для счётчика FPS (ETW) и смены ядер у защищённых процессов",
                &mut g.auto_elevate,
            );
            widgets::toggle_row(ui, &pal, "Уведомления в оверлее", "Смена профиля, итоги бенчмарка", &mut g.notifications);
        });
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::SHIELD, "Античиты", "Как AfterCore ведёт себя в защищённых играх", |ui| {
            widgets::muted(
                ui,
                &pal,
                "AfterCore не внедряется в игры и не читает их память. FPS берётся из системных событий Windows (ETW), \
                 оверлей - отдельное прозрачное окно, на котором только цифры производительности",
            );
            ui.add_space(2.0);
            for line in [
                "Процесс игры не открывается вообще: путь, CPU, RAM и потоки - из общего списка процессов Windows",
                "Без чтения памяти, списка модулей, хуков и внедрения DLL - ни в одной игре",
                "Оверлей - маленькое окно по размеру цифр, не прячется от скриншотов античита",
                "Ядра и приоритет у защищённых игр не трогаются (ни автоматически, ни кнопкой), пока не разрешено ниже",
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ico::CHECK).font(crate::ui::theme::icon(12.0)).color(pal.good));
                    ui.label(egui::RichText::new(line).font(body(13.0)).color(pal.text));
                });
            }
            ui.add_space(4.0);
            widgets::toggle_row(
                ui,
                &pal,
                "Менять ядра и приоритет в играх с античитом",
                "По правилам игр. Античит может заблокировать или записать такую попытку - включайте на свой риск",
                &mut self.cfg.general.tweak_anticheat_games,
            );
            ui.label(egui::RichText::new("Распознаются").font(bold(13.0)).color(pal.muted));
            ui.horizontal_wrapped(|ui| {
                for ac in ac_core::anticheat::KNOWN {
                    widgets::pill(ui, &pal, ac.name, if ac.kernel { pal.accent } else { pal.accent2 });
                }
            });
        });
        ui.add_space(4.0);

        let mut remove_black: Option<usize> = None;
        widgets::section(ui, &pal, ico::SPEED, "Мониторинг", "Частота опроса датчиков и исключения", |ui| {
            widgets::slider(ui, &pal, "Опрос датчиков", &mut self.cfg.general.sensor_interval_ms, 200..=3000, " мс");
            ui.label(egui::RichText::new("Не показывать оверлей в приложениях").font(bold(13.0)).color(pal.muted));
            ui.horizontal_wrapped(|ui| {
                for (i, b) in self.cfg.general.blacklist.iter().enumerate() {
                    if widgets::chip(ui, &pal, &format!("{b}  ✕"), false).on_hover_text("Убрать").clicked() {
                        remove_black = Some(i);
                    }
                }
            });
            ui.horizontal(|ui| {
                widgets::input(ui, &mut self.settings.new_black, "launcher.exe", 220.0);
                if widgets::ghost(ui, &pal, ico::ADD, "Добавить").clicked() {
                    let v = self.settings.new_black.trim().to_lowercase();
                    if !v.is_empty() && !self.cfg.general.blacklist.contains(&v) {
                        self.cfg.general.blacklist.push(v);
                    }
                    self.settings.new_black.clear();
                }
            });
        });
        if let Some(i) = remove_black {
            self.cfg.general.blacklist.remove(i);
        }
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::FOLDER, "Данные", "Все настройки хранятся рядом с программой", |ui| {
            widgets::muted(ui, &pal, &ac_core::paths::data_dir().to_string_lossy());
            ui.horizontal_wrapped(|ui| {
                if widgets::ghost(ui, &pal, ico::FOLDER, "Открыть папку").clicked() {
                    ac_core::process::shell_open(&ac_core::paths::data_dir().to_string_lossy());
                }
                if widgets::ghost(ui, &pal, ico::EXPORT, "Экспорт всех настроек").clicked() {
                    let (ts, _) = ac_core::win::local_time_strings();
                    let path = ac_core::paths::exports_dir().join(format!("aftercore_{ts}.json"));
                    if self.cfg.save(&path).is_ok() {
                        self.toast("Настройки сохранены в data\\exports", pal.good);
                        ac_core::process::shell_open(&ac_core::paths::exports_dir().to_string_lossy());
                    }
                }
                ui.menu_button(egui::RichText::new("Импорт настроек…").font(body(14.0)), |ui| {
                    let files: Vec<_> = std::fs::read_dir(ac_core::paths::exports_dir())
                        .map(|rd| {
                            rd.flatten()
                                .map(|e| e.path())
                                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("aftercore_")))
                                .collect()
                        })
                        .unwrap_or_default();
                    if files.is_empty() {
                        ui.label("Нет файлов aftercore_*.json в data\\exports");
                    }
                    for f in files {
                        let n = f.file_name().unwrap().to_string_lossy().to_string();
                        if ui.button(n).clicked() {
                            self.cfg = Config::load(&f);
                            self.toast("Настройки импортированы", pal.good);
                            ui.close_menu();
                        }
                    }
                });
                if !self.settings.confirm_reset {
                    if widgets::danger(ui, &pal, ico::REFRESH, "Сбросить всё").clicked() {
                        self.settings.confirm_reset = true;
                    }
                } else {
                    if widgets::danger(ui, &pal, ico::WARN, "Точно сбросить?").clicked() {
                        self.cfg = Config::default();
                        self.settings.confirm_reset = false;
                        self.toast("Настройки сброшены", pal.warn);
                    }
                    if widgets::ghost(ui, &pal, "", "Отмена").clicked() {
                        self.settings.confirm_reset = false;
                    }
                }
            });
        });
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::INFO, "О программе", &format!("AfterCore {}", ac_core::APP_VERSION), |ui| {
            widgets::muted(
                ui,
                &pal,
                "AfterCore считает кадры без внедрения в игры: события Present от DirectX и графического ядра Windows \
                 читаются через ETW, а оверлей - отдельное прозрачное окно поверх игры. Поэтому он не конфликтует \
                 с античитами и не может уронить игру",
            );
            let etw = match &*self.core.etw_state.read() {
                EtwState::Running { providers } => format!("работает · {}", providers.join(", ")),
                EtwState::NeedsAdmin => "нужны права администратора".into(),
                EtwState::Failed(e) => format!("ошибка: {e}"),
                EtwState::Stopped => "остановлен".into(),
            };
            let gpu = self.core.sensors.get().gpu.name;
            let t = &self.core.topo;
            egui::Grid::new("about").num_columns(2).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                for (k, v) in [
                    ("Счётчик кадров (ETW)", etw),
                    ("Права администратора", if self.core.elevated { "да".into() } else { "нет".into() }),
                    ("Видеокарта (NVML)", if gpu.is_empty() { "недоступно - показываем PDH".into() } else { gpu }),
                    ("Процессор", format!("{} · {} / {}", t.name, t.cores.len(), t.logical_count())),
                ] {
                    ui.label(egui::RichText::new(k).font(body(13.0)).color(pal.muted));
                    ui.label(egui::RichText::new(v).font(body(13.0)).color(pal.text));
                    ui.end_row();
                }
            });
        });
    }
}

fn theme_card(ui: &mut egui::Ui, pal: &Pal, kind: UiThemeKind, accent: [u8; 4], selected: bool) -> egui::Response {
    let tp = Pal::new(&ac_core::config::UiSettings { theme: kind, accent, ..Default::default() });
    let (rect, resp) = ui.allocate_exact_size(vec2(168.0, 112.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let t = ui.ctx().animate_bool_with_time(resp.id.with("s"), selected, 0.15);
    let p = ui.painter();
    p.rect_filled(rect, 14, tp.bg);
    // Mini UI: sidebar + cards.
    let side = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(34.0, rect.height() - 40.0));
    p.rect_filled(side, 6, tp.sidebar);
    p.rect_filled(Rect::from_min_size(side.min + vec2(6.0, 8.0), vec2(22.0, 5.0)), 3, tp.accent);
    for k in 0..3 {
        p.rect_filled(Rect::from_min_size(side.min + vec2(6.0, 18.0 + k as f32 * 9.0), vec2(22.0, 4.0)), 2, tp.faint);
    }
    let card = Rect::from_min_size(pos2(side.right() + 8.0, side.top()), vec2(rect.right() - side.right() - 16.0, 34.0));
    p.rect_filled(card, 6, tp.surface);
    p.rect_filled(Rect::from_min_size(card.min + vec2(8.0, 8.0), vec2(40.0, 7.0)), 3, tp.text);
    p.rect_filled(Rect::from_min_size(card.min + vec2(8.0, 20.0), vec2(60.0, 5.0)), 3, tp.muted);
    let card2 = Rect::from_min_size(pos2(card.left(), card.bottom() + 6.0), vec2(card.width(), side.bottom() - card.bottom() - 6.0));
    p.rect_filled(card2, 6, tp.surface);
    p.rect_filled(Rect::from_min_size(card2.min + vec2(8.0, 7.0), vec2(26.0, 8.0)), 4, tp.accent);
    p.text(pos2(rect.left() + 12.0, rect.bottom() - 16.0), Align2::LEFT_CENTER, kind.title(), bold(13.0), tp.text);
    p.rect_stroke(
        rect,
        14,
        Stroke::new(1.0 + t * 1.5, mix(mix(tp.border, pal.accent, h * 0.5), pal.accent, t)),
        StrokeKind::Inside,
    );
    if t > 0.0 {
        p.circle_filled(pos2(rect.right() - 16.0, rect.bottom() - 16.0), 8.0 * t, pal.accent);
    }
    let _ = alpha;
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
