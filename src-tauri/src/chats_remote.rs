//! Codex sessions running on SSH hosts.
//!
//! When a Claude desktop chat works over SSH, Codex runs on that host and its
//! logs stay there. The desktop app records each chat's `sshConfig` (host and
//! key) in its session files; for chats active in the last 24 hours we run
//! `remote_codex.sh` over SSH (key auth only, `BatchMode`, short timeout) and
//! turn its metadata lines into `Chat`s. Results are cached per host.

use crate::model::{Chat, EnvKind, Environment, Provider, Subagent};
use crate::tail::{folder_name, AWAKE_SECS, LIST_SECS, WORKING_SECS};
use chrono::{SecondsFormat, TimeZone, Utc};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("remote_codex.sh");
const CACHE_SECS: u64 = 30;
const SESSION_CACHE_SECS: u64 = 60;
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
    let Some(local) = dirs::data_local_dir() else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(local.join("Claude-Data")) else {
        return vec![];
    };
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

#[derive(Default)]
struct SessionIndex {
    hosts: HashMap<String, String>,
    targets: Vec<(i64, SshTarget)>,
}

impl SessionIndex {
    fn active_targets(&self, now_ms: i64) -> Vec<SshTarget> {
        let mut seen = BTreeSet::new();
        self.targets
            .iter()
            .filter(|(last, target)| {
                now_ms.saturating_sub(*last) <= LIST_SECS as i64 * 1000
                    && seen.insert(target.host.as_str())
            })
            .map(|(_, target)| target.clone())
            .collect()
    }
}

fn scan_sessions(roots: &[PathBuf]) -> SessionIndex {
    let mut index = SessionIndex::default();
    let mut stack: Vec<(PathBuf, u8)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !(name.starts_with("local_") && name.ends_with(".json")) {
                continue;
            }
            let Ok(raw) = std::fs::read(&path) else {
                continue;
            };
            let Ok(session) = serde_json::from_slice::<Value>(&raw) else {
                continue;
            };
            let ssh = &session["sshConfig"];
            let Some(host) = ssh["sshHost"].as_str().filter(|host| !host.is_empty()) else {
                continue;
            };
            if let Some(id) = session["cliSessionId"].as_str() {
                index.hosts.insert(id.to_owned(), host.to_owned());
            }
            if let Some(last) = session["lastActivityAt"]
                .as_i64()
                .filter(|_| session["isArchived"].as_bool() != Some(true))
            {
                index.targets.push((
                    last,
                    SshTarget {
                        host: host.to_owned(),
                        identity: ssh["sshIdentityFile"].as_str().map(expand_home),
                    },
                ));
            }
        }
    }
    index.targets.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
    index
}

fn with_sessions<T>(f: impl FnOnce(&SessionIndex) -> T) -> T {
    static SESSIONS: OnceLock<Mutex<(Instant, SessionIndex)>> = OnceLock::new();
    let mut cached = SESSIONS
        .get_or_init(|| Mutex::new((Instant::now(), scan_sessions(&session_roots()))))
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    if cached.0.elapsed().as_secs() >= SESSION_CACHE_SECS {
        *cached = (Instant::now(), scan_sessions(&session_roots()));
    }
    f(&cached.1)
}

pub fn host_for_cli_session(cli_session_id: &str) -> Option<String> {
    with_sessions(|index| index.hosts.get(cli_session_id).cloned())
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
pub fn parse(output: &str, host: &str) -> (Option<String>, Vec<Chat>) {
    let mut hostname = None;
    let mut sessions: Vec<Session> = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(3, '\t');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("H"), Some(name), None) if !name.trim().is_empty() => {
                hostname.get_or_insert_with(|| name.trim().to_owned());
            }
            (Some("F"), Some(m), Some(now)) => sessions.push(Session {
                modified: m.trim().parse().unwrap_or_default(),
                now: now.trim().parse().unwrap_or_default(),
                ..Session::default()
            }),
            (Some("M"), Some(kv), None) => {
                let Some(s) = sessions.last_mut() else {
                    continue;
                };
                let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&format!("{{{kv}}}"))
                else {
                    continue;
                };
                for (key, value) in map {
                    let Some(value) = value.as_str().map(str::to_owned) else {
                        continue;
                    };
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
                let Some(s) = sessions.last_mut() else {
                    continue;
                };
                let Some((key, n)) = kv.split_once(':') else {
                    continue;
                };
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
        if age > LIST_SECS {
            continue;
        }
        let working = age <= WORKING_SECS;
        let asleep = age > AWAKE_SECS;
        if let Some(parent) = &s.parent {
            subs.push((
                parent.clone(),
                Subagent {
                    id: id.clone(),
                    name: s.nickname.clone().unwrap_or_else(|| "Subagente".into()),
                    kind: None,
                    last_activity: stamp(s.modified),
                    working,
                    asleep,
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
            env_id: format!("ssh:{host}"),
            provider: Provider::Codex,
            cwd: s.cwd,
            last_activity: stamp(s.modified),
            working,
            asleep,
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
    (hostname, chats)
}

fn run_ssh(target: &SshTarget) -> Option<String> {
    let mut cmd = Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=4",
        "-o",
        "StrictHostKeyChecking=accept-new",
    ]);
    if let Some(key) = &target.identity {
        cmd.arg("-i").arg(key);
    }
    cmd.arg("--")
        .arg(&target.host)
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
    // Drain output concurrently so the 24-hour listing cannot fill the SSH pipe.
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut output = String::new();
        stdout.read_to_string(&mut output).ok().map(|_| output)
    });
    // A Windows checkout may turn the script's line endings into CRLF; sh needs LF.
    let script = SCRIPT.replace('\r', "");
    if child.stdin.take()?.write_all(script.as_bytes()).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if started.elapsed() > SSH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    reader.join().ok().flatten()
}

struct HostReading {
    taken_at: Instant,
    hostname: Option<String>,
    chats: Vec<Chat>,
}

type HostCache = Mutex<HashMap<String, HostReading>>;

fn cache() -> &'static HostCache {
    static CACHE: OnceLock<HostCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Active Codex chats on every SSH host a recent desktop chat is connected to.
pub fn read(now_ms: i64) -> Vec<Chat> {
    let mut all = Vec::new();
    for target in with_sessions(|index| index.active_targets(now_ms)) {
        let cached = cache().lock().ok().and_then(|c| {
            c.get(&target.host)
                .filter(|reading| reading.taken_at.elapsed().as_secs() < CACHE_SECS)
                .map(|reading| reading.chats.clone())
        });
        let chats = cached.unwrap_or_else(|| {
            // A host we cannot reach is remembered as empty for the cache period.
            let (hostname, chats) = run_ssh(&target)
                .map(|out| parse(&out, &target.host))
                .unwrap_or_default();
            if let Ok(mut c) = cache().lock() {
                let hostname = hostname.or_else(|| {
                    c.get(&target.host)
                        .and_then(|reading| reading.hostname.clone())
                });
                c.insert(
                    target.host.clone(),
                    HostReading {
                        taken_at: Instant::now(),
                        hostname,
                        chats: chats.clone(),
                    },
                );
            }
            chats
        });
        all.extend(chats);
    }
    all
}

pub fn environments() -> Vec<Environment> {
    let mut hosts: BTreeSet<_> = with_sessions(|index| index.hosts.values().cloned().collect());
    let names: HashMap<_, _> = cache()
        .lock()
        .map(|cache| {
            hosts.extend(cache.keys().cloned());
            cache
                .iter()
                .filter_map(|(host, reading)| Some((host.clone(), reading.hostname.clone()?)))
                .collect()
        })
        .unwrap_or_default();
    let config = dirs::home_dir()
        .and_then(|home| std::fs::read_to_string(home.join(".ssh").join("config")).ok())
        .unwrap_or_default();

    hosts
        .into_iter()
        .map(|host| Environment {
            id: format!("ssh:{host}"),
            name: names
                .get(&host)
                .cloned()
                .or_else(|| ssh_alias(&config, &host))
                .unwrap_or(host),
            kind: EnvKind::Ssh,
        })
        .collect()
}

fn ssh_alias(config: &str, target: &str) -> Option<String> {
    let (user, hostname) = target.rsplit_once('@')?;
    let mut alias = None;
    let mut configured_user = None;
    let mut configured_hostname = None;
    // Only literal Host entries can be displayed as aliases.
    for line in config.lines().chain(std::iter::once("Host")) {
        let line = line.split('#').next()?.trim();
        let key_end = line
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(line.len());
        let (key, value) = line.split_at(key_end);
        let value = value
            .trim_start_matches(|c: char| c.is_whitespace() || c == '=')
            .trim();
        if key.eq_ignore_ascii_case("Host") || key.eq_ignore_ascii_case("Match") {
            if configured_user == Some(user)
                && configured_hostname.is_some_and(|name: &str| name.eq_ignore_ascii_case(hostname))
            {
                if let Some(alias) = alias {
                    return Some(alias);
                }
            }
            alias = key
                .eq_ignore_ascii_case("Host")
                .then(|| {
                    value
                        .split_whitespace()
                        .map(|alias| alias.trim_matches('"'))
                        .find(|alias| !alias.is_empty() && !alias.contains(['*', '?', '!']))
                        .map(str::to_owned)
                })
                .flatten();
            configured_user = None;
            configured_hostname = None;
        } else if key.eq_ignore_ascii_case("User") {
            configured_user.get_or_insert(value.trim_matches('"'));
        } else if key.eq_ignore_ascii_case("HostName") {
            configured_hostname.get_or_insert(value.trim_matches('"'));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "H\tbeelink\n\
F\t100000\t100010\n\
M\t\"id\":\"P1\"\n\
M\t\"cwd\":\"/root/homelab\"\n\
M\t\"originator\":\"Claude Code\"\n\
T\t\"input_tokens\":237500\n\
T\t\"model_context_window\":475000\n\
F\t99995\t100010\n\
M\t\"id\":\"S1\"\n\
M\t\"cwd\":\"/root/homelab\"\n\
M\t\"parent_thread_id\":\"P1\"\n\
M\t\"agent_nickname\":\"Pauli\"\n\
F\t92810\t100010\n\
M\t\"id\":\"SLEEPING\"\n\
F\t100\t100010\n\
M\t\"id\":\"OLD\"\n";

    #[test]
    fn parses_remote_sessions_and_nests_subagents() {
        let (hostname, chats) = parse(OUT, "user@synthetic");
        assert_eq!(hostname.as_deref(), Some("beelink"));
        assert_eq!(chats.len(), 2, "old session dropped, subagent nested");
        let c = &chats[0];
        assert_eq!(c.id, "P1");
        assert_eq!(c.title, "homelab");
        assert_eq!(c.cwd.as_deref(), Some("/root/homelab"));
        assert!(c.from_claude);
        assert!(c.working);
        assert_eq!(c.env_id, "ssh:user@synthetic");
        assert!(!c.asleep);
        assert!(chats[1].asleep && !chats[1].working);
        assert_eq!(c.context_percent, Some(50.0));
        assert_eq!(c.subagents.len(), 1);
        assert_eq!(c.subagents[0].name, "Pauli");
    }

    #[test]
    fn orphan_remote_subagent_stays_a_chat() {
        let out = "F\t1000\t1005\nM\t\"id\":\"S1\"\nM\t\"parent_thread_id\":\"GONE\"\nM\t\"agent_nickname\":\"Erdos\"\n";
        let (_, chats) = parse(out, "user@synthetic");
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].title, "Erdos");
    }

    #[test]
    fn finds_active_ssh_chats_only() {
        let dir = std::env::temp_dir().join(format!("ag-remote-{}", std::process::id()));
        let nested = dir.join("acct").join("org");
        std::fs::create_dir_all(&nested).unwrap();
        let write = |name: &str, body: &str| std::fs::write(nested.join(name), body).unwrap();
        write(
            "local_a.json",
            r#"{"cliSessionId":"synthetic-cli","lastActivityAt":1000000,"sshConfig":{"sshHost":"root@10.0.0.5","sshIdentityFile":"/k/id"}}"#,
        );
        write(
            "local_b.json",
            r#"{"lastActivityAt":1000000,"sshConfig":{"sshHost":"root@10.0.0.5"}}"#,
        );
        write(
            "local_old.json",
            r#"{"lastActivityAt":1,"sshConfig":{"sshHost":"root@10.0.0.9"}}"#,
        );
        write("local_plain.json", r#"{"lastActivityAt":1000000}"#);
        write(
            "local_arch.json",
            r#"{"lastActivityAt":1000000,"isArchived":true,"sshConfig":{"sshHost":"root@10.0.0.7"}}"#,
        );
        let index = scan_sessions(std::slice::from_ref(&dir));
        assert_eq!(
            index.hosts.get("synthetic-cli").map(String::as_str),
            Some("root@10.0.0.5")
        );
        let targets = index.active_targets(87_000_000);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(
            targets.len(),
            1,
            "deduped by host; old, plain and archived skipped"
        );
        assert_eq!(targets[0].host, "root@10.0.0.5");
    }

    #[test]
    fn ssh_config_alias_matches_hostname_and_user() {
        let config = "Host *\n  User nobody\nHost wrong\n  HostName example.test\n  User other\nHost beelink\n  HostName example.test\n  User dev # lab\n";
        assert_eq!(
            ssh_alias(config, "dev@example.test").as_deref(),
            Some("beelink")
        );
        assert_eq!(ssh_alias(config, "missing@example.test"), None);
    }
}
