//! Shapes shared with the frontend. Keep in sync with `src/types.ts`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

/// One rate-limit window (e.g. the rolling 5 hour session, or the weekly cap).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    /// 0–100. `None` when the provider did not report it.
    pub used_percent: Option<f64>,
    /// ISO-8601 instant when this window resets.
    pub resets_at: Option<String>,
    /// Length of the window in minutes (300 for the 5 h session, 10080 for a week).
    pub window_minutes: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Live numbers.
    Ok,
    /// The tool is not installed or the user is not logged in.
    Unavailable,
    /// Something failed; `message` says what.
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub provider: Provider,
    pub status: Status,
    pub message: Option<String>,
    /// Plan name as reported (e.g. "max", "pro", "plus").
    pub plan: Option<String>,
    /// Short window (5 h session).
    pub primary: Option<LimitWindow>,
    /// Long window (weekly).
    pub secondary: Option<LimitWindow>,
    /// Tokens counted from local logs over the last 5 hours.
    pub tokens_5h: Option<u64>,
    /// When these numbers were observed (ISO-8601).
    pub observed_at: Option<String>,
}

impl ProviderUsage {
    pub fn empty(provider: Provider, status: Status, message: impl Into<String>) -> Self {
        Self {
            provider,
            status,
            message: Some(message.into()),
            plan: None,
            primary: None,
            secondary: None,
            tokens_5h: None,
            observed_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub id: String,
    pub name: String,
    pub kind: Option<String>,
    pub last_activity: String,
    pub working: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    pub provider: Provider,
    pub title: String,
    pub cwd: Option<String>,
    pub last_activity: String,
    pub working: bool,
    pub from_claude: bool,
    pub context_percent: Option<f64>,
    pub context_approx: bool,
    pub subagents: Vec<Subagent>,
    pub more_subagents: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub claude: ProviderUsage,
    pub codex: ProviderUsage,
    pub chats: Vec<Chat>,
    pub taken_at: String,
}
