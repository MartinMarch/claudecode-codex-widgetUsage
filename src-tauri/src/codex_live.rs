//! Live Codex limits, the same numbers `/status` shows.
//!
//! The session logs only carry `rate_limits` when Codex runs a turn on this
//! machine, so they go stale whenever Codex is used elsewhere (another
//! computer, an SSH host, the cloud). Here we ask OpenAI directly with the
//! login Codex already keeps in `$CODEX_HOME/auth.json`.
//!
//! The token is only read, never refreshed or rewritten, and only sent to
//! chatgpt.com. When anything fails the log-based numbers stay in place.

use crate::activity::{self, LogLevel};
use crate::model::{LimitWindow, ProviderUsage, Status};
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::path::Path;
use std::time::Instant;

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const TTL_SECS: u64 = 60;
const NO_LOGIN: &str = "No hay login de Codex (`codex login`)";

#[derive(Deserialize)]
struct Auth {
    tokens: Option<Tokens>,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    account_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct WhamUsage {
    plan_type: Option<String>,
    rate_limit: Option<WhamRateLimit>,
}

#[derive(Deserialize, Debug, Clone)]
struct WhamRateLimit {
    primary_window: Option<WhamWindow>,
    secondary_window: Option<WhamWindow>,
}

#[derive(Deserialize, Debug, Clone)]
struct WhamWindow {
    used_percent: Option<f64>,
    limit_window_seconds: Option<u64>,
    reset_at: Option<i64>,
}

fn window(w: &WhamWindow, default_minutes: u32) -> LimitWindow {
    LimitWindow {
        used_percent: w.used_percent,
        resets_at: w
            .reset_at
            .and_then(|s| Utc.timestamp_opt(s, 0).single())
            .map(|t| t.to_rfc3339()),
        window_minutes: w
            .limit_window_seconds
            .map(|s| u32::try_from(s / 60).unwrap_or(default_minutes))
            .unwrap_or(default_minutes),
    }
}

/// Puts the live limits over the log-based reading, keeping its local token count.
pub fn overlay(local: ProviderUsage, live: &WhamUsage) -> ProviderUsage {
    let Some(limits) = &live.rate_limit else {
        return local;
    };
    let primary = limits.primary_window.as_ref().map(|w| window(w, 300));
    if primary.is_none() {
        return local;
    }
    ProviderUsage {
        status: Status::Ok,
        message: None,
        plan: live.plan_type.clone().or(local.plan),
        primary,
        secondary: limits.secondary_window.as_ref().map(|w| window(w, 10_080)),
        observed_at: Some(Utc::now().to_rfc3339()),
        ..local
    }
}

pub struct CodexLive {
    client: reqwest::Client,
    last: Option<(Instant, Option<WhamUsage>)>,
}

impl CodexLive {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .user_agent("agent-gauge/0.1")
                .build()
                .expect("http client"),
            last: None,
        }
    }

    pub fn force_next(&mut self) {
        self.last = None;
    }

    /// Latest live limits, cached for a minute. `None` when unavailable.
    pub async fn read(&mut self, codex_home: Option<&Path>) -> Option<WhamUsage> {
        let fresh = self
            .last
            .as_ref()
            .is_some_and(|(t, _)| t.elapsed().as_secs() < TTL_SECS);
        if !fresh {
            let result = match codex_home {
                Some(home) => self.fetch(home).await,
                None => Err((LogLevel::Warn, NO_LOGIN.into())),
            };
            let result = match result {
                Ok(usage) => {
                    let limits = usage.rate_limit.as_ref();
                    activity::record_usage(
                        "Codex",
                        limits
                            .and_then(|rate| rate.primary_window.as_ref())
                            .and_then(|window| window.used_percent),
                        limits
                            .and_then(|rate| rate.secondary_window.as_ref())
                            .and_then(|window| window.used_percent),
                    );
                    Some(usage)
                }
                Err((level, message)) => {
                    activity::record("Codex", level, message);
                    None
                }
            };
            // A failed attempt keeps the previous good numbers.
            let keep = result.is_none() && matches!(self.last, Some((_, Some(_))));
            if keep {
                if let Some((t, _)) = &mut self.last {
                    *t = Instant::now();
                }
            } else {
                self.last = Some((Instant::now(), result));
            }
        }
        self.last.as_ref().and_then(|(_, u)| u.clone())
    }

    async fn fetch(&self, home: &Path) -> Result<WhamUsage, (LogLevel, String)> {
        let raw = std::fs::read_to_string(home.join("auth.json")).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                (LogLevel::Warn, NO_LOGIN.into())
            } else {
                (LogLevel::Error, "No se pudo leer el login de Codex".into())
            }
        })?;
        let tokens = serde_json::from_str::<Auth>(&raw)
            .map_err(|_| (LogLevel::Error, "No se pudo leer el login de Codex".into()))?
            .tokens
            .ok_or_else(|| (LogLevel::Warn, NO_LOGIN.into()))?;
        let mut req = self.client.get(USAGE_URL).bearer_auth(&tokens.access_token);
        if let Some(account) = &tokens.account_id {
            req = req.header("ChatGPT-Account-Id", account);
        }
        let resp = req
            .send()
            .await
            .map_err(|_| (LogLevel::Error, "Sin conexión con OpenAI".into()))?;
        if !resp.status().is_success() {
            return Err((
                LogLevel::Error,
                format!("OpenAI respondió {}", resp.status().as_u16()),
            ));
        }
        resp.json::<WhamUsage>()
            .await
            .map_err(|_| (LogLevel::Error, "Respuesta inesperada de OpenAI".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    // Shape of a real response (Codex 0.171, 2026-11-25), identifiers removed.
    const BODY: &str = r#"{"plan_type":"plus","rate_limit":{"allowed":true,"limit_reached":false,
        "primary_window":{"used_percent":41,"limit_window_seconds":18000,"reset_after_seconds":17181,"reset_at":4102444800},
        "secondary_window":{"used_percent":51,"limit_window_seconds":604800,"reset_after_seconds":522911,"reset_at":4102444800}},
        "credits":{"has_credits":false}}"#;

    fn local() -> ProviderUsage {
        ProviderUsage {
            tokens_5h: Some(1234),
            ..ProviderUsage::empty(Provider::Codex, Status::Unavailable, "sin datos")
        }
    }

    #[test]
    fn live_limits_replace_stale_log_numbers() {
        let live: WhamUsage = serde_json::from_str(BODY).unwrap();
        let u = overlay(local(), &live);
        assert!(matches!(u.status, Status::Ok));
        assert!(u.message.is_none());
        assert_eq!(u.plan.as_deref(), Some("plus"));
        let p = u.primary.unwrap();
        assert_eq!(p.used_percent, Some(41.0));
        assert_eq!(p.window_minutes, 300);
        assert_eq!(p.resets_at.as_deref(), Some("2100-01-01T00:00:00+00:00"));
        let s = u.secondary.unwrap();
        assert_eq!(s.used_percent, Some(51.0));
        assert_eq!(s.window_minutes, 10_080);
        assert_eq!(u.tokens_5h, Some(1234), "local token count is kept");
    }

    #[test]
    fn response_without_limits_keeps_local_reading() {
        let live: WhamUsage = serde_json::from_str(r#"{"plan_type":"plus"}"#).unwrap();
        let u = overlay(local(), &live);
        assert!(matches!(u.status, Status::Unavailable));
        assert!(u.primary.is_none());
    }
}
