//! Codex sessions running on SSH hosts.
//!
//! When a Claude desktop chat works over SSH, Codex runs on that host and its
//! logs stay there. The desktop app records each chat's `sshConfig` (host and
//! key) in its session files; for chats active in the last 10 minutes we run
//! `remote_codex.sh` over SSH (key auth only, `BatchMode`, short timeout) and
//! turn its metadata lines into `Chat`s. Results are cached per host.

use crate::model::{Chat, Provider, Subagent};
use crate::tail::{folder_name, ACTIVE_SECS, WORKING_SECS};
use chrono::{SecondsFormat, TimeZone, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("remote_codex.sh");
const CACHE_SECS: u64 = 30;
const SSH_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub identity: Option<PathBuf>,
}

/// Folders where the Claude desktop app keeps its Claude Code session files.
fn session_roots() -> Vec<PathBuf> {
    if let Ok(dir) = std::env::var("AGENT_GAUGE_CLAUDE_SESSIONS") {
        return vec![PathBuf::from(dir)];
    }
    let Some(local) = dirs::data_local_dir() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(local.join("Claude-Data")) else { return vec![] };
    entries
        .flatten()
        .map(|e| e.path().join("claude-code-sessions"))
        .filter(|p| p.is_dir())
        .collect()
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => dirs::home_dir().map_or_else(|| PathBuf::from(path), |h| h.join(rest)),
        None => PathBuf::from(path),
    }
}

/// SSH hosts of desktop chats active in the last `ACTIVE_SECS`, one per host.
pub fn active_targets(roots: &[PathBuf], now_ms: i64) -> Vec<SshTarget> {
    let mut out: Vec<SshTarget> = Vec::new();
    let mut stack: Vec<(PathBuf, u8)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if !(name.starts_with("local_") && name.ends_with(".json")) {
                continue;
            }
            let Some(target) = target_from_file(&path, now_ms) else { continue };
            if !out.iter().any(|t| t.host == target.host) {
                out.push(target);
            }
        }
    }
    out
}

fn target_from_file(path: &Path, now_ms: i64) -> Option<SshTarget> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let ssh = v.get("sshConfig")?;
    let host = ssh["sshHost"].as_str().filter(|h| !h.is_empty())?;
    if v["isArchived"].as_bool() == Some(true) {
        return None;
    }
    let last = v["lastActivityAt"].as_i64()?;
    if now_ms - last > (ACTIVE_SECS as i64) * 1000 {
        return None;
    }
    Some(SshTarget {
        host: host.to_owned(),
        identity: ssh["sshIdentityFile"].as_str().map(expand_home),
    })
}

#[derive(Default)]
struct Session {
    id: Option<String>,
    cwd: Option<String>,
    originator: Option<String>,
    parent: Option<String>,
    nickname: Option<String>,
    input_tokens: Option<u64>,
    window: Option<u64>,
    modified: i64,
    now: i64,
}

/// Turns `remote_codex.sh` output into chats, subagents nested under their parent.
pub fn parse(output: &str) -> Vec<Chat> {
    let mut sessions: Vec<Session> = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(3, '\t');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("F"), Some(m), Some(now)) => sessions.push(Session {
                modified: m.trim().parse().unwrap_or_default(),
                now: now.trim().parse().unwrap_or_default(),
                ..Session::default()
            }),
            (Some("M"), Some(kv), None) => {
                let Some(s) = sessions.last_mut() else { continue };
                let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&format!("{{{kv}}}")) else {
                    continue;
                };
                for (key, value) in map {
                    let Some(value) = value.as_str().map(str::to_owned) else { continue };
                    let slot = match key.as_str() {
                        "id" => &mut s.id,
                        "cwd" => &mut s.cwd,
                        "originator" => &mut s.originator,
                        "parent_thread_id" => &mut s.parent,
                        "agent_nickname" => &mut s.nickname,
                        _ => continue,
                    };
                    slot.get_or_insert(value);
                }
            }
            (Some("T"), Some(kv), None) => {
                let Some(s) = sessions.last_mut() else { continue };
                let Some((key, n)) = kv.split_once(':') else { continue };
                let n = n.trim().parse::<u64>().ok();
                match key.trim_matches('"') {
                    "input_tokens" => s.input_tokens = n,
                    "model_context_window" => s.window = n,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let stamp = |secs: i64| {
        Utc.timestamp_opt(secs, 0)
            .single()
            .map(|t| t.to_rfc3339_opts(SecondsFormat::Nanos, true))
            .unwrap_or_default()
    };
    let mut chats: Vec<Chat> = Vec::new();
    let mut subs: Vec<(String, Subagent)> = Vec::new();
    for s in sessions {
        let Some(id) = s.id.clone() else { continue };
        let age = s.now.saturating_sub(s.modified).max(0) as u64;
        if age > ACTIVE_SECS {
            continue;
        }
        let working = age <= WORKING_SECS;
        if let Some(parent) = &s.parent {
            subs.push((
                parent.clone(),
                Subagent {
                    id: id.clone(),
                    name: s.nickname.clone().unwrap_or_else(|| "Subagente".into()),
                    kind: None,
                    last_activity: stamp(s.modified),
                    working,
                },
            ));
            // Also keep it as a chat, in case its parent is not active.
        }
        let context_percent = match (s.input_tokens, s.window) {
            (Some(t), Some(w)) if w > 0 => Some((t as f64 / w as f64 * 100.0).min(100.0)),
            _ => None,
        };
        chats.push(Chat {
            title: s
                .nickname
                .clone()
                .or_else(|| s.cwd.as_deref().and_then(folder_name).map(str::to_owned))
                .unwrap_or_else(|| "Codex".into()),
            id,
            provider: Provider::Codex,
            cwd: s.cwd,
            last_activity: stamp(s.modified),
            working,
            from_claude: s.originator.as_deref() == Some("Claude Code"),
            context_percent,
            context_approx: false,
            subagents: vec![],
            more_subagents: 0,
        });
    }
    // Subagents whose parent is listed move under it; orphans stay as chats.
    let parents: Vec<String> = chats.iter().map(|c| c.id.clone()).collect();
    for (parent, sub) in subs {
        if parents.contains(&parent) {
            chats.retain(|c| c.id != sub.id);
            if let Some(p) = chats.iter_mut().find(|c| c.id == parent) {
                p.subagents.push(sub);
            }
        }
    }
    chats
}

fn run_ssh(target: &SshTarget) -> Option<String> {
    let mut cmd = Command::new("ssh");
    cmd.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=4", "-o", "StrictHostKeyChecking=accept-new"]);
    if let Some(key) = &target.identity {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(&target.host)
        .arg("sh -s")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing
    }
    let mut child = cmd.spawn().ok()?;
    // A Windows checkout may turn the script's line endings into CRLF; sh needs LF.
    let script = SCRIPT.replace('\r', "");
    child.stdin.take()?.write_all(script.as_bytes()).ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if started.elapsed() > SSH_TIMEOUT => {
                let _ = child.kill();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(out)
}

type HostCache = Mutex<HashMap<String, (Instant, Vec<Chat>)>>;

fn cache() -> &'static HostCache {
    static CACHE: OnceLock<HostCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Active Codex chats on every SSH host a recent desktop chat is connected to.
pub fn read(now_ms: i64) -> Vec<Chat> {
    let mut all = Vec::new();
    for target in active_targets(&session_roots(), now_ms) {
        let cached = cache()
            .lock()
            .ok()
            .and_then(|c| c.get(&target.host).filter(|(t, _)| t.elapsed().as_secs() < CACHE_SECS).map(|(_, v)| v.clone()));
        let chats = cached.unwrap_or_else(|| {
            // A host we cannot reach is remembered as empty for the cache period.
            let chats = run_ssh(&target).map(|out| parse(&out)).unwrap_or_default();
            if let Ok(mut c) = cache().lock() {
                c.insert(target.host.clone(), (Instant::now(), chats.clone()));
            }
            chats
        });
        all.extend(chats);
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "F\t1000\t1010\n\
M\t\"id\":\"P1\"\n\
M\t\"cwd\":\"/root/homelab\"\n\
M\t\"originator\":\"Claude Code\"\n\
T\t\"input_tokens\":237500\n\
T\t\"model_context_window\":475000\n\
F\t995\t1010\n\
M\t\"id\":\"S1\"\n\
M\t\"cwd\":\"/root/homelab\"\n\
M\t\"parent_thread_id\":\"P1\"\n\
M\t\"agent_nickname\":\"Pauli\"\n\
F\t100\t1010\n\
M\t\"id\":\"OLD\"\n";

    #[test]
    fn parses_remote_sessions_and_nests_subagents() {
        let chats = parse(OUT);
        assert_eq!(chats.len(), 1, "old session dropped, subagent nested");
        let c = &chats[0];
        assert_eq!(c.id, "P1");
        assert_eq!(c.title, "homelab");
        assert_eq!(c.cwd.as_deref(), Some("/root/homelab"));
        assert!(c.from_claude);
        assert!(c.working);
        assert_eq!(c.context_percent, Some(50.0));
        assert_eq!(c.subagents.len(), 1);
        assert_eq!(c.subagents[0].name, "Pauli");
    }

    #[test]
    fn orphan_remote_subagent_stays_a_chat() {
        let out = "F\t1000\t1005\nM\t\"id\":\"S1\"\nM\t\"parent_thread_id\":\"GONE\"\nM\t\"agent_nickname\":\"Erdos\"\n";
        let chats = parse(out);
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].title, "Erdos");
    }

    #[test]
    fn finds_active_ssh_chats_only() {
        let dir = std::env::temp_dir().join(format!("ag-remote-{}", std::process::id()));
        let nested = dir.join("acct").join("org");
        std::fs::create_dir_all(&nested).unwrap();
        let write = |name: &str, body: &str| std::fs::write(nested.join(name), body).unwrap();
        write("local_a.json", r#"{"lastActivityAt":1000000,"sshConfig":{"sshHost":"root@10.0.0.5","sshIdentityFile":"/k/id"}}"#);
        write("local_b.json", r#"{"lastActivityAt":1000000,"sshConfig":{"sshHost":"root@10.0.0.5"}}"#);
        write("local_old.json", r#"{"lastActivityAt":1,"sshConfig":{"sshHost":"root@10.0.0.9"}}"#);
        write("local_plain.json", r#"{"lastActivityAt":1000000}"#);
        write("local_arch.json", r#"{"lastActivityAt":1000000,"isArchived":true,"sshConfig":{"sshHost":"root@10.0.0.7"}}"#);
        let targets = active_targets(std::slice::from_ref(&dir), 1_000_000 + 5_000);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(targets.len(), 1, "deduped by host; old, plain and archived skipped");
        assert_eq!(targets[0].host, "root@10.0.0.5");
    }
}
