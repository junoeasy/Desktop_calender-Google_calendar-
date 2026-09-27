use chrono::{Local, TimeZone, Utc};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;
use crate::{auth::Auth, storage::Storage};

#[derive(Clone, Serialize, Deserialize)]
pub struct CalendarEvent { pub id: String, pub calendar_id: String, pub title: String, pub start: String, pub end: String, pub all_day: bool, pub recurring: bool, pub link: String, pub color: String, pub writable: bool }
#[derive(Clone, Serialize, Deserialize)]
pub struct Task { pub id: String, pub list_id: String, pub title: String, pub notes: String, pub due: Option<String>, pub completed: bool }
#[derive(Clone, Serialize, Deserialize)]
pub struct TaskList { pub id: String, pub title: String }
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot { pub events: Vec<CalendarEvent>, pub tasks: Vec<Task>, pub task_lists: Vec<TaskList>, pub cached_at: String, pub offline: bool }
#[derive(Deserialize)]
pub struct EventInput { pub id: Option<String>, pub calendar_id: String, pub title: String, pub start: String, pub end: String, pub all_day: bool }
#[derive(Deserialize)]
pub struct TaskInput { pub id: Option<String>, pub list_id: String, pub title: String, pub notes: String, pub due: Option<String> }

pub struct Google { http: reqwest::Client }
impl Google {
    pub fn new(http: reqwest::Client) -> Self { Self { http } }
    async fn request(&self, auth: &Auth, store: &Storage, method: Method, url: Url, body: Option<Value>) -> Result<Value, String> {
        let token = auth.access(store).await?;
        let mut req = self.http.request(method, url).bearer_auth(token);
        if let Some(value) = body { req = req.json(&value); }
        let response = req.send().await.map_err(|e| e.to_string())?;
        let status = response.status(); let text = response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() { return Err(format!("Google API {status}: {text}")); }
        if text.is_empty() { Ok(Value::Null) } else { serde_json::from_str(&text).map_err(|e| e.to_string()) }
    }
    async fn pages(&self, auth: &Auth, store: &Storage, url: Url) -> Result<Vec<Value>, String> {
        let mut all = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut page_url = url.clone();
            if let Some(next) = &token { page_url.query_pairs_mut().append_pair("pageToken", next); }
            let page = self.request(auth, store, Method::GET, page_url, None).await?;
            if let Some(items) = page.get("items").and_then(Value::as_array) { all.extend(items.iter().cloned()); }
            token = page.get("nextPageToken").and_then(Value::as_str).map(str::to_owned);
            if token.is_none() { break; }
        }
        Ok(all)
    }
    fn path(base: &str, segments: &[&str]) -> Result<Url, String> {
        let mut url = Url::parse(base).map_err(|e| e.to_string())?;
        { let mut path = url.path_segments_mut().map_err(|_| "잘못된 API 경로")?; path.extend(segments.iter().copied()); }
        Ok(url)
    }
    pub async fn month(&self, auth: &Auth, store: &Storage, month: &str) -> Result<Snapshot, String> {
        let (year, mon) = parse_month(month)?;
        let next = if mon == 12 { (year + 1, 1) } else { (year, mon + 1) };
        let from = Local.with_ymd_and_hms(year, mon, 1, 0, 0, 0).single().ok_or("잘못된 월 시작일")?.to_rfc3339();
        let to = Local.with_ymd_and_hms(next.0, next.1, 1, 0, 0, 0).single().ok_or("잘못된 월 종료일")?.to_rfc3339();
        let calendars = self.pages(auth, store, Url::parse("https://www.googleapis.com/calendar/v3/users/me/calendarList?maxResults=250").unwrap()).await?;
        let mut events = Vec::new();
        for cal in calendars.iter().filter(|c| !c["hidden"].as_bool().unwrap_or(false) && c["selected"].as_bool().unwrap_or(true)) {
            let Some(id) = cal["id"].as_str() else { continue };
            let mut url = Self::path("https://www.googleapis.com/calendar/v3/calendars/", &[id, "events"])?;
            url.query_pairs_mut().append_pair("timeMin", &from).append_pair("timeMax", &to).append_pair("singleEvents", "true").append_pair("showDeleted", "false").append_pair("maxResults", "2500");
            let color = cal["backgroundColor"].as_str().unwrap_or("#9d8cf7").to_string();
            let writable = matches!(cal["accessRole"].as_str(), Some("owner" | "writer"));
            for e in self.pages(auth, store, url).await? {
                if e["status"] == "cancelled" { continue; }
                let Some(event_id) = e["id"].as_str() else { continue };
                let all_day = e["start"]["date"].is_string();
                let start = e["start"][if all_day { "date" } else { "dateTime" }].as_str().unwrap_or("").to_string();
                let end = e["end"][if all_day { "date" } else { "dateTime" }].as_str().unwrap_or("").to_string();
                if start.is_empty() || end.is_empty() { continue; }
                events.push(CalendarEvent { id: event_id.into(), calendar_id: id.into(), title: e["summary"].as_str().unwrap_or("(제목 없음)").into(), start, end, all_day, recurring: e.get("recurringEventId").is_some() || e.get("recurrence").is_some(), link: e["htmlLink"].as_str().unwrap_or("").into(), color: color.clone(), writable });
            }
        }
        let lists = self.pages(auth, store, Url::parse("https://tasks.googleapis.com/tasks/v1/users/@me/lists?maxResults=100").unwrap()).await?;
        let mut task_lists = Vec::new(); let mut tasks = Vec::new();
        for list in lists {
            let Some(id) = list["id"].as_str() else { continue };
            task_lists.push(TaskList { id: id.into(), title: list["title"].as_str().unwrap_or("할 일").into() });
            let mut url = Self::path("https://tasks.googleapis.com/tasks/v1/lists/", &[id, "tasks"])?;
            url.query_pairs_mut().append_pair("maxResults", "100").append_pair("showCompleted", "true").append_pair("showHidden", "true");
            for task in self.pages(auth, store, url).await? {
                if task["deleted"].as_bool().unwrap_or(false) { continue; }
                let Some(task_id) = task["id"].as_str() else { continue };
                tasks.push(Task { id: task_id.into(), list_id: id.into(), title: task["title"].as_str().unwrap_or("(제목 없음)").into(), notes: task["notes"].as_str().unwrap_or("").into(), due: task["due"].as_str().map(str::to_owned), completed: task["status"] == "completed" });
            }
        }
        Ok(Snapshot { events, tasks, task_lists, cached_at: Utc::now().to_rfc3339(), offline: false })
    }
    pub async fn save_event(&self, auth: &Auth, store: &Storage, input: EventInput) -> Result<(), String> {
        if input.title.trim().is_empty() { return Err("일정 제목이 비어 있습니다".into()); }
        let mut url = Self::path("https://www.googleapis.com/calendar/v3/calendars/", &[&input.calendar_id, "events"])?;
        let method = if let Some(id) = &input.id { url.path_segments_mut().map_err(|_| "잘못된 URL")?.push(id); Method::PATCH } else { Method::POST };
        let time = |value: &str| if input.all_day { json!({"date": value}) } else { json!({"dateTime": value}) };
        self.request(auth, store, method, url, Some(json!({"summary": input.title.trim(), "start": time(&input.start), "end": time(&input.end)}))).await?;
        Ok(())
    }
    pub async fn delete_event(&self, auth: &Auth, store: &Storage, calendar_id: &str, event_id: &str) -> Result<(), String> {
        self.request(auth, store, Method::DELETE, Self::path("https://www.googleapis.com/calendar/v3/calendars/", &[calendar_id, "events", event_id])?, None).await?; Ok(())
    }
    pub async fn save_task(&self, auth: &Auth, store: &Storage, input: TaskInput) -> Result<(), String> {
        if input.title.trim().is_empty() || input.list_id.is_empty() { return Err("할 일 제목과 목록이 필요합니다".into()); }
        let mut url = Self::path("https://tasks.googleapis.com/tasks/v1/lists/", &[&input.list_id, "tasks"])?;
        let method = if let Some(id) = &input.id { url.path_segments_mut().map_err(|_| "잘못된 URL")?.push(id); Method::PATCH } else { Method::POST };
        let due = input.due.as_ref().filter(|d| !d.is_empty()).map(|d| format!("{d}T00:00:00.000Z"));
        self.request(auth, store, method, url, Some(json!({"title": input.title.trim(), "notes": input.notes, "due": due}))).await?; Ok(())
    }
    pub async fn complete_task(&self, auth: &Auth, store: &Storage, list_id: &str, task_id: &str, completed: bool) -> Result<(), String> {
        let body = if completed { json!({"status":"completed"}) } else { json!({"status":"needsAction","completed":null}) };
        self.request(auth, store, Method::PATCH, Self::path("https://tasks.googleapis.com/tasks/v1/lists/", &[list_id, "tasks", task_id])?, Some(body)).await?; Ok(())
    }
    pub async fn delete_task(&self, auth: &Auth, store: &Storage, list_id: &str, task_id: &str) -> Result<(), String> {
        self.request(auth, store, Method::DELETE, Self::path("https://tasks.googleapis.com/tasks/v1/lists/", &[list_id, "tasks", task_id])?, None).await?; Ok(())
    }
}

fn parse_month(s: &str) -> Result<(i32,u32), String> {
    if s.len() != 7 || s.as_bytes()[4] != b'-' { return Err("월 형식은 YYYY-MM이어야 합니다".into()); }
    let year: i32 = s[..4].parse().map_err(|_| "잘못된 연도")?;
    let month: u32 = s[5..].parse().map_err(|_| "잘못된 월")?;
    if !(1..=12).contains(&month) || !(1900..=2100).contains(&year) { return Err("월 범위를 확인하세요".into()); }
    Ok((year, month))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn month_validation() { assert_eq!(parse_month("2026-09").unwrap(), (2026,9)); assert!(parse_month("2026-13").is_err()); assert!(parse_month("2026-9").is_err()); }
}
