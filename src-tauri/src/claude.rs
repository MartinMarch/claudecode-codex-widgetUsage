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
//! We never refresh or rewrite the user's OAuth token: if it has expired we
//! ask them to open `claude` once, which refreshes it.

use crate::model::{LimitWindow, Provider, ProviderUsage, Status};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsFile {
    claude_ai_oauth: Option<OauthBlob>,
}

/// Reads the stored credentials. macOS keeps them in the login Keychain;
/// Linux and Windows keep them in `~/.claude/.credentials.json`.
fn read_credentials() -> Result<OauthBlob, String> {
    let raw = read_credentials_raw()?;
    let parsed: CredentialsFile =
        serde_json::from_str(&raw).map_err(|e| format!("No se pudieron leer las credenciales: {e}"))?;
    parsed
        .claude_ai_oauth
        .ok_or_else(|| "Inicia sesión con tu cuenta de Claude en `claude` para ver los límites.".into())
}

fn read_credentials_raw() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
            }
        }
    }
    let path = config_dir()
        .map(|d| d.join(".credentials.json"))
        .ok_or("No se encontró la carpeta de usuario.")?;
    fs::read_to_string(&path).map_err(|_| "Inicia sesión en Claude Code (`claude`) para ver tus límites.".to_string())
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

/// Caches the remote call so the widget can poll often without hammering the API.
pub struct ClaudeSource {
    last_remote: Option<(Instant, Result<Limits, String>)>,
    client: reqwest::Client,
}

const REMOTE_TTL_SECS: u64 = 60;

impl ClaudeSource {
    pub fn new() -> Self {
        Self {
            last_remote: None,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
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

        let fresh = self
            .last_remote
            .as_ref()
            .map(|(t, _)| t.elapsed().as_secs() < REMOTE_TTL_SECS)
            .unwrap_or(false);
        if !fresh {
            let result = self.fetch_remote().await;
            // Keep the previous good numbers if this attempt failed (rate limit, offline...).
            let keep_previous = result.is_err()
                && matches!(self.last_remote, Some((_, Ok(_))));
            if !keep_previous {
                self.last_remote = Some((Instant::now(), result));
            }
        }

        let now = Utc::now().to_rfc3339();
        match &self.last_remote {
            Some((_, Ok((primary, secondary, plan)))) => ProviderUsage {
                provider: Provider::Claude,
                status: Status::Ok,
                message: None,
                plan: plan.clone(),
                primary: primary.clone(),
                secondary: secondary.clone(),
                tokens_5h: tokens,
                observed_at: Some(now),
            },
            Some((_, Err(msg))) => ProviderUsage {
                // Local token counts still work without the remote limits.
                tokens_5h: tokens,
                ..ProviderUsage::empty(Provider::Claude, Status::Error, msg.clone())
            },
            None => ProviderUsage::empty(Provider::Claude, Status::Error, "Sin datos todavía."),
        }
    }

    async fn fetch_remote(&self) -> Result<Limits, String> {
        let creds = read_credentials()?;
        if let Some(exp) = creds.expires_at {
            if exp < Utc::now().timestamp_millis() {
                return Err("La sesión de Claude Code caducó. Abre `claude` una vez para renovarla.".into());
            }
        }
        let resp = self
            .client
            .get(USAGE_URL)
            .bearer_auth(&creds.access_token)
            .header("anthropic-beta", OAUTH_BETA)
            .send()
            .await
            .map_err(|_| "Sin conexión con Anthropic. Se reintentará en un minuto.".to_string())?;
        if !resp.status().is_success() {
            return Err(format!(
                "Anthropic respondió {}. Se reintentará en un minuto.",
                resp.status().as_u16()
            ));
        }
        let body: ApiUsage = resp
            .json()
            .await
            .map_err(|e| format!("Respuesta inesperada de Anthropic: {e}"))?;
        Ok((
            to_window(body.five_hour, 300),
            to_window(body.seven_day, 10_080),
            creds.subscription_type,
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
