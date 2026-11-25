use crate::model::{Chat, Provider, Subagent};
use crate::tail::{activity, folder_name, read_head, read_tail, TAIL_BYTES};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

pub fn read(root: &Path, now: SystemTime) -> Vec<Chat> {
    let mut chats = Vec::new();
    let Ok(projects) = fs::read_dir(root.join("projects")) else {
        return chats;
    };
    for project in projects.flatten() {
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        let project_name = project.file_name();
        chats.extend(
            files
                .flatten()
                .filter_map(|file| read_chat(&file.path(), &project_name.to_string_lossy(), now)),
        );
    }
    chats
}

fn read_chat(path: &Path, project: &str, now: SystemTime) -> Option<Chat> {
    if path.extension()?.to_str()? != "jsonl" || !path.is_file() {
        return None;
    }
    let id = path.file_stem()?.to_str()?.to_owned();
    let (last_activity, working) = activity(path, now)?;
    // The folder the chat started in names its project. Later lines follow the
    // agent's `cd`s into subfolders, which would split one project in two.
    let start_cwd = read_head(path, TAIL_BYTES).lines().find_map(|line| {
        serde_json::from_str::<Value>(line).ok()?["cwd"].as_str().map(str::to_owned)
    });
    let mut cwd = None;
    let mut title = None;
    let mut context_percent = None;
    let mut context_approx = false;
    let mut large_context = false;
    for line in read_tail(path, TAIL_BYTES).lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(dir) = entry["cwd"].as_str() {
            cwd = Some(dir.to_owned());
        }
        match entry["type"].as_str() {
            Some("custom-title") => {
                if let Some(value) = entry["customTitle"].as_str() {
                    title = Some(value.to_owned());
                }
            }
            Some("assistant") => {
                let message = &entry["message"];
                let Some(usage) = message["usage"].as_object() else {
                    continue;
                };
                let tokens = [
                    "input_tokens",
                    "cache_creation_input_tokens",
                    "cache_read_input_tokens",
                ]
                .iter()
                .map(|key| usage.get(*key).and_then(Value::as_u64).unwrap_or_default())
                .fold(0u64, u64::saturating_add);
                let (window, approx) =
                    context_window(message["model"].as_str().unwrap_or_default());
                large_context |= tokens > window;
                let window = if large_context { 1_000_000 } else { window };
                context_percent = Some((tokens as f64 / window as f64 * 100.0).min(100.0));
                context_approx = approx || large_context;
            }
            _ => {}
        }
    }

    // A chat that started at a filesystem root ("/" on an SSH host) names
    // itself after the folder it moved into instead.
    let cwd = start_cwd.filter(|c| folder_name(c).is_some()).or(cwd);
    Some(Chat {
        title: title.unwrap_or_else(|| {
            cwd.as_deref()
                .and_then(folder_name)
                .unwrap_or(project)
                .to_owned()
        }),
        subagents: read_subagents(&path.with_extension("").join("subagents"), now),
        id,
        provider: Provider::Claude,
        cwd,
        last_activity,
        working,
        from_claude: false,
        context_percent,
        context_approx,
        more_subagents: 0,
    })
}

fn read_subagents(dir: &Path, now: SystemTime) -> Vec<Subagent> {
    let Ok(files) = fs::read_dir(dir) else {
        return vec![];
    };
    files
        .flatten()
        .filter_map(|file| {
            let path = file.path();
            if path.extension()?.to_str()? != "jsonl" || !path.is_file() {
                return None;
            }
            let id = path.file_stem()?.to_str()?.strip_prefix("agent-")?;
            let (last_activity, working) = activity(&path, now)?;
            let meta: Value = fs::read(path.with_extension("meta.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
            if meta["finished"].as_bool() == Some(true) {
                return None;
            }
            let kind = meta["agentType"].as_str();
            Some(Subagent {
                id: id.to_owned(),
                name: meta["description"]
                    .as_str()
                    .or(kind)
                    .unwrap_or(id)
                    .to_owned(),
                kind: kind.map(str::to_owned),
                last_activity,
                working,
            })
        })
        .collect()
}

pub fn context_window(model: &str) -> (u64, bool) {
    (200_000, !model.starts_with("claude-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const USER: &str = r#"{"type":"user","cwd":"C:\\x\\proj"}"#;
    const ASSISTANT: &str = r#"{"type":"assistant","cwd":"C:\\x\\proj","message":{"model":"claude-opus-5-5","usage":{"input_tokens":4,"cache_creation_input_tokens":96,"cache_read_input_tokens":99900,"output_tokens":10}}}"#;

    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            Self(std::env::temp_dir().join(format!("ag-claude-{}-{name}", std::process::id())))
        }

        fn write(&self, path: &str, contents: &str, age: u64) {
            let path = self.0.join("projects/project").join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        }

        fn read(&self) -> Vec<Chat> {
            read(&self.0, SystemTime::now())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn active_chat_with_title_cwd_and_context() {
        let f = Fixture::new("active");
        f.write(
            "chat.jsonl",
            &format!(
                "{USER}\n{ASSISTANT}\n{}",
                r#"{"type":"custom-title","customTitle":"Mi chat"}"#
            ),
            0,
        );
        let chats = f.read();
        assert_eq!(chats.len(), 1);
        let chat = &chats[0];
        assert_eq!(chat.title, "Mi chat");
        assert_eq!(chat.cwd.as_deref(), Some(r"C:\x\proj"));
        assert_eq!(chat.context_percent, Some(50.0));
        assert!(!chat.context_approx);
        assert!(chat.working);
        assert_eq!(chat.provider, Provider::Claude);
    }

    #[test]
    fn old_chat_is_not_listed() {
        let f = Fixture::new("old");
        f.write("chat.jsonl", ASSISTANT, 601);
        assert!(f.read().is_empty());
    }

    #[test]
    fn new_chat_without_assistant_turn() {
        let f = Fixture::new("new");
        f.write("chat.jsonl", USER, 0);
        let chats = f.read();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].title, "proj");
        assert_eq!(chats[0].context_percent, None);
    }

    #[test]
    fn project_folder_is_where_the_chat_started() {
        let f = Fixture::new("cd");
        let later = ASSISTANT.replace(r"C:\\x\\proj", r"C:\\x\\proj\\src-tauri");
        f.write("chat.jsonl", &format!("{USER}\n{later}\n"), 0);
        assert_eq!(f.read()[0].cwd.as_deref(), Some(r"C:\x\proj"));
    }

    #[test]
    fn half_written_last_line_is_skipped() {
        let f = Fixture::new("half");
        f.write(
            "chat.jsonl",
            &format!("{ASSISTANT}\n{{\"type\":\"assist"),
            0,
        );
        assert_eq!(f.read()[0].context_percent, Some(50.0));
    }

    #[test]
    fn over_window_switches_to_1m() {
        let f = Fixture::new("large-context");
        f.write("chat.jsonl", &ASSISTANT.replace("99900", "399900"), 0);
        let chats = f.read();
        assert_eq!(chats[0].context_percent, Some(40.0));
        assert!(chats[0].context_approx);
    }

    #[test]
    fn running_and_finished_subagents() {
        let f = Fixture::new("subagents");
        f.write("chat.jsonl", USER, 0);
        f.write("chat/subagents/agent-a.jsonl", "", 0);
        f.write(
            "chat/subagents/agent-a.meta.json",
            r#"{"agentType":"Explore","description":"Buscar tests","finished":false}"#,
            0,
        );
        f.write("chat/subagents/agent-b.jsonl", "", 0);
        f.write(
            "chat/subagents/agent-b.meta.json",
            r#"{"agentType":"web-fetch","finished":true}"#,
            0,
        );
        let chats = f.read();
        let agents = &chats[0].subagents;
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "Buscar tests");
        assert_eq!(agents[0].kind.as_deref(), Some("Explore"));
    }
}
