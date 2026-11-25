use crate::model::Chat;
use crate::{chats_claude, chats_codex, claude, codex};
use std::time::SystemTime;

const MAX_CHATS: usize = 5;
const MAX_SUBAGENTS: usize = 3;

pub fn select(mut all: Vec<Chat>) -> Vec<Chat> {
    // Both readers emit fixed-width UTC timestamps, so lexical order is chronological.
    all.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
    if all.len() > MAX_CHATS {
        let provider = all[0].provider;
        if all[..MAX_CHATS]
            .iter()
            .all(|chat| chat.provider == provider)
        {
            if let Some(other) = all[MAX_CHATS..]
                .iter()
                .position(|chat| chat.provider != provider)
            {
                all.swap(MAX_CHATS - 1, MAX_CHATS + other);
            }
        }
        all.truncate(MAX_CHATS);
    }
    for chat in &mut all {
        chat.subagents
            .sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        let remaining = chat.subagents.len().saturating_sub(MAX_SUBAGENTS);
        chat.more_subagents = u32::try_from(remaining).unwrap_or(u32::MAX);
        chat.subagents.truncate(MAX_SUBAGENTS);
    }
    all
}

pub fn read_all(now: SystemTime) -> Vec<Chat> {
    let mut all = claude::config_dir().map_or_else(Vec::new, |root| chats_claude::read(&root, now));
    if let Some(home) = codex::home() {
        all.extend(chats_codex::read(&home, now));
    }
    select(all)
}

#[cfg(test)]
mod tests {
    use super::select;
    use crate::model::{Chat, Provider, Subagent};

    fn chat(second: u8, provider: Provider) -> Chat {
        Chat {
            id: second.to_string(),
            provider,
            title: "Prueba".into(),
            cwd: None,
            last_activity: format!("2026-11-25T00:00:{second:02}Z"),
            working: false,
            from_claude: false,
            context_percent: None,
            context_approx: false,
            subagents: vec![],
            more_subagents: 0,
        }
    }

    #[test]
    fn keeps_five_most_recent() {
        let chats = select((1..=7).map(|i| chat(i, Provider::Claude)).collect());
        let ids: Vec<_> = chats.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["7", "6", "5", "4", "3"]);
    }

    #[test]
    fn guarantees_one_codex_chat() {
        let mut all: Vec<_> = (2..=7).map(|i| chat(i, Provider::Claude)).collect();
        all.push(chat(1, Provider::Codex));
        let chats = select(all);
        let ids: Vec<_> = chats.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["7", "6", "5", "4", "1"]);
        assert_eq!(chats[4].provider, Provider::Codex);
    }

    #[test]
    fn trims_subagents_to_three() {
        let mut parent = chat(0, Provider::Claude);
        parent.subagents = (1..=5)
            .map(|i| Subagent {
                id: i.to_string(),
                name: "Prueba".into(),
                kind: None,
                last_activity: chat(i, Provider::Claude).last_activity,
                working: false,
            })
            .collect();
        let chats = select(vec![parent]);
        let ids: Vec<_> = chats[0].subagents.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["5", "4", "3"]);
        assert_eq!(chats[0].more_subagents, 2);
    }
}
