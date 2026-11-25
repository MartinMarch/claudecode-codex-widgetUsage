//! Claude Code usage.
//!
//! Two sources:
//! 1. Plan limits (5 h session + weekly) from the same endpoint Claude Code's
//!    `/usage` screen uses, authenticated with the OAuth token Claude Code
//!    already stored on this machine. The endpoint is not a public API, so
//!    every field is optional and failures degrade to "unavailable".
//! 2. Tokens over the last 5 hours, summed from the local transcripts in
//!    `~/.claude/projects/**.jsonl`. Works offline and needs no token.
//!
//! On Windows and Linux, expiring credentials can be refreshed in place.
//! The macOS Keychain stays read-only.

use crate::model::{LimitWindow, Provider, ProviderUsage, Status};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
#[cfg(not(target_os = "macos"))]
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
#[cfg(not(target_os = "macos"))]
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
#[cfg(not(target_os = "macos"))]
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
#[cfg(not(target_os = "macos"))]
const REFRESH_INTERVAL_SECS: u64 = 300;
#[cfg(not(target_os = "macos"))]
const REFRESH_FAILED: &str =
    "Tu sesión de Claude caducó del todo. Abre Claude Code y vuelve a iniciar sesión.";

pub fn config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    dirs::home_dir().map(|h| h.join(".claude"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OauthBlob {
    access_token: String,
    expires_at: Option<i64>,
    subscription_type: Option<String>,
}

#[cfg(not(target_os = "macos"))]
#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    refresh_token_expires_in: Option<i64>,
}

#[cfg(not(target_os = "macos"))]
fn needs_refresh(expires_at_ms: Option<i64>, now_ms: i64) -> bool {
    expires_at_ms
        .is_some_and(|expiry| expiry < now_ms.saturating_add(REFRESH_INTERVAL_SECS as i64 * 1000))
}

#[cfg(not(target_os = "macos"))]
fn apply_refresh(creds: &mut Value, response: &RefreshResponse, now_ms: i64) {
    let oauth = &mut creds["claudeAiOauth"];
    oauth["accessToken"] = json!(response.access_token);
    oauth["expiresAt"] = json!(now_ms.saturating_add(response.expires_in.saturating_mul(1000)));
    if let Some(token) = &response.refresh_token {
        oauth["refreshToken"] = json!(token);
    }
    if let Some(seconds) = response.refresh_token_expires_in {
        oauth["refreshTokenExpiresAt"] = json!(now_ms.saturating_add(seconds.saturating_mul(1000)));
    }
}

#[cfg(not(target_os = "macos"))]
fn write_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let temp = path.with_extension("json.tmp-agent-gauge");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&temp)?;
    let written = serde_json::to_writer(&file, value)
        .map_err(std::io::Error::from)
        .and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(not(target_os = "macos"))]
fn read_file_credentials(path: &Path) -> Result<(Value, OauthBlob), String> {
    let file = fs::File::open(path)
        .map_err(|_| "Inicia sesión en Claude Code (`claude`) para ver tus límites.")?;
    let value: Value = serde_json::from_reader(BufReader::new(file)).map_err(|_| {
        "No se pudieron leer las credenciales. Abre Claude Code y vuelve a iniciar sesión."
    })?;
    let oauth = serde_json::from_value(value["claudeAiOauth"].clone())
        .map_err(|_| "Inicia sesión con tu cuenta de Claude en `claude` para ver los límites.")?;
    Ok((value, oauth))
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsFile {
    claude_ai_oauth: Option<OauthBlob>,
}

/// Reads the stored credentials. macOS keeps them in the login Keychain;
/// Linux and Windows keep them in `~/.claude/.credentials.json`.
#[cfg(target_os = "macos")]
fn read_credentials() -> Result<OauthBlob, String> {
    let raw = read_credentials_raw()?;
    let parsed: CredentialsFile = serde_json::from_str(&raw)
        .map_err(|e| format!("No se pudieron leer las credenciales: {e}"))?;
    parsed.claude_ai_oauth.ok_or_else(|| {
        "Inicia sesión con tu cuenta de Claude en `claude` para ver los límites.".into()
    })
}

#[cfg(target_os = "macos")]
fn read_credentials_raw() -> Result<String, String> {
    let out = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
    }
    let path = credentials_path()?;
    fs::read_to_string(&path)
        .map_err(|_| "Inicia sesión en Claude Code (`claude`) para ver tus límites.".to_string())
}

fn credentials_path() -> Result<PathBuf, String> {
    config_dir()
        .map(|dir| dir.join(".credentials.json"))
        .ok_or_else(|| "No se encontró la carpeta de usuario.".into())
}

#[derive(Deserialize, Debug)]
struct ApiWindow {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ApiUsage {
    five_hour: Option<ApiWindow>,
    seven_day: Option<ApiWindow>,
}

fn to_window(w: Option<ApiWindow>, minutes: u32) -> Option<LimitWindow> {
    w.map(|w| LimitWindow {
        used_percent: w.utilization,
        resets_at: w.resets_at,
        window_minutes: minutes,
    })
}

type Limits = (Option<LimitWindow>, Option<LimitWindow>, Option<String>);

/// `parse_usage` reports HTTP 429 as this prefix followed by the Retry-After seconds.
const RATE_LIMITED: &str = "429:";
/// After a 429, wait this long and double it each time (capped), so a rate
/// limit is never prolonged by our own retries.
const BACKOFF_MIN_SECS: u64 = 120;
const BACKOFF_MAX_SECS: u64 = 900;

/// Last good limits, kept on disk so a restart shows numbers right away.
fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("agent-gauge").join("claude-limits.json"))
}

fn load_cached() -> Option<Limits> {
    serde_json::from_str(&fs::read_to_string(cache_path()?).ok()?).ok()
}

fn save_cached(limits: &Limits) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(limits) {
        let _ = fs::write(path, json);
    }
}

/// Seconds to wait before the next call, given the last result and the current backoff.
fn next_wait(result: &Result<Limits, String>, backoff: &mut u64) -> u64 {
    match result {
        Err(msg) if msg.starts_with(RATE_LIMITED) => {
            let retry_after = msg[RATE_LIMITED.len()..].parse::<u64>().unwrap_or(0);
            *backoff = (*backoff * 2).clamp(BACKOFF_MIN_SECS, BACKOFF_MAX_SECS);
            (*backoff).max(retry_after)
        }
        _ => {
            *backoff = BACKOFF_MIN_SECS / 2;
            REMOTE_TTL_SECS
        }
    }
}

/// Caches the remote call so the widget can poll often without hammering the API.
pub struct ClaudeSource {
    last_remote: Option<Result<Limits, String>>,
    next_attempt: Option<Instant>,
    backoff_secs: u64,
    #[cfg(not(target_os = "macos"))]
    last_refresh_attempt: Option<Instant>,
    #[cfg(not(target_os = "macos"))]
    unpersisted_refresh_token: Option<String>,
    client: reqwest::Client,
}

const REMOTE_TTL_SECS: u64 = 60;

impl ClaudeSource {
    pub fn new() -> Self {
        Self {
            last_remote: load_cached().map(Ok),
            next_attempt: None,
            backoff_secs: BACKOFF_MIN_SECS / 2,
            #[cfg(not(target_os = "macos"))]
            last_refresh_attempt: None,
            #[cfg(not(target_os = "macos"))]
            unpersisted_refresh_token: None,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("agent-gauge/0.1")
                .build()
                .expect("http client"),
        }
    }

    pub async fn read(&mut self) -> ProviderUsage {
        let tokens = config_dir().map(|d| tokens_last_5h(&d.join("projects")));
        let installed = config_dir().map(|d| d.exists()).unwrap_or(false);
        if !installed {
            return ProviderUsage::empty(
                Provider::Claude,
                Status::Unavailable,
                "Claude Code no está instalado en este equipo.",
            );
        }

        let due = self.next_attempt.is_none_or(|at| Instant::now() >= at);
        if due {
            let result = self.fetch_remote().await;
            // Every attempt, good or bad, sets the next one: failures must not retry each poll.
            let wait = next_wait(&result, &mut self.backoff_secs);
            self.next_attempt = Some(Instant::now() + std::time::Duration::from_secs(wait));
            match result {
                Ok(limits) => {
                    save_cached(&limits);
                    self.last_remote = Some(Ok(limits));
                }
                Err(msg) => {
                    // Keep the previous good numbers (rate limit, offline...).
                    let keep_previous = matches!(self.last_remote, Some(Ok(_)));
                    #[cfg(not(target_os = "macos"))]
                    let keep_previous = keep_previous && msg != REFRESH_FAILED;
                    if !keep_previous {
                        let msg = if msg.starts_with(RATE_LIMITED) {
                            format!(
                                "Anthropic limita las consultas ahora mismo. Se reintentará en {} min.",
                                wait.div_ceil(60)
                            )
                        } else {
                            msg
                        };
                        self.last_remote = Some(Err(msg));
                    }
                }
            }
        }

        let now = Utc::now().to_rfc3339();
        match &self.last_remote {
            Some(Ok((primary, secondary, plan))) => ProviderUsage {
                provider: Provider::Claude,
                status: Status::Ok,
                message: None,
                plan: plan.clone(),
                primary: primary.clone(),
                secondary: secondary.clone(),
                tokens_5h: tokens,
                observed_at: Some(now),
            },
            Some(Err(msg)) => ProviderUsage {
                // Local token counts still work without the remote limits.
                tokens_5h: tokens,
                ..ProviderUsage::empty(Provider::Claude, Status::Error, msg.clone())
            },
            None => ProviderUsage::empty(Provider::Claude, Status::Error, "Sin datos todavía."),
        }
    }

    #[cfg(target_os = "macos")]
    async fn fetch_remote(&mut self) -> Result<Limits, String> {
        let creds = read_credentials()?;
        if creds
            .expires_at
            .is_some_and(|exp| exp < Utc::now().timestamp_millis())
        {
            return Err(
                "La sesión de Claude Code caducó. Abre `claude` una vez para renovarla.".into(),
            );
        }
        let response = self.fetch_usage(&creds).await?;
        Self::parse_usage(response, creds.subscription_type).await
    }

    #[cfg(not(target_os = "macos"))]
    async fn fetch_remote(&mut self) -> Result<Limits, String> {
        let path = credentials_path()?;
        let (_, mut creds) = read_file_credentials(&path)?;
        let expiring = needs_refresh(creds.expires_at, Utc::now().timestamp_millis());
        if expiring {
            creds = self
                .refresh_credentials(&path, &creds, false, TOKEN_URL)
                .await?;
        }
        let mut response = self.fetch_usage(&creds).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && !expiring {
            creds = self
                .refresh_credentials(&path, &creds, true, TOKEN_URL)
                .await?;
            response = self.fetch_usage(&creds).await?;
        }
        Self::parse_usage(response, creds.subscription_type).await
    }

    #[cfg(not(target_os = "macos"))]
    async fn refresh_credentials(
        &mut self,
        path: &Path,
        previous: &OauthBlob,
        unauthorized: bool,
        token_url: &str,
    ) -> Result<OauthBlob, String> {
        let (value, latest) = read_file_credentials(path).map_err(|_| REFRESH_FAILED)?;
        let now = Utc::now().timestamp_millis();
        let rejected = unauthorized && latest.access_token == previous.access_token;
        if !rejected && !needs_refresh(latest.expires_at, now) {
            return Ok(latest);
        }
        if self
            .last_refresh_attempt
            .is_some_and(|attempt| attempt.elapsed().as_secs() < REFRESH_INTERVAL_SECS)
        {
            return Err(REFRESH_FAILED.into());
        }
        let oauth = &value["claudeAiOauth"];
        let refresh_token = oauth["refreshToken"]
            .as_str()
            .filter(|token| !token.is_empty())
            .ok_or(REFRESH_FAILED)?;
        if self.unpersisted_refresh_token.as_deref() == Some(refresh_token)
            || oauth["refreshTokenExpiresAt"]
                .as_i64()
                .is_some_and(|expires| expires <= now)
        {
            return Err(REFRESH_FAILED.into());
        }
        let scope = oauth["scopes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        self.last_refresh_attempt = Some(Instant::now());
        let response = self
            .client
            .post(token_url)
            .json(&json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": CLIENT_ID,
                "scope": scope,
            }))
            .send()
            .await
            .map_err(|_| REFRESH_FAILED)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(REFRESH_FAILED.into());
        }
        let refreshed: RefreshResponse = response.json().await.map_err(|_| REFRESH_FAILED)?;
        if refreshed.refresh_token.is_some() {
            self.unpersisted_refresh_token = Some(refresh_token.to_owned());
        }
        // Preserve unrelated edits made while the request was in flight.
        let (mut current, current_oauth) =
            read_file_credentials(path).map_err(|_| REFRESH_FAILED)?;
        if current_oauth.access_token != latest.access_token
            || current["claudeAiOauth"]["refreshToken"].as_str() != Some(refresh_token)
        {
            return Ok(current_oauth);
        }
        apply_refresh(&mut current, &refreshed, Utc::now().timestamp_millis());
        write_atomic(path, &current).map_err(|_| REFRESH_FAILED)?;
        self.unpersisted_refresh_token = None;
        serde_json::from_value(current["claudeAiOauth"].take()).map_err(|_| REFRESH_FAILED.into())
    }

    async fn fetch_usage(&self, creds: &OauthBlob) -> Result<reqwest::Response, String> {
        self.client
            .get(USAGE_URL)
            .bearer_auth(&creds.access_token)
            .header("anthropic-beta", OAUTH_BETA)
            .send()
            .await
            .map_err(|_| "Sin conexión con Anthropic. Se reintentará en un minuto.".to_string())
    }

    async fn parse_usage(resp: reqwest::Response, plan: Option<String>) -> Result<Limits, String> {
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(0);
            return Err(format!("{RATE_LIMITED}{retry_after}"));
        }
        if !resp.status().is_success() {
            return Err(format!(
                "Anthropic respondió {}. Se reintentará en un minuto.",
                resp.status().as_u16()
            ));
        }
        let body: ApiUsage = resp
            .json()
            .await
            .map_err(|_| "Respuesta inesperada de Anthropic. Se reintentará en un minuto.")?;
        Ok((
            to_window(body.five_hour, 300),
            to_window(body.seven_day, 10_080),
            plan,
        ))
    }
}

/// Sums input + output + cache tokens from transcript lines in the last 5 hours.
fn tokens_last_5h(projects: &Path) -> u64 {
    let cutoff = Utc::now() - Duration::hours(5);
    let cutoff_sys: SystemTime = cutoff.into();
    let mut seen = HashSet::new();
    let mut total = 0u64;

    let Ok(project_dirs) = fs::read_dir(projects) else { return 0 };
    for dir in project_dirs.flatten() {
        let Ok(files) = fs::read_dir(dir.path()) else { continue };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let recent = file
                .metadata()
                .and_then(|m| m.modified())
                .map(|m| m >= cutoff_sys)
                .unwrap_or(false);
            if !recent {
                continue;
            }
            let Ok(f) = fs::File::open(&path) else { continue };
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                if !line.contains("\"usage\"") {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                let ts = v
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok());
                if ts.map(|t| t < cutoff).unwrap_or(true) {
                    continue;
                }
                let msg = &v["message"];
                // The same assistant message is logged once per content block.
                if let Some(id) = msg.get("id").and_then(|i| i.as_str()) {
                    if !seen.insert(id.to_string()) {
                        continue;
                    }
                }
                let u = &msg["usage"];
                for key in [
                    "input_tokens",
                    "output_tokens",
                    "cache_creation_input_tokens",
                    "cache_read_input_tokens",
                ] {
                    total += u.get(key).and_then(|n| n.as_u64()).unwrap_or(0);
                }
            }
        }
    }
    total
}

/// Current OAuth access token (already refreshed by `ClaudeSource::read` when
/// needed). Only used to call Anthropic.
pub fn access_token() -> Option<String> {
    #[cfg(not(target_os = "macos"))]
    {
        let path = config_dir()?.join(".credentials.json");
        read_file_credentials(&path).ok().map(|(_, oauth)| oauth.access_token)
    }
    #[cfg(target_os = "macos")]
    {
        read_credentials().ok().map(|oauth| oauth.access_token)
    }
}

/// Organization of the signed-in account, from Claude Code's `.claude.json`.
pub fn organization_uuid() -> Option<String> {
    let file = match std::env::var("CLAUDE_CONFIG_DIR") {
        Ok(dir) => PathBuf::from(dir).join(".claude.json"),
        Err(_) => dirs::home_dir()?.join(".claude.json"),
    };
    let value: Value = serde_json::from_str(&fs::read_to_string(file).ok()?).ok()?;
    value["oauthAccount"]["organizationUuid"].as_str().map(str::to_owned)
}


#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn needs_refresh_window() {
        let now = 1_000_000;
        assert!(needs_refresh(Some(now - 1), now));
        assert!(needs_refresh(Some(now + 240_000), now));
        assert!(!needs_refresh(Some(now + 300_000), now));
        assert!(!needs_refresh(Some(now + 600_000), now));
        assert!(!needs_refresh(None, now));
    }

    #[test]
    fn apply_refresh_keeps_other_keys() {
        let mut creds = json!({
            "claudeAiOauth": {
                "accessToken": "a", "refreshToken": "r", "expiresAt": 1,
                "scopes": ["user:inference"], "subscriptionType": "pro"
            },
            "mcpOAuth": {"x": {"k": 1}}
        });
        apply_refresh(
            &mut creds,
            &RefreshResponse {
                access_token: "A2".into(),
                refresh_token: Some("R2".into()),
                expires_in: 3600,
                refresh_token_expires_in: Some(86400),
            },
            1_000,
        );
        assert_eq!(
            creds,
            json!({
                "claudeAiOauth": {
                    "accessToken": "A2", "refreshToken": "R2", "expiresAt": 3_601_000,
                    "refreshTokenExpiresAt": 86_401_000,
                    "scopes": ["user:inference"], "subscriptionType": "pro"
                },
                "mcpOAuth": {"x": {"k": 1}}
            })
        );
    }

    #[test]
    fn write_atomic_replaces_file() {
        let dir = std::env::temp_dir().join(format!("ag-refresh-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("credentials.json");
        fs::write(&path, r#"{"original":true}"#).unwrap();
        let next = json!({"updated": true});
        let result = write_atomic(&path, &next);
        let actual = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        result.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&actual).unwrap(),
            next
        );
        assert!(!path.with_extension("json.tmp-agent-gauge").exists());
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn refresh_persists_rotated_tokens_from_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        let dir = std::env::temp_dir().join(format!("ag-refresh-http-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("credentials.json");
        let original = json!({
            "claudeAiOauth": {
                "accessToken": "synthetic-access", "refreshToken": "synthetic-refresh",
                "expiresAt": 1, "scopes": ["user:inference", "user:profile"]
            },
            "mcpOAuth": {"keep": true}
        });
        fs::write(&path, original.to_string()).unwrap();
        let (_, previous) = read_file_credentials(&path).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line, "POST /token HTTP/1.1\r\n");
            let mut length = 0;
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                json!({
                    "grant_type": "refresh_token", "refresh_token": "synthetic-refresh",
                    "client_id": CLIENT_ID, "scope": "user:inference user:profile"
                })
            );
            let body = r#"{"access_token":"rotated-access","refresh_token":"rotated-refresh","expires_in":3600}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });

        let mut source = ClaudeSource::new();
        source.client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let creds = tauri::async_runtime::block_on(
            source.refresh_credentials(&path, &previous, false, &url),
        )
        .unwrap();
        server.join().unwrap();
        let (saved, _) = read_file_credentials(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_dir(dir).unwrap();
        assert_eq!(creds.access_token, "rotated-access");
        assert_eq!(saved["claudeAiOauth"]["refreshToken"], "rotated-refresh");
        assert_eq!(
            saved["claudeAiOauth"]["scopes"],
            original["claudeAiOauth"]["scopes"]
        );
        assert_eq!(saved["mcpOAuth"], original["mcpOAuth"]);
    }
}

#[cfg(test)]
mod backoff_tests {
    use super::*;

    #[test]
    fn rate_limits_back_off_and_success_resets() {
        let mut backoff = BACKOFF_MIN_SECS / 2;
        let limited: Result<Limits, String> = Err(format!("{RATE_LIMITED}0"));
        assert_eq!(next_wait(&limited, &mut backoff), 120);
        assert_eq!(next_wait(&limited, &mut backoff), 240);
        assert_eq!(next_wait(&limited, &mut backoff), 480);
        assert_eq!(next_wait(&limited, &mut backoff), 900);
        assert_eq!(next_wait(&limited, &mut backoff), 900, "capped at 15 min");
        let ok: Result<Limits, String> = Ok((None, None, None));
        assert_eq!(next_wait(&ok, &mut backoff), REMOTE_TTL_SECS);
        assert_eq!(next_wait(&limited, &mut backoff), 120, "starts over after a success");
    }

    #[test]
    fn retry_after_wins_when_longer() {
        let mut backoff = BACKOFF_MIN_SECS / 2;
        let limited: Result<Limits, String> = Err(format!("{RATE_LIMITED}1800"));
        assert_eq!(next_wait(&limited, &mut backoff), 1800);
    }

    #[test]
    fn other_errors_wait_the_normal_minute() {
        let mut backoff = BACKOFF_MIN_SECS / 2;
        let offline: Result<Limits, String> = Err("Sin conexión".into());
        assert_eq!(next_wait(&offline, &mut backoff), REMOTE_TTL_SECS);
    }
}
