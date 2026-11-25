//! Claude Code sessions running in Anthropic's cloud on GitHub repositories.
//!
//! Listed with the same endpoint Claude Code uses to find your web sessions
//! (`/v1/sessions`, beta `ccr-byoc-2025-07-29`), authenticated with the
//! Claude Code login. Only metadata is used: repository, title, status,
//! last update and context usage. Cached for a minute; HTTP 429 backs off.

use crate::model::{Chat, Provider};
use crate::tail::{AWAKE_SECS, LIST_SECS};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use std::time::{Duration, Instant};

const SESSIONS_URL: &str = "https://api.anthropic.com/v1/sessions?beta=true";
const BETA: &str = "ccr-byoc-2025-07-29";
const TTL_SECS: u64 = 60;

/// `owner/name` of the session's first GitHub repository.
fn repo_of(session: &Value) -> Option<String> {
    let ctx = &session["session_context"];
    if let Some(repo) = ctx["outcomes"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|o| o["git_info"]["repo"].as_str())
    {
        return Some(repo.to_owned());
    }
    ctx["sources"].as_array().into_iter().flatten().find_map(|s| {
        let url = s["url"].as_str()?;
        let path = url.trim_end_matches(".git").split("github.com/").nth(1)?;
        Some(path.trim_end_matches('/').to_owned())
    })
}

/// Turns a `/v1/sessions` page into chats of the "cloud" environment.
pub fn parse_sessions(body: &Value, now: DateTime<Utc>) -> Vec<Chat> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            if s["environment_kind"].as_str() != Some("anthropic_cloud") {
                return None;
            }
            let updated = DateTime::parse_from_rfc3339(s["updated_at"].as_str()?)
                .ok()?
                .with_timezone(&Utc);
            let age = (now - updated).num_seconds().max(0) as u64;
            if age > LIST_SECS {
                return None;
            }
            let running = s["session_status"].as_str() == Some("running")
                || s["status_bucket"].as_str() == Some("running");
            let usage = &s["external_metadata"]["context_usage"];
            let context_percent = match (usage["used_tokens"].as_f64(), usage["max_tokens"].as_f64()) {
                (Some(used), Some(max)) if max > 0.0 => Some((used / max * 100.0).min(100.0)),
                _ => None,
            };
            let repo = repo_of(s);
            Some(Chat {
                id: s["id"].as_str()?.to_owned(),
                provider: Provider::Claude,
                title: s["title"]
                    .as_str()
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
                    .or_else(|| repo.clone())
                    .unwrap_or_else(|| "Sesión en la nube".into()),
                cwd: repo,
                last_activity: updated.to_rfc3339_opts(SecondsFormat::Nanos, true),
                working: running,
                asleep: !running && age > AWAKE_SECS,
                env_id: "cloud".into(),
                from_claude: false,
                context_percent,
                context_approx: false,
                subagents: vec![],
                more_subagents: 0,
            })
        })
        .collect()
}

pub struct CloudClaude {
    client: reqwest::Client,
    next_attempt: Option<Instant>,
    backoff_secs: u64,
    last: Vec<Chat>,
}

impl CloudClaude {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .user_agent("agent-gauge/0.1")
                .build()
                .expect("http client"),
            next_attempt: None,
            backoff_secs: 60,
            last: vec![],
        }
    }

    /// Cloud sessions of the last 24 h; the previous list when a call fails.
    pub async fn read(&mut self) -> Vec<Chat> {
        if self.next_attempt.is_some_and(|at| Instant::now() < at) {
            return self.last.clone();
        }
        let (result, rate_limited) = self.fetch().await;
        let wait = if rate_limited {
            self.backoff_secs = (self.backoff_secs * 2).clamp(120, 900);
            self.backoff_secs
        } else {
            self.backoff_secs = 60;
            TTL_SECS
        };
        self.next_attempt = Some(Instant::now() + Duration::from_secs(wait));
        if let Some(chats) = result {
            self.last = chats;
        }
        self.last.clone()
    }

    async fn fetch(&self) -> (Option<Vec<Chat>>, bool) {
        let (Some(token), Some(org)) = (crate::claude::access_token(), crate::claude::organization_uuid()) else {
            return (None, false);
        };
        let resp = match self
            .client
            .get(SESSIONS_URL)
            .bearer_auth(token)
            .header("anthropic-beta", BETA)
            .header("anthropic-version", "2023-06-01")
            .header("x-organization-uuid", org)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(_) => return (None, false),
        };
        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return (None, true);
        }
        if !resp.status().is_success() {
            return (None, false);
        }
        match resp.json::<Value>().await {
            Ok(body) => (Some(parse_sessions(&body, Utc::now())), false),
            Err(_) => (None, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(id: &str, updated: &str, status: &str, repo_url: &str) -> Value {
        json!({
            "id": id, "title": "Mi tarea", "environment_kind": "anthropic_cloud",
            "session_status": status, "status_bucket": "completed", "updated_at": updated,
            "session_context": {"sources": [{"type": "git_repository", "url": repo_url}], "outcomes": []},
            "external_metadata": {"context_usage": {"max_tokens": 1000000, "used_tokens": 250000}}
        })
    }

    #[test]
    fn cloud_sessions_become_chats_grouped_by_repo() {
        let now = DateTime::parse_from_rfc3339("2026-11-25T12:00:00Z").unwrap().with_timezone(&Utc);
        let body = json!({"data": [
            session("a", "2026-11-25T11:59:50Z", "running", "https://github.com/me/app"),
            session("b", "2026-11-25T09:00:00Z", "idle", "https://github.com/me/web.git"),
            session("old", "2026-11-23T09:00:00Z", "idle", "https://github.com/me/x"),
            {"id": "byoc", "environment_kind": "byoc", "updated_at": "2026-11-25T11:59:50Z"}
        ]});
        let chats = parse_sessions(&body, now);
        assert_eq!(chats.len(), 2, "old and non-cloud sessions are skipped");
        let a = &chats[0];
        assert_eq!(a.cwd.as_deref(), Some("me/app"));
        assert_eq!(a.env_id, "cloud");
        assert!(a.working && !a.asleep);
        assert_eq!(a.context_percent, Some(25.0));
        let b = &chats[1];
        assert_eq!(b.cwd.as_deref(), Some("me/web"));
        assert!(!b.working && b.asleep);
    }

    #[test]
    fn repo_prefers_outcome_git_info() {
        let s = json!({"session_context": {
            "outcomes": [{"type": "git_repository", "git_info": {"repo": "org/real", "type": "github"}}],
            "sources": [{"url": "https://github.com/org/other"}]}});
        assert_eq!(repo_of(&s).as_deref(), Some("org/real"));
    }
}
