mod chats;
mod chats_claude;
mod chats_codex;
mod claude;
mod codex;
mod model;
mod tail;

use model::Snapshot;
use serde::Serialize;
use std::time::SystemTime;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, State, WebviewWindow,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::Mutex;

struct Sources {
    claude: Mutex<claude::ClaudeSource>,
}

#[tauri::command]
async fn snapshot(sources: State<'_, Sources>) -> Result<Snapshot, String> {
    let claude = sources.claude.lock().await.read().await;
    let (codex, projects) = tauri::async_runtime::spawn_blocking(|| {
        (codex::read(), chats::read_all(SystemTime::now()))
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(Snapshot {
        claude,
        codex,
        projects,
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

/// Asks Windows 11 for its standard 8 px rounded corners explicitly, so the
/// acrylic shape always matches the panel's `--radius: 8px` (styles.css).
/// Harmless on Windows 10, which ignores the attribute.
#[cfg(target_os = "windows")]
fn round_corners(window: &WebviewWindow) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let pref = DWMWCP_ROUND;
    // SAFETY: valid window handle owned by this process; pointer and size describe `pref`.
    unsafe {
        DwmSetWindowAttribute(
            hwnd.0 as _,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}

/// Windows 11 turns the acrylic backdrop into a flat colour whenever the
/// window is inactive. The widget lives unfocused, so we answer every
/// WM_NCACTIVATE as "active": the frame (and its acrylic) never goes dull.
/// Real keyboard focus is untouched; the page tracks it on its own.
#[cfg(target_os = "windows")]
fn keep_frame_active(window: &WebviewWindow) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_NCACTIVATE;

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        // SAFETY: forwarding to the next handler in the subclass chain.
        unsafe {
            if msg == WM_NCACTIVATE {
                return DefSubclassProc(hwnd, msg, 1, lparam);
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
    }

    let Ok(hwnd) = window.hwnd() else { return };
    // SAFETY: valid window handle owned by this thread's process; `proc` lives for the program.
    unsafe {
        SetWindowSubclass(hwnd.0 as _, Some(proc), 1, 0);
    }
}

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
        round_corners(window);
        keep_frame_active(window);
        // On Windows 11 the system acrylic ignores this tint and follows the window
        // theme instead, so the window is pinned to the dark theme in tauri.conf.json;
        // otherwise a light Windows theme washes the glass out grey.
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
    // A window that has never been shown may not report a monitor yet (seen on Windows).
    let Some(monitor) = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
    else {
        return;
    };
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
