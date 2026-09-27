use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::{Read, Write}, net::TcpListener, sync::Mutex, time::{Duration, Instant}};
use url::Url;
use crate::storage::Storage;

const SERVICE: &str = "com.junho.desktopcalendar";
const SCOPES: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/tasks";
const CLIENT_SECRET: Option<&str> = option_env!("GOOGLE_OAUTH_CLIENT_SECRET");

fn token_error(prefix: &str, status: reqwest::StatusCode, body: &str) -> String {
    if body.contains("client_secret is missing") {
        return "Google OAuth 클라이언트 보안 비밀번호가 필요합니다. Google Cloud의 데스크톱 OAuth 클라이언트에서 보안 비밀번호를 확인하고 GOOGLE_OAUTH_CLIENT_SECRET을 설정한 뒤 앱을 다시 빌드하세요.".into();
    }
    format!("{prefix} ({status}): {body}")
}

#[derive(Deserialize)]
struct TokenReply { access_token: String, refresh_token: Option<String>, expires_in: u64 }
struct Token { access: String, expires_at: Instant }

pub struct Auth { token: Mutex<Option<Token>>, http: reqwest::Client }

impl Auth {
    pub fn new(http: reqwest::Client) -> Self { Self { token: Mutex::new(None), http } }
    fn entry() -> Result<keyring::Entry, String> { keyring::Entry::new(SERVICE, "google-refresh-token").map_err(|e| e.to_string()) }
    pub fn status(&self) -> bool { self.token.lock().map(|t| t.is_some()).unwrap_or(false) || Self::entry().and_then(|e| e.get_password().map_err(|e| e.to_string())).is_ok() }
    pub fn sign_out(&self) -> Result<(), String> { if let Ok(entry) = Self::entry() { if let Err(e) = entry.delete_credential() { if !matches!(e, keyring::Error::NoEntry) { return Err(e.to_string()); } } } *self.token.lock().map_err(|e| e.to_string())? = None; Ok(()) }
    fn set_token(&self, reply: TokenReply) -> Result<(), String> {
        if let Some(refresh) = reply.refresh_token { Self::entry()?.set_password(&refresh).map_err(|e| e.to_string())?; }
        else if Self::entry()?.get_password().is_err() { return Err("Google이 갱신 토큰을 반환하지 않았습니다. 다시 로그인하세요".into()); }
        *self.token.lock().map_err(|e| e.to_string())? = Some(Token { access: reply.access_token, expires_at: Instant::now() + Duration::from_secs(reply.expires_in) });
        Ok(())
    }
    pub async fn access(&self, store: &Storage) -> Result<String, String> {
        if let Some(t) = self.token.lock().map_err(|e| e.to_string())?.as_ref() { if t.expires_at > Instant::now() + Duration::from_secs(60) { return Ok(t.access.clone()); } }
        let client_id = store.settings()?.client_id;
        if client_id.is_empty() { return Err("Google OAuth 클라이언트 ID를 설정하세요".into()); }
        let refresh = Self::entry()?.get_password().map_err(|_| "Google 계정에 다시 로그인하세요".to_string())?;
        let mut form = vec![("client_id", client_id.as_str()), ("refresh_token", refresh.as_str()), ("grant_type", "refresh_token")];
        if let Some(secret) = CLIENT_SECRET.filter(|secret| !secret.is_empty()) { form.push(("client_secret", secret)); }
        let response = self.http.post("https://oauth2.googleapis.com/token").form(&form).send().await.map_err(|e| e.to_string())?;
        let status = response.status(); let body = response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() { return Err(token_error("Google 인증 갱신 실패", status, &body)); }
        let reply: TokenReply = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        self.set_token(reply)?;
        Ok(self.token.lock().map_err(|e| e.to_string())?.as_ref().unwrap().access.clone())
    }
    pub async fn sign_in(&self, client_id: String) -> Result<(), String> {
        if !client_id.ends_with(".apps.googleusercontent.com") { return Err("Desktop OAuth 클라이언트 ID를 입력하세요".into()); }
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let redirect = format!("http://127.0.0.1:{}/", listener.local_addr().map_err(|e| e.to_string())?.port());
        let random = |len| { let mut bytes = vec![0u8; len]; rand::thread_rng().fill_bytes(&mut bytes); URL_SAFE_NO_PAD.encode(bytes) };
        let state = random(24); let verifier = random(48); let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth").map_err(|e| e.to_string())?;
        url.query_pairs_mut().append_pair("client_id", &client_id).append_pair("redirect_uri", &redirect).append_pair("response_type", "code").append_pair("scope", SCOPES).append_pair("access_type", "offline").append_pair("prompt", "consent").append_pair("code_challenge", &challenge).append_pair("code_challenge_method", "S256").append_pair("state", &state);
        webbrowser::open(url.as_str()).map_err(|e| e.to_string())?;
        let state_copy = state.clone();
        let code = tokio::task::spawn_blocking(move || -> Result<String, String> {
            listener.set_nonblocking(true).map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(180);
            loop {
                if Instant::now() > deadline { return Err("로그인 시간이 초과되었습니다".into()); }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
                        let mut buf = [0u8; 8192]; let size = stream.read(&mut buf).map_err(|e| e.to_string())?;
                        let request = String::from_utf8_lossy(&buf[..size]);
                        let path = request.split_whitespace().nth(1).ok_or("잘못된 OAuth 응답")?;
                        let url = Url::parse(&format!("http://127.0.0.1{path}")).map_err(|e| e.to_string())?;
                        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
                        let valid = params.get("state") == Some(&state_copy);
                        let result = if valid { params.get("code").cloned() } else { None };
                        let page = if result.is_some() { "로그인이 완료되었습니다. 이 탭을 닫아도 됩니다." } else { "로그인을 완료하지 못했습니다. 앱에서 다시 시도하세요." };
                        let body = format!("<html><meta charset=\"utf-8\"><body style=\"font-family:sans-serif;padding:40px\">{page}</body></html>");
                        let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                        let _ = stream.write_all(reply.as_bytes());
                        return result.ok_or_else(|| params.get("error").cloned().unwrap_or_else(|| "OAuth state가 일치하지 않습니다".into()));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                    Err(e) => return Err(e.to_string()),
                }
            }
        }).await.map_err(|e| e.to_string())??;
        let mut form = vec![("client_id", client_id.as_str()), ("code", code.as_str()), ("code_verifier", verifier.as_str()), ("redirect_uri", redirect.as_str()), ("grant_type", "authorization_code")];
        if let Some(secret) = CLIENT_SECRET.filter(|secret| !secret.is_empty()) { form.push(("client_secret", secret)); }
        let response = self.http.post("https://oauth2.googleapis.com/token").form(&form).send().await.map_err(|e| e.to_string())?;
        let status = response.status(); let body = response.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() { return Err(token_error("Google 로그인 실패", status, &body)); }
        self.set_token(serde_json::from_str(&body).map_err(|e| e.to_string())?)
    }
}
