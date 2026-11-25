//! Codex usage, read entirely from local session logs.
//!
//! Every Codex session writes `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`.
//! After each model turn it appends an `event_msg` of type `token_count` that
//! carries the account's `rate_limits` (primary = 5 h window, secondary =
//! weekly). The newest such event is the freshest picture of the account.
//!
//! Field names have changed between Codex versions (`resets_in_seconds` vs
//! `resets_at`), so parsing is tolerant and everything is optional.

use crate::model::{LimitWindow, Provider, ProviderUsage, Status};
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn home() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CODEX_HOME") {
        return Some(PathBuf::from(dir));
    }
    dirs::home_dir().map(|h| h.join(".codex"))
}

/// Session files from the most recent days, newest first.
fn recent_session_files(sessions: &Path, max_days: usize) -> Vec<(SystemTime, PathBuf)> {
    fn sorted_dirs(p: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default();
        v.sort();
        v.reverse();
        v
    }
    let mut day_dirs = Vec::new();
    'outer: for y in sorted_dirs(sessions) {
        for m in sorted_dirs(&y) {
            for d in sorted_dirs(&m) {
                day_dirs.push(d);
                if day_dirs.len() >= max_days {
                    break 'outer;
                }
            }
        }
    }
    let mut files: Vec<(SystemTime, PathBuf)> = day_dirs
        .iter()
        .flat_map(|d| fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files
}

fn parse_window(v: &Value, observed: DateTime<Utc>, default_minutes: u32) -> Option<LimitWindow> {
    if v.is_null() {
        return None;
    }
    let minutes = v
        .get("window_minutes")
        .and_then(Value::as_u64)
        .map(|m| m as u32)
        .unwrap_or(default_minutes);
    let resets_at = if let Some(secs) = v.get("resets_at").and_then(Value::as_i64) {
        Utc.timestamp_opt(secs, 0).single()
    } else { v.get("resets_in_seconds").and_then(Value::as_i64).map(|secs| observed + Duration::seconds(secs)) };
    let mut used_percent = v.get("used_percent").and_then(Value::as_f64);
    // The log only updates when Codex runs. If the window has rolled over since,
    // the logged percentage is stale: the window started again from zero.
    let resets_at = match resets_at {
        Some(t) if t <= Utc::now() => {
            used_percent = used_percent.map(|_| 0.0);
            None
        }
        other => other,
    };
    Some(LimitWindow {
        used_percent,
        resets_at: resets_at.map(|t| t.to_rfc3339()),
        window_minutes: minutes,
    })
}

pub fn read() -> ProviderUsage {
    let Some(home) = home().filter(|h| h.exists()) else {
        return ProviderUsage::empty(Provider::Codex, Status::Unavailable, "Codex no está instalado en este equipo.");
    };
    let files = recent_session_files(&home.join("sessions"), 8);
    if files.is_empty() {
        return ProviderUsage::empty(
            Provider::Codex,
            Status::Unavailable,
            "Aún no hay sesiones de Codex. Lanza `codex` y aparecerá tu uso.",
        );
    }

    let cutoff = Utc::now() - Duration::hours(5);
    let cutoff_sys: SystemTime = cutoff.into();
    let mut latest: Option<(DateTime<Utc>, Value)> = None;
    let mut tokens = 0u64;

    for (modified, path) in &files {
        let in_window = *modified >= cutoff_sys;
        // Once we have limits, older files only matter for the 5 h token sum.
        if latest.is_some() && !in_window {
            break;
        }
        let Ok(f) = fs::File::open(path) else { continue };
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            if !line.contains("token_count") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let payload = &v["payload"];
            if payload["type"] != "token_count" {
                continue;
            }
            let Some(ts) = v["timestamp"]
                .as_str()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc))
            else {
                continue;
            };
            if ts >= cutoff {
                tokens += payload["info"]["last_token_usage"]["total_tokens"]
                    .as_u64()
                    .unwrap_or(0);
            }
            let limits = &payload["rate_limits"];
            if !limits.is_null() && latest.as_ref().map(|(t, _)| ts > *t).unwrap_or(true) {
                latest = Some((ts, limits.clone()));
            }
        }
    }

    let Some((observed, limits)) = latest else {
        return ProviderUsage {
            tokens_5h: Some(tokens),
            ..ProviderUsage::empty(
                Provider::Codex,
                Status::Unavailable,
                "Esta versión de Codex no registra límites. Actualiza Codex para verlos.",
            )
        };
    };

    ProviderUsage {
        provider: Provider::Codex,
        status: Status::Ok,
        message: None,
        plan: limits["plan_type"].as_str().map(str::to_string),
        primary: parse_window(&limits["primary"], observed, 300),
        secondary: parse_window(&limits["secondary"], observed, 10_080),
        tokens_5h: Some(tokens),
        observed_at: Some(observed.to_rfc3339()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn window_with_relative_reset() {
        let observed = Utc::now();
        let w = parse_window(
            &json!({"used_percent": 42.5, "window_minutes": 299, "resets_in_seconds": 600}),
            observed,
            300,
        )
        .unwrap();
        assert_eq!(w.used_percent, Some(42.5));
        assert_eq!(w.window_minutes, 299);
        assert_eq!(w.resets_at.unwrap(), (observed + Duration::seconds(600)).to_rfc3339());
    }

    #[test]
    fn window_with_absolute_reset_and_missing_minutes() {
        let observed = Utc::now();
        // 2100-01-01: always in the future.
        let w = parse_window(&json!({"used_percent": 3.0, "resets_at": 4102444800i64}), observed, 10_080).unwrap();
        assert_eq!(w.window_minutes, 10_080);
        assert_eq!(w.used_percent, Some(3.0));
        assert_eq!(w.resets_at.unwrap(), "2100-01-01T00:00:00+00:00");
        assert!(parse_window(&Value::Null, observed, 300).is_none());
    }

    #[test]
    fn window_that_already_reset_reads_as_zero() {
        // Real shape from Codex 0.171 (`resets_at` in Unix seconds), with a past reset.
        let observed = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let w = parse_window(
            &json!({"used_percent": 15.0, "window_minutes": 300, "resets_at": 1767272400}),
            observed,
            300,
        )
        .unwrap();
        assert_eq!(w.used_percent, Some(0.0));
        assert!(w.resets_at.is_none());
    }

    #[test]
    fn reads_latest_limits_from_session_logs() {
        let dir = std::env::temp_dir().join(format!("agent-gauge-test-{}", std::process::id()));
        let day = dir.join("sessions/2026/11/25");
        fs::create_dir_all(&day).unwrap();
        let now = Utc::now();
        let line = |ts: DateTime<Utc>, pct: f64| {
            json!({"timestamp": ts.to_rfc3339(), "type": "event_msg", "payload": {
                "type": "token_count",
                "info": {"last_token_usage": {"total_tokens": 1000}},
                "rate_limits": {"primary": {"used_percent": pct, "window_minutes": 300, "resets_in_seconds": 3600},
                                "secondary": {"used_percent": 7.0, "window_minutes": 10080, "resets_in_seconds": 3600}}
            }})
            .to_string()
        };
        fs::write(
            day.join("rollout-a.jsonl"),
            format!("{}\n{}\n{{\"type\":\"other\"}}\n", line(now - Duration::minutes(10), 10.0), line(now - Duration::minutes(1), 30.0)),
        )
        .unwrap();
        std::env::set_var("CODEX_HOME", &dir);
        let u = read();
        std::env::remove_var("CODEX_HOME");
        fs::remove_dir_all(&dir).ok();
        assert!(matches!(u.status, Status::Ok));
        assert_eq!(u.primary.unwrap().used_percent, Some(30.0));
        assert_eq!(u.secondary.unwrap().used_percent, Some(7.0));
        assert_eq!(u.tokens_5h, Some(2000));
    }
}
