mod auth;
mod google;
mod platform;
mod storage;

use auth::Auth;
use chrono::{Datelike, Local};
use google::{EventInput, Google, Snapshot, TaskInput};
use std::sync::Mutex;
use storage::{Settings, Storage};
use tauri::{Emitter, Manager, State, WebviewWindow, WindowEvent, menu::{Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent}};
use tauri_plugin_autostart::ManagerExt;

struct AppState { store: Storage, auth: Auth, google: Google, month: Mutex<String> }
fn current_month() -> String { let d = Local::now(); format!("{:04}-{:02}", d.year(), d.month()) }
fn cached(state: &AppState, month: &str) -> Result<Option<Snapshot>, String> { state.store.get(&format!("month:v2:{month}"))?.map(|s| serde_json::from_str(&s).map_err(|e| e.to_string())).transpose() }
async fn sync(state: &AppState, month: &str, force: bool) -> Result<Snapshot, String> {
    if !force { if let Some(snapshot) = cached(state, month)? { return Ok(snapshot); } }
    if !state.auth.status() { return Ok(Snapshot { events: vec![], tasks: vec![], task_lists: vec![], calendars: vec![], cached_at: String::new(), offline: false }); }
    let generation = state.auth.generation();
    match state.google.month(&state.auth, &state.store, month).await {
        Ok(snapshot) => { state.auth.store_if_current(generation, || state.store.set(&format!("month:v2:{month}"), &serde_json::to_string(&snapshot).map_err(|e| e.to_string())?))?; Ok(snapshot) }
        Err(err) => {
            if generation != state.auth.generation() { return Err("데이터가 변경되어 이전 동기화를 중단했습니다".into()); }
            if err.starts_with("Google 인증 갱신 실패") || err.starts_with("Google API 401") { return Err(format!("Google 계정에 다시 로그인하세요. {err}")); }
            if err.starts_with("Google API 4") { return Err(err); }
            if let Some(mut snapshot) = cached(state, month)? { snapshot.offline = true; Ok(snapshot) } else { Err(err) }
        }
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let mut settings = state.store.settings()?;
    if let Some(client_id) = option_env!("GOOGLE_OAUTH_CLIENT_ID") { settings.client_id = client_id.to_string(); }
    Ok(settings)
}
#[tauri::command]
fn save_settings(app: tauri::AppHandle, win: WebviewWindow, state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    if !(35..=100).contains(&settings.opacity) || settings.width < 600 || settings.height < 400 || !matches!(settings.theme.as_str(), "midnight" | "ocean" | "forest" | "light" | "vintage") { return Err("잘못된 화면 설정입니다".into()); }
    let manager = app.autolaunch();
    if settings.autostart { manager.enable().map_err(|e| e.to_string())?; } else { manager.disable().map_err(|e| e.to_string())?; }
    platform::apply(&win, settings.desktop_mode)?;
    state.store.set_settings(&settings)
}
#[tauri::command]
fn save_window_geometry(state: State<'_, AppState>, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
    if width < 600 || height < 400 { return Ok(()); }
    let mut settings = state.store.settings()?;
    settings.x = Some(x); settings.y = Some(y); settings.width = width; settings.height = height;
    state.store.set_settings(&settings)
}
#[tauri::command]
fn auth_status(state: State<'_, AppState>) -> bool { state.auth.status() }
#[tauri::command]
async fn sign_in(state: State<'_, AppState>, client_id: String) -> Result<(), String> {
    let client_id = option_env!("GOOGLE_OAUTH_CLIENT_ID").unwrap_or(&client_id).to_string();
    let mut settings = state.store.settings()?; settings.client_id = client_id.clone(); state.store.set_settings(&settings)?;
    state.auth.sign_in(client_id).await?;
    state.store.clear_cache()
}
#[tauri::command]
fn sign_out(state: State<'_, AppState>) -> Result<(), String> { state.auth.sign_out()?; state.store.clear_cache() }
fn changed(state: &AppState) {
    if let Err(err) = state.auth.invalidate_syncs() { eprintln!("invalidate syncs: {err}"); }
    if let Err(err) = state.store.clear_cache() { eprintln!("clear cache: {err}"); }
}
#[tauri::command]
async fn sync_month(state: State<'_, AppState>, month: String, force: bool) -> Result<Snapshot, String> {
    *state.month.lock().map_err(|e| e.to_string())? = month.clone();
    sync(&state, &month, force).await
}
#[tauri::command]
fn cached_month(state: State<'_, AppState>, month: String) -> Result<Option<Snapshot>, String> { cached(&state, &month) }
#[tauri::command]
async fn prefetch_month(state: State<'_, AppState>, month: String) -> Result<(), String> { sync(&state, &month, false).await.map(|_| ()) }
#[tauri::command]
async fn save_event(state: State<'_, AppState>, input: EventInput) -> Result<serde_json::Value, String> { let saved = state.google.save_event(&state.auth, &state.store, input).await?; changed(&state); Ok(saved) }
#[tauri::command]
async fn delete_event(state: State<'_, AppState>, calendar_id: String, event_id: String) -> Result<(), String> { state.google.delete_event(&state.auth, &state.store, &calendar_id, &event_id).await?; changed(&state); Ok(()) }
#[tauri::command]
async fn save_task(state: State<'_, AppState>, input: TaskInput) -> Result<serde_json::Value, String> { let saved = state.google.save_task(&state.auth, &state.store, input).await?; changed(&state); Ok(saved) }
#[tauri::command]
async fn complete_task(state: State<'_, AppState>, list_id: String, task_id: String, completed: bool) -> Result<(), String> { state.google.complete_task(&state.auth, &state.store, &list_id, &task_id, completed).await?; changed(&state); Ok(()) }
#[tauri::command]
async fn delete_task(state: State<'_, AppState>, list_id: String, task_id: String) -> Result<(), String> { state.google.delete_task(&state.auth, &state.store, &list_id, &task_id).await?; changed(&state); Ok(()) }
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
    let host = parsed.host_str().unwrap_or("");
    if parsed.scheme() != "https" || !(host == "calendar.google.com" || (host == "www.google.com" && parsed.path().starts_with("/calendar/"))) { return Err("Google Calendar 링크만 열 수 있습니다".into()); }
    webbrowser::open(&url).map_err(|e| e.to_string())?; Ok(())
}

fn show(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if let Err(e) = platform::apply(&win, false) { eprintln!("interactive window: {e}"); }
        let _ = win.show();
        platform::raise(&win);
        let _ = win.set_focus();
    }
}
fn pin(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let state = app.state::<AppState>();
        if let Ok(mut settings) = state.store.settings() {
            settings.desktop_mode = true;
            if let Err(e) = state.store.set_settings(&settings) { eprintln!("save desktop mode: {e}"); }
        }
        if let Err(e) = platform::apply(&win, true) { eprintln!("desktop placement: {e}"); }
        let _ = win.show();
        let _ = app.emit("desktop-mode-changed", true);
    }
}
fn trigger_sync(app: &tauri::AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let month = state.month.lock().map(|m| m.clone()).unwrap_or_else(|_| current_month());
        if sync(&state, &month, true).await.is_ok() { let _ = handle.emit("sync-complete", ()); }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let store = Storage::new(dir.join("calendar.sqlite")).map_err(std::io::Error::other)?;
            let settings = store.settings().map_err(std::io::Error::other)?;
            let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(25)).build()?;
            app.manage(AppState { store, auth: Auth::new(client.clone()), google: Google::new(client), month: Mutex::new(current_month()) });
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize { width: settings.width, height: settings.height }));
                if let (Some(x), Some(y)) = (settings.x, settings.y) { let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y })); }
                if let Err(e) = platform::apply(&win, settings.desktop_mode) { eprintln!("desktop placement: {e}"); }
            }
            let open = MenuItem::with_id(app, "open", "달력 열기 (클릭 가능)", true, None::<&str>)?;
            let pin_item = MenuItem::with_id(app, "pin", "바탕화면에 고정", true, None::<&str>)?;
            let sync_item = MenuItem::with_id(app, "sync", "지금 동기화", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "앱 종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &pin_item, &sync_item, &quit])?;
            let mut tray = TrayIconBuilder::new().menu(&menu).show_menu_on_left_click(false).on_menu_event(|app, event| match event.id().as_ref() {
                "open" => show(app), "pin" => pin(app), "sync" => trigger_sync(app), "quit" => app.exit(0), _ => ()
            }).on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event { show(tray.app_handle()); }
            });
            if let Some(icon) = app.default_window_icon() { tray = tray.icon(icon.clone()); }
            tray.build(app)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
                interval.tick().await;
                loop { interval.tick().await; trigger_sync(&handle); }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event { api.prevent_close(); let _ = window.hide(); }
        })
        .invoke_handler(tauri::generate_handler![get_settings, save_settings, save_window_geometry, auth_status, sign_in, sign_out, sync_month, cached_month, prefetch_month, save_event, delete_event, save_task, complete_task, delete_task, open_link])
        .run(tauri::generate_context!())
        .expect("failed to run Desktop Calendar");
}
