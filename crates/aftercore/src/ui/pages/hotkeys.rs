use std::sync::atomic::Ordering;

use ac_core::config::default_hotkeys;
use ac_core::hotkey::{Hotkey, HotkeyAction, MOD_ALT, MOD_CONTROL, MOD_SHIFT};
use eframe::egui::{self, vec2, Align, Align2, Color32, Layout, Margin, Sense, Stroke, StrokeKind};

use crate::ui::theme::{alpha, bold, body, icon, mix};
use crate::ui::widgets::{self, ico};
use crate::ui::App;

fn key_to_vk(k: egui::Key) -> Option<u32> {
    use egui::Key::*;
    let name = k.name();
    if name.len() == 1 {
        let c = name.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x6F + n);
        }
    }
    Some(match k {
        Escape => 0x1B,
        Insert => 0x2D,
        Delete => 0x2E,
        Home => 0x24,
        End => 0x23,
        PageUp => 0x21,
        PageDown => 0x22,
        ArrowLeft => 0x25,
        ArrowUp => 0x26,
        ArrowRight => 0x27,
        ArrowDown => 0x28,
        Space => 0x20,
        Tab => 0x09,
        Enter => 0x0D,
        Backspace => 0x08,
        Minus => 0xBD,
        Equals | Plus => 0xBB,
        Comma => 0xBC,
        Period => 0xBE,
        Semicolon => 0xBA,
        Backtick => 0xC0,
        OpenBracket => 0xDB,
        CloseBracket => 0xDD,
        Backslash => 0xDC,
        Slash => 0xBF,
        Quote => 0xDE,
        _ => return None,
    })
}

impl App {
    fn stop_capture(&mut self) {
        self.capturing = None;
        self.core.hotkeys_paused.store(false, Ordering::SeqCst);
        self.core.hotkey_reload.store(true, Ordering::SeqCst);
    }

    pub fn page_hotkeys(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        widgets::page_title(ui, &pal, "Бинды", "Глобальные горячие клавиши - работают прямо в игре и из трея");

        // Capture keyboard while waiting for a combo.
        if let Some(action) = self.capturing {
            let ev = ui.ctx().input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, m)) = ev {
                if key == egui::Key::Escape && !m.any() {
                    self.stop_capture();
                } else if let Some(vk) = key_to_vk(key) {
                    let mut mods = 0;
                    if m.ctrl || m.command {
                        mods |= MOD_CONTROL;
                    }
                    if m.alt {
                        mods |= MOD_ALT;
                    }
                    if m.shift {
                        mods |= MOD_SHIFT;
                    }
                    let hk = Hotkey::new(mods, vk);
                    if hk.is_reasonable() {
                        // A combo can belong to one action only.
                        self.cfg.hotkeys.retain(|a, h| *a == action || *h != hk);
                        self.cfg.hotkeys.insert(action, hk);
                        self.toast(format!("{} → {}", action.title(), hk.label()), pal.good);
                        self.stop_capture();
                    } else {
                        self.toast("Добавьте Ctrl, Alt или Shift - или используйте F1-F24", pal.warn);
                    }
                }
            }
        }

        let errors = self.core.hotkey_errors.read().clone();
        let mut start: Option<HotkeyAction> = None;
        let mut clear: Option<HotkeyAction> = None;
        widgets::section(ui, &pal, ico::KEYBOARD, "Горячие клавиши", "Нажмите на сочетание, чтобы переназначить. Esc - отмена", |ui| {
            for a in HotkeyAction::ALL {
                let bound = self.cfg.hotkeys.get(&a).copied();
                let capturing = self.capturing == Some(a);
                egui::Frame::new()
                    .fill(if capturing { alpha(pal.accent, 22) } else { pal.surface2 })
                    .stroke(Stroke::new(1.0_f32, if capturing { pal.accent } else { pal.border }))
                    .corner_radius(14)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                            ui.painter().rect_filled(r, 10, pal.accent_soft);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, a.icon(), icon(15.0), pal.accent);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(egui::RichText::new(a.title()).font(bold(14.5)).color(pal.text));
                                ui.label(egui::RichText::new(a.description()).font(body(12.5)).color(pal.muted));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if bound.is_some() && !capturing && widgets::icon_btn(ui, &pal, ico::DELETE, "Убрать").clicked() {
                                    clear = Some(a);
                                }
                                let text = if capturing {
                                    "Нажмите клавиши…".to_string()
                                } else {
                                    bound.map(|h| h.label()).unwrap_or_else(|| "Не назначено".into())
                                };
                                if keycap(ui, &pal, &text, capturing, bound.is_some()).clicked() && !capturing {
                                    start = Some(a);
                                }
                                if errors.contains(&a) {
                                    widgets::pill(ui, &pal, "Занято другой программой", pal.warn);
                                }
                            });
                        });
                    });
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if widgets::ghost(ui, &pal, ico::REFRESH, "Сбросить по умолчанию").clicked() {
                    self.cfg.hotkeys = default_hotkeys();
                }
                if self.capturing.is_some() && widgets::ghost(ui, &pal, "", "Отмена").clicked() {
                    self.stop_capture();
                }
            });
        });
        if let Some(a) = clear {
            self.cfg.hotkeys.remove(&a);
        }
        if let Some(a) = start {
            self.capturing = Some(a);
            self.core.hotkeys_paused.store(true, Ordering::SeqCst);
            self.core.hotkey_reload.store(true, Ordering::SeqCst);
        }
        ui.add_space(4.0);
        widgets::card(ui, &pal, |ui| {
            ui.label(egui::RichText::new(format!("{}  Как это работает", ico::INFO)).font(bold(14.0)).color(pal.accent));
            widgets::muted(
                ui,
                &pal,
                "Нажатия ловятся двумя способами сразу - перехватом клавиатуры и опросом клавиш - прямо в AfterCore, \
                 в игру ничего не внедряется и клавиша доходит до неё как обычно",
            );
        });
        ui.add_space(4.0);
        self.keys_diagnostic(ui);
    }

    fn keys_diagnostic(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        widgets::section(
            ui,
            &pal,
            ico::SEARCH,
            "Диагностика клавиш",
            "Если бинд не срабатывает в игре: включите, зайдите в игру, нажмите бинд, вернитесь сюда",
            |ui| {
                let mut on = crate::hotkeys::diag_enabled();
                if widgets::toggle_row(
                    ui,
                    &pal,
                    "Записывать нажатия",
                    "Только коды клавиш, без текста. Пишется в data\\keys-diagnostic.log - файл остаётся, пока вы его не очистите",
                    &mut on,
                ) {
                    crate::hotkeys::set_diag(on);
                }
                ui.label(
                    egui::RichText::new(format!(
                        "Событий клавиатуры с запуска - хук: {} · опрос: {}",
                        crate::hotkeys::SEEN_HOOK.load(Ordering::Relaxed),
                        crate::hotkeys::SEEN_POLL.load(Ordering::Relaxed)
                    ))
                    .font(body(12.5))
                    .color(pal.muted),
                );
                let events = crate::hotkeys::diag_events();
                ui.horizontal(|ui| {
                    if widgets::ghost(ui, &pal, ico::FOLDER, "Открыть лог").clicked() {
                        ac_core::process::shell_open(&ac_core::paths::data_dir().join("keys-diagnostic.log").to_string_lossy());
                    }
                    if widgets::ghost(ui, &pal, ico::DELETE, "Очистить").clicked() {
                        crate::hotkeys::clear_diag();
                    }
                });
                if events.is_empty() {
                    widgets::muted(ui, &pal, "Пока пусто - нажмите любую клавишу");
                    return;
                }
                egui::Grid::new("keys_diag").num_columns(5).spacing(vec2(18.0, 4.0)).striped(false).show(ui, |ui| {
                    for h in ["Время", "Клавиша", "Канал", "Активное окно", "Результат"] {
                        ui.label(egui::RichText::new(h).font(bold(12.0)).color(pal.faint));
                    }
                    ui.end_row();
                    for e in events.iter().rev().take(14) {
                        let app = self.core.process_name(e.fg_pid);
                        let key = if e.vk == 0 { "-".to_string() } else { Hotkey::new(e.mods, e.vk).label() };
                        let col = if e.result.starts_with("СРАБОТАЛ") { pal.good } else { pal.text };
                        ui.label(egui::RichText::new(&e.time).font(body(12.5)).color(pal.muted));
                        ui.label(egui::RichText::new(key).font(bold(13.0)).color(pal.text));
                        ui.label(egui::RichText::new(e.path).font(body(12.5)).color(pal.muted));
                        ui.label(egui::RichText::new(app).font(body(12.5)).color(pal.text));
                        ui.label(egui::RichText::new(&e.result).font(body(12.5)).color(col));
                        ui.end_row();
                    }
                });
            },
        );
    }
}

fn keycap(ui: &mut egui::Ui, pal: &crate::ui::theme::Pal, text: &str, active: bool, bound: bool) -> egui::Response {
    let font = bold(13.5);
    let w = ui.painter().layout_no_wrap(text.to_string(), font.clone(), pal.text).size().x + 28.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(120.0), 36.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let p = ui.painter();
    let fill = if active { pal.accent } else { mix(pal.surface3, pal.surface, 0.3 - h * 0.3) };
    // Key "depth".
    p.rect_filled(rect.translate(vec2(0.0, 2.0)), 10, Color32::from_black_alpha(if pal.dark { 90 } else { 30 }));
    p.rect_filled(rect, 10, fill);
    p.rect_stroke(rect, 10, Stroke::new(1.0_f32, mix(pal.border, pal.accent, h)), StrokeKind::Inside);
    let col = if active {
        pal.on_accent
    } else if bound {
        pal.text
    } else {
        pal.faint
    };
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
