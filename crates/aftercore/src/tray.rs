//! System tray icon with a quick menu.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::core::Core;

pub fn create(core: Arc<Core>) -> Option<TrayIcon> {
    let show = MenuItem::new("Открыть AfterCore", true, None);
    let overlay = MenuItem::new("Показать / скрыть оверлей", true, None);
    let bench = MenuItem::new("Старт / стоп бенчмарка", true, None);
    let next = MenuItem::new("Следующий профиль", true, None);
    let quit = MenuItem::new("Выход", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &show,
        &PredefinedMenuItem::separator(),
        &overlay,
        &bench,
        &next,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .ok()?;

    let icon = Icon::from_rgba(ac_overlay::logo::logo_rgba(32), 32, 32).ok()?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("AfterCore")
        .with_icon(icon)
        .build()
        .ok()?;

    let ids = (show.id().clone(), overlay.id().clone(), bench.id().clone(), next.id().clone(), quit.id().clone());
    {
        let core = core.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let (show, overlay, bench, next, quit) = &ids;
            if &e.id == show {
                core.show_launcher();
            } else if &e.id == overlay {
                core.dispatch(ac_core::hotkey::HotkeyAction::ToggleOverlay);
            } else if &e.id == bench {
                core.bench_toggle();
            } else if &e.id == next {
                core.dispatch(ac_core::hotkey::HotkeyAction::NextProfile);
            } else if &e.id == quit {
                core.shutdown();
                std::process::exit(0);
            }
        }));
    }
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if core.quit.load(Ordering::Relaxed) {
            return;
        }
        match e {
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
            | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => core.show_launcher(),
            _ => {}
        }
    }));
    Some(tray)
}
