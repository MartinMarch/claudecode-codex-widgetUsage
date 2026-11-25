use crate::codex::recent_session_files;
use crate::model::{Chat, Provider, Subagent};
use crate::tail::{activity, folder_name, read_tail, ACTIVE_SECS, TAIL_BYTES};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::time::SystemTime;

pub fn read(home: &Path, now: SystemTime) -> Vec<Chat> {
    let mut files = recent_session_files(&home.join("sessions"), 2)
        .into_iter()
        .take_while(|(modified, _)| {
            now.duration_since(*modified).unwrap_or_default().as_secs() <= ACTIVE_SECS
        })
        .peekable();
    if files.peek().is_none() {
        return vec![];
    }
    let titles = read_titles(&home.join("session_index.jsonl"));
    let mut sessions: Vec<_> = files
        .filter_map(|(_, path)| read_session(&path, now, &titles))
        .collect();
    let indices: HashMap<_, _> = sessions
        .iter()
        .enumerate()
        .map(|(index, (chat, _))| (chat.id.as_str(), index))
        .collect();
    let parents: Vec<_> = sessions
        .iter()
        .map(|(_, parent)| parent.as_deref().and_then(|id| indices.get(id).copied()))
        .collect();

    let mut attached = vec![false; sessions.len()];
    for (index, parent) in parents.iter().copied().enumerate() {
        let Some(mut root) = parent else {
            continue;
        };
        // The snapshot has one level of subagents. Follow active parents to the
        // visible chat, leaving malformed cycles as standalone rows.
        for _ in 0..parents.len() {
            match parents[root] {
                Some(next) => root = next,
                None => {
                    let chat = &sessions[index].0;
                    let agent = Subagent {
                        id: chat.id.clone(),
                        name: chat.title.clone(),
                        kind: None,
                        last_activity: chat.last_activity.clone(),
                        working: chat.working,
                    };
                    sessions[root].0.subagents.push(agent);
                    attached[index] = true;
                    break;
                }
            }
        }
    }
    sessions
        .into_iter()
        .zip(attached)
        .filter_map(|((chat, _), attached)| (!attached).then_some(chat))
        .collect()
}

fn read_titles(path: &Path) -> HashMap<String, String> {
    let Ok(file) = File::open(path) else {
        return HashMap::new();
    };
    BufReader::new(file)
        .split(b'\n')
        .map_while(Result::ok)
        .filter_map(|line| {
            let entry: Value = serde_json::from_slice(&line).ok()?;
            Some((
                entry["id"].as_str()?.to_owned(),
                entry["thread_name"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn read_session(
    path: &Path,
    now: SystemTime,
    titles: &HashMap<String, String>,
) -> Option<(Chat, Option<String>)> {
    if !path.is_file() {
        return None;
    }
    let (last_activity, working) = activity(path, now)?;
    let mut first_line = Vec::new();
    BufReader::new(File::open(path).ok()?.take(TAIL_BYTES))
        .read_until(b'\n', &mut first_line)
        .ok()?;
    let meta: Value = serde_json::from_slice(&first_line).ok()?;
    if meta["type"].as_str()? != "session_meta" {
        return None;
    }
    let payload = &meta["payload"];
    let id = payload["id"]
        .as_str()
        .or_else(|| path.file_stem()?.to_str())?;
    let cwd = payload["cwd"].as_str();
    let spawn = payload["source"]["subagent"]["thread_spawn"].as_object();
    let parent = spawn
        .and_then(|s| s.get("parent_thread_id"))
        .and_then(Value::as_str);
    let title = if let Some(spawn) = spawn {
        spawn
            .get("agent_nickname")
            .and_then(Value::as_str)
            .unwrap_or("Subagente")
    } else {
        titles
            .get(id)
            .map(String::as_str)
            .or_else(|| cwd.and_then(folder_name))
            .unwrap_or(id)
    };
    let context_percent = read_tail(path, TAIL_BYTES).lines().rev().find_map(|line| {
        let entry: Value = serde_json::from_str(line).ok()?;
        let payload = &entry["payload"];
        if entry["type"].as_str()? != "event_msg" || payload["type"].as_str()? != "token_count" {
            return None;
        }
        let info = &payload["info"];
        let window = info["model_context_window"].as_u64()?;
        let input = info["last_token_usage"]["input_tokens"].as_u64()?;
        (window > 0).then(|| (input as f64 / window as f64 * 100.0).min(100.0))
    });

    Some((
        Chat {
            id: id.to_owned(),
            provider: Provider::Codex,
            title: title.to_owned(),
            cwd: cwd.map(str::to_owned),
            last_activity,
            working,
            from_claude: payload["originator"].as_str() == Some("Claude Code"),
            context_percent,
            context_approx: false,
            subagents: Vec::new(),
            more_subagents: 0,
        },
        parent.map(str::to_owned),
    ))
}

#[cfg(test)]
mod tests {
    use super::read;
    use crate::model::Provider;
    use std::{
        fs,
        path::PathBuf,
        time::{Duration, SystemTime},
    };

    const PARENT: &str = r#"{"type":"session_meta","payload":{"id":"P1","cwd":"C:\\Users\\me\\Code","originator":"Claude Code"}}"#;
    const CHILD: &str = r#"{"type":"session_meta","payload":{"id":"S1","source":{"subagent":{"thread_spawn":{"parent_thread_id":"P1","agent_nickname":"Pauli"}}}}}"#;
    const COUNT: &str = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":237500},"model_context_window":475000}}}"#;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            Self(std::env::temp_dir().join(format!("ag-codex-chats-{}-{name}", std::process::id())))
        }

        fn session(&self, file: &str, contents: &str, age: u64) {
            let dir = self.0.join("sessions/2026/11/25");
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join(file);
            fs::write(&path, contents).unwrap();
            fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn chat_from_claude_with_context() {
        let f = Fixture::new("context");
        f.session("rollout-p.jsonl", &format!("{PARENT}\n{COUNT}"), 0);
        fs::write(
            f.0.join("session_index.jsonl"),
            r#"{"id":"P1","thread_name":"graphify"}"#,
        )
        .unwrap();
        let chats = read(&f.0, SystemTime::now());
        assert_eq!(chats.len(), 1);
        let chat = &chats[0];
        assert_eq!(chat.title, "graphify");
        assert!(chat.from_claude);
        assert_eq!(chat.context_percent, Some(50.0));
        assert_eq!(chat.provider, Provider::Codex);
    }

    #[test]
    fn subagent_attaches_to_active_parent() {
        let f = Fixture::new("attached");
        f.session("rollout-p.jsonl", PARENT, 0);
        f.session("rollout-s.jsonl", CHILD, 0);
        let chats = read(&f.0, SystemTime::now());
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].id, "P1");
        assert_eq!(chats[0].subagents.len(), 1);
        assert_eq!(chats[0].subagents[0].name, "Pauli");
    }

    #[test]
    fn orphan_subagent_is_its_own_row() {
        let f = Fixture::new("orphan");
        f.session("rollout-p.jsonl", PARENT, 601);
        f.session("rollout-s.jsonl", CHILD, 0);
        let chats = read(&f.0, SystemTime::now());
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].title, "Pauli");
    }

    #[test]
    fn compressed_and_old_sessions_ignored() {
        let f = Fixture::new("ignored");
        f.session("rollout-archived.jsonl.zst", PARENT, 0);
        f.session("rollout-old.jsonl", PARENT, 601);
        assert!(read(&f.0, SystemTime::now()).is_empty());
    }
}
