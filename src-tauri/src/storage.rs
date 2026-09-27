use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub client_id: String,
    pub opacity: u8,
    pub desktop_mode: bool,
    pub autostart: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
}

impl Default for Settings {
    fn default() -> Self { Self { client_id: String::new(), opacity: 88, desktop_mode: false, autostart: false, x: None, y: None, width: 1100, height: 740 } }
}

#[derive(Clone)]
pub struct Storage { path: PathBuf }

impl Storage {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        let this = Self { path };
        this.db()?.execute_batch("CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);").map_err(|e| e.to_string())?;
        Ok(this)
    }
    fn db(&self) -> Result<Connection, String> { Connection::open(&self.path).map_err(|e| e.to_string()) }
    pub fn get(&self, key: &str) -> Result<Option<String>, String> { self.db()?.query_row("SELECT value FROM kv WHERE key=?1", [key], |r| r.get(0)).optional().map_err(|e| e.to_string()) }
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> { self.db()?.execute("INSERT INTO kv(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value]).map_err(|e| e.to_string())?; Ok(()) }
    pub fn clear_cache(&self) -> Result<(), String> { self.db()?.execute("DELETE FROM kv WHERE key LIKE 'month:%'", []).map_err(|e| e.to_string())?; Ok(()) }
    pub fn settings(&self) -> Result<Settings, String> { Ok(self.get("settings")?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()) }
    pub fn set_settings(&self, settings: &Settings) -> Result<(), String> { self.set("settings", &serde_json::to_string(settings).map_err(|e| e.to_string())?) }
}
