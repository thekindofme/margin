//! Menu bar presence: quick start/stop and meeting prompts without opening the window.
use crate::state::AppState;
use crate::{recorder, scheduler};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "margin-tray";
const ICON: &[u8] = include_bytes!("../icons/tray.png");
const ICON_REC: &[u8] = include_bytes!("../icons/tray-rec.png");

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let recording = state.recording.lock().is_some();
    let prompt = state.prompt.lock().clone();
    let menu = Menu::new(app)?;
    if recording {
        menu.append(&MenuItem::with_id(app, "stop", "Stop recording", true, None::<&str>)?)?;
    } else {
        if let Some(p) = &prompt {
            menu.append(&MenuItem::with_id(app, "prompt", format!("Take notes: {}", p.title), true, None::<&str>)?)?;
        }
        menu.append(&MenuItem::with_id(app, "quick", "Start a quick note", true, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "open", "Open Margin", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit Margin", true, None::<&str>)?)?;
    Ok(menu)
}

pub fn init(app: &AppHandle) -> tauri::Result<TrayIcon> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(true)
        .tooltip("Margin")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, ev| {
            let app = app.clone();
            match ev.id.as_ref() {
                "open" => show_main(&app),
                "quit" => {
                    recorder::stop_and_wait(&app, std::time::Duration::from_secs(20));
                    app.exit(0)
                }
                "stop" => {
                    let _ = recorder::stop(&app, true);
                }
                "quick" => {
                    std::thread::spawn(move || {
                        let state = app.state::<AppState>();
                        let r = recorder::new_meeting(&state, "", None).and_then(|m| {
                            recorder::start(&app, &m.id, None)?;
                            Ok(m.id)
                        });
                        match r {
                            Ok(id) => {
                                let _ = app.emit("open-meeting", serde_json::json!({"id": id}));
                            }
                            Err(e) => recorder::notify(&app, "Couldn't start", &e.to_string()),
                        }
                        show_main(&app);
                    });
                }
                "prompt" => {
                    std::thread::spawn(move || {
                        let p = app.state::<AppState>().prompt.lock().clone();
                        if let Some(p) = p {
                            let r = match &p.event {
                                Some(ev) => scheduler::start_for_event(&app, ev),
                                None => {
                                    let state = app.state::<AppState>();
                                    recorder::new_meeting(&state, &p.title, None).and_then(|m| {
                                        recorder::start(&app, &m.id, None)?;
                                        let _ = app.emit("open-meeting", serde_json::json!({"id": m.id}));
                                        Ok(())
                                    })
                                }
                            };
                            if let Err(e) = r {
                                recorder::notify(&app, "Couldn't start", &e.to_string());
                            }
                        }
                        show_main(&app);
                    });
                }
                _ => {}
            }
        })
        .build(app)
}

/// Rebuild the menu and swap the icon to reflect recording state.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let state = app.state::<AppState>();
    let recording = state.recording.lock().is_some();
    // Only rebuild when something visible changed, so an open menu isn't yanked away.
    static LAST: parking_lot::Mutex<Option<String>> = parking_lot::Mutex::new(None);
    let key = format!("{recording}|{:?}", state.prompt.lock().as_ref().map(|p| p.title.clone()));
    {
        let mut last = LAST.lock();
        if last.as_deref() == Some(key.as_str()) {
            return;
        }
        *last = Some(key);
    }
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    if let Ok(img) = Image::from_bytes(if recording { ICON_REC } else { ICON }) {
        let _ = tray.set_icon(Some(img));
        let _ = tray.set_icon_as_template(!recording);
    }
}
