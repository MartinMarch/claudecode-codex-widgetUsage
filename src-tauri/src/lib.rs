mod claude;
mod codex;
mod model;
mod procs;

use model::Snapshot;
use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, State, WebviewWindow,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::Mutex;

struct Sources {
    claude: Mutex<claude::ClaudeSource>,
    procs: Mutex<procs::ProcessSource>,
}

#[tauri::command]
async fn snapshot(sources: State<'_, Sources>) -> Result<Snapshot, String> {
    let claude = sources.claude.lock().await.read().await;
    let codex = tauri::async_runtime::spawn_blocking(codex::read)
        .await
        .map_err(|e| e.to_string())?;
    let processes = sources.procs.lock().await.read();
    Ok(Snapshot {
        claude,
        codex,
        processes,
        taken_at: chrono::Utc::now().to_rfc3339(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformInfo {
    os: &'static str,
    /// True when the OS blurs the desktop behind the window for us.
    native_blur: bool,
}

#[tauri::command]
fn platform_info(state: State<'_, BlurState>) -> PlatformInfo {
    PlatformInfo { os: std::env::consts::OS, native_blur: state.0 }
}

struct BlurState(bool);

fn apply_glass(window: &WebviewWindow) -> bool {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
        return apply_vibrancy(
            window,
            NSVisualEffectMaterial::HudWindow,
            Some(NSVisualEffectState::Active),
            Some(22.0),
        )
        .is_ok();
    }
    #[cfg(target_os = "windows")]
    {
        use window_vibrancy::{apply_acrylic, apply_blur};
        return apply_acrylic(window, Some((16, 20, 32, 120))).is_ok()
            || apply_blur(window, Some((16, 20, 32, 120))).is_ok();
    }
    #[allow(unreachable_code)]
    {
        let _ = window;
        false
    }
}

const WIDGET_WIDTH: f64 = 340.0;

/// Docks the widget in the top-right corner of the screen it opens on.
fn place_top_right(window: &WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else { return };
    let scale = monitor.scale_factor();
    // A hidden window can report 0×0 on some platforms; fall back to the configured width.
    let width = window
        .outer_size()
        .map(|s| s.width as i32)
        .unwrap_or(0)
        .max((WIDGET_WIDTH * scale) as i32);
    let margin = (24.0 * scale) as i32;
    let top = (48.0 * scale) as i32; // clear the macOS menu bar / top panels
    let pos = monitor.position();
    let x = pos.x + monitor.size().width as i32 - width - margin;
    let _ = window.set_position(PhysicalPosition::new(x, pos.y + top));
}

fn toggle(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let toggle_item = MenuItem::with_id(app, "toggle", "Mostrar u ocultar", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", "Abrir al iniciar sesión", true, autostart_on, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_item, &autostart, &PredefinedMenuItem::separator(app)?, &quit])?;

    let autostart_handle = autostart.clone();
    TrayIconBuilder::with_id("tray")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Agent Gauge")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle" => toggle(app),
            "autostart" => {
                let launcher = app.autolaunch();
                let enable = !launcher.is_enabled().unwrap_or(false);
                let _ = if enable { launcher.enable() } else { launcher.disable() };
                let _ = autostart_handle.set_checked(launcher.is_enabled().unwrap_or(enable));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(Sources {
            claude: Mutex::new(claude::ClaudeSource::new()),
            procs: Mutex::new(procs::ProcessSource::new()),
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory); // no Dock icon

            let window = app.get_webview_window("main").expect("main window");
            let blur = apply_glass(&window);
            app.manage(BlurState(blur));
            place_top_right(&window);
            window.show()?;
            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![snapshot, platform_info])
        .run(tauri::generate_context!())
        .expect("error while running Agent Gauge");
}
