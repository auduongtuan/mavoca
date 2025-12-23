use tauri::{
    menu::Menu,
    Manager, Runtime, Emitter,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VocabWord {
    word: String,
    meaning: String,
    example: String,
    category: String,
}

#[derive(Clone)]
struct AppState {
    vocab_list: Arc<Mutex<Vec<VocabWord>>>,
    current_index: Arc<Mutex<usize>>,
    is_window_visible: Arc<Mutex<bool>>,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn load_vocab_from_json() -> Vec<VocabWord> {
    let vocab_json = include_str!("../../public/vocab.json");
    serde_json::from_str(vocab_json).unwrap_or_else(|_| vec![])
}

fn update_tray_title(app: &tauri::AppHandle, word: &str) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(Some(word));
    }
}

#[tauri::command]
fn get_current_vocab_word(state: tauri::State<AppState>) -> Option<VocabWord> {
    let vocab_list = state.vocab_list.lock().unwrap();
    let index = *state.current_index.lock().unwrap();
    vocab_list.get(index).cloned()
}

#[tauri::command]
fn update_current_word_index(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
    index: usize,
) {
    let vocab_list = state.vocab_list.lock().unwrap();
    if index < vocab_list.len() {
        *state.current_index.lock().unwrap() = index;
        if let Some(word) = vocab_list.get(index) {
            update_tray_title(&app, &word.word);
        }
    }
}

#[tauri::command]
fn show_main_window(app: tauri::AppHandle, state: tauri::State<AppState>) {
    *state.is_window_visible.lock().unwrap() = true;
    if let Some(window) = app.get_webview_window("main") {
        // Position popup near menu bar (top-right area)
        let _ = position_popup_window(&window);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn position_popup_window(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    // Get monitor for scale factor
    let monitor = window.primary_monitor()?.unwrap_or_else(|| {
        window.available_monitors().unwrap_or_default().into_iter().next().unwrap()
    });

    let screen_size = monitor.size();
    let scale_factor = monitor.scale_factor();
    let window_width = 320.0 * scale_factor;

    // Position near top-right (default if click position not available)
    let x = screen_size.width as f64 - window_width - (20.0 * scale_factor);
    let y = 30.0 * scale_factor;

    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: x as i32,
        y: y as i32,
    }))?;

    Ok(())
}

fn position_popup_at_click(window: &tauri::WebviewWindow, click_x: f64, click_y: f64) -> tauri::Result<()> {
    // Get window size
    let window_size = window.outer_size()?;
    let window_width = window_size.width as f64;

    // Position popup below and aligned with click position (tray icon)
    // Adjust x so popup doesn't go off-screen to the right
    let x = (click_x - window_width / 2.0).max(10.0);
    let y = click_y + 5.0; // Small gap below menubar

    window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: x as i32,
        y: y as i32,
    }))?;

    Ok(())
}

#[tauri::command]
fn hide_main_window(app: tauri::AppHandle, state: tauri::State<AppState>) {
    *state.is_window_visible.lock().unwrap() = false;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn start_auto_rotation(app: tauri::AppHandle, state: AppState) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(10));

            // Only rotate if window is not visible
            let is_visible = *state.is_window_visible.lock().unwrap();
            if !is_visible {
                let mut index = state.current_index.lock().unwrap();
                let vocab_list = state.vocab_list.lock().unwrap();

                if !vocab_list.is_empty() {
                    *index = (*index + 1) % vocab_list.len();
                    if let Some(word) = vocab_list.get(*index) {
                        update_tray_title(&app, &word.word);
                        // Emit event to frontend to update if needed
                        let _ = app.emit("vocab-changed", word.clone());
                    }
                }
            }
        }
    });
}

fn setup_system_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    // Remove menu to allow direct left-click on tray text
    // Users can now click the menubar text directly to toggle the popup
    let _ = app.tray_by_id("main").unwrap().set_menu::<Menu<R>>(None);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load vocab data
    let vocab_list = load_vocab_from_json();
    let initial_word = vocab_list.first().map(|w| w.word.clone());

    // Initialize app state
    let app_state = AppState {
        vocab_list: Arc::new(Mutex::new(vocab_list)),
        current_index: Arc::new(Mutex::new(0)),
        is_window_visible: Arc::new(Mutex::new(false)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            greet,
            show_main_window,
            hide_main_window,
            get_current_vocab_word,
            update_current_word_index
        ])
        .setup(move |app| {
            // Setup system tray
            setup_system_tray(app.handle())?;

            // Set initial tray title
            if let Some(word) = initial_word {
                update_tray_title(app.handle(), &word);
            }

            // Start auto-rotation
            start_auto_rotation(app.handle().clone(), app_state.clone());

            // Hide main window on startup
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();

                // Apply window vibrancy for transparency with rounded corners
                #[cfg(target_os = "macos")]
                {
                    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                    use cocoa::base::{id, YES};
                    use objc::{msg_send, sel, sel_impl};

                    // Apply vibrancy first - Menu material is very transparent
                    let _ = apply_vibrancy(&window, NSVisualEffectMaterial::Menu, None, None);

                    // Then apply rounded corners to the content view
                    let ns_window = window.ns_window().unwrap() as id;
                    unsafe {
                        let content_view: id = msg_send![ns_window, contentView];
                        let _: () = msg_send![content_view, setWantsLayer: YES];

                        let layer: id = msg_send![content_view, layer];
                        let _: () = msg_send![layer, setCornerRadius: 12.0_f64];
                        let _: () = msg_send![layer, setMasksToBounds: YES];
                    }
                }

                // Hide popup when focus is lost (clicking outside)
                let window_clone = window.clone();
                let state_clone = app_state.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        *state_clone.is_window_visible.lock().unwrap() = false;
                        let _ = window_clone.hide();
                    }
                });
            }

            Ok(())
        })
        .on_tray_icon_event(move |tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                position,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    if let Some(window) = app.get_webview_window("main") {
                        let is_visible = window.is_visible().unwrap_or(false);
                        if is_visible {
                            *state.is_window_visible.lock().unwrap() = false;
                            let _ = window.hide();
                        } else {
                            *state.is_window_visible.lock().unwrap() = true;
                            // Position popup at click location (tray icon position)
                            let _ = position_popup_at_click(&window, position.x, position.y);
                            let _ = window.show().and_then(|_| window.set_focus());
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
