use crate::model::{Chat, CrewKind, CrewMember, Project, Provider, Subagent};
use crate::tail::folder_name;
use crate::{chats_claude, chats_codex, claude, codex};
use std::collections::BTreeMap;
use std::time::SystemTime;

const MAX_PROJECTS: usize = 4;
const MAX_CREW: usize = 8;

pub fn normalize_cwd(cwd: &str) -> String {
    let cwd = cwd.trim_end_matches(['\\', '/']);
    if cfg!(windows) {
        cwd.to_lowercase()
    } else {
        cwd.to_owned()
    }
}

pub fn group(all: Vec<Chat>) -> Vec<Project> {
    let mut folders = BTreeMap::<_, Vec<_>>::new();
    for chat in all {
        folders
            .entry(chat.cwd.as_deref().map(normalize_cwd))
            .or_default()
            .push(chat);
    }
    adopt_nested(&mut folders);
    let mut projects: Vec<_> = folders
        .into_iter()
        .map(|(id, chats)| project(id.unwrap_or_else(|| "sin-carpeta".into()), chats))
        .collect();
    projects.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
    projects.truncate(MAX_PROJECTS);
    projects
}

/// `child` is `parent` or a folder inside it (paths already normalized).
fn nested(parent: &str, child: &str) -> bool {
    child
        .strip_prefix(parent)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['\\', '/']))
}

/// A folder with no Claude chat joins the closest Claude project above or
/// below it, so a Codex that Claude started from a subfolder (or the repo
/// root) lands under that Claude instead of showing as a separate project.
fn adopt_nested(folders: &mut BTreeMap<Option<String>, Vec<Chat>>) {
    let has_boss = |chats: &Vec<Chat>| chats.iter().any(|c| c.provider == Provider::Claude);
    let bosses: Vec<String> = folders
        .iter()
        .filter(|(_, chats)| has_boss(chats))
        .filter_map(|(key, _)| key.clone())
        .collect();
    let orphans: Vec<String> = folders
        .iter()
        .filter(|(_, chats)| !has_boss(chats))
        .filter_map(|(key, _)| key.clone())
        .collect();
    for orphan in orphans {
        let home = bosses
            .iter()
            .filter(|boss| nested(boss, &orphan) || nested(&orphan, boss))
            .max_by_key(|boss| boss.len());
        if let Some(home) = home {
            let chats = folders.remove(&Some(orphan)).unwrap_or_default();
            folders.entry(Some(home.clone())).or_default().extend(chats);
        }
    }
}

fn project(id: String, mut chats: Vec<Chat>) -> Project {
    // The readers emit fixed-width UTC timestamps, so lexical order is chronological.
    let boss_index = chats
        .iter()
        .enumerate()
        .filter(|(_, chat)| chat.provider == Provider::Claude)
        .max_by(|(_, a), (_, b)| a.last_activity.cmp(&b.last_activity))
        .map(|(index, _)| index);
    let mut boss = boss_index.map(|index| chats.remove(index));
    let cwd = boss
        .as_ref()
        .or_else(|| chats.first())
        .and_then(|chat| chat.cwd.clone());
    let name = cwd
        .as_deref()
        .map_or("Sin carpeta", |cwd| folder_name(cwd).unwrap_or(cwd));

    let mut crew = Vec::new();
    if let Some(boss) = &mut boss {
        crew.extend(subagent_members(
            std::mem::take(&mut boss.subagents),
            CrewKind::ClaudeSub,
        ));
        boss.more_subagents = 0;
    }
    for chat in chats {
        let kind = match chat.provider {
            Provider::Claude => CrewKind::ClaudeChat,
            Provider::Codex => {
                crew.extend(subagent_members(chat.subagents, CrewKind::CodexSub));
                CrewKind::Codex
            }
        };
        crew.push(CrewMember {
            id: chat.id,
            kind,
            name: chat.title,
            working: chat.working,
            context_percent: chat.context_percent,
            last_activity: chat.last_activity,
        });
    }

    let last_activity = crew
        .iter()
        .map(|member| member.last_activity.as_str())
        .chain(boss.iter().map(|chat| chat.last_activity.as_str()))
        .max()
        .unwrap_or_default()
        .to_owned();
    let working =
        boss.as_ref().is_some_and(|chat| chat.working) || crew.iter().any(|member| member.working);
    crew.sort_by(|a, b| {
        b.working
            .cmp(&a.working)
            .then_with(|| b.last_activity.cmp(&a.last_activity))
    });
    let more_crew = u32::try_from(crew.len().saturating_sub(MAX_CREW)).unwrap_or(u32::MAX);
    crew.truncate(MAX_CREW);
    Project {
        id,
        name: name.to_owned(),
        cwd,
        last_activity,
        working,
        boss,
        crew,
        more_crew,
    }
}

fn subagent_members(agents: Vec<Subagent>, kind: CrewKind) -> impl Iterator<Item = CrewMember> {
    agents.into_iter().map(move |agent| CrewMember {
        id: agent.id,
        kind,
        name: agent.name,
        working: agent.working,
        context_percent: None,
        last_activity: agent.last_activity,
    })
}

pub fn read_all(now: SystemTime) -> Vec<Project> {
    let mut all = claude::config_dir().map_or_else(Vec::new, |root| chats_claude::read(&root, now));
    if let Some(home) = codex::home() {
        all.extend(chats_codex::read(&home, now));
    }
    let now_ms = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    all.extend(crate::chats_remote::read(now_ms));
    group(all)
}

#[cfg(test)]
mod tests {
    use super::group;
    use crate::model::{Chat, CrewKind, Provider, Subagent};

    fn chat(second: u8, provider: Provider) -> Chat {
        Chat {
            id: second.to_string(),
            provider,
            title: "Prueba".into(),
            cwd: None,
            last_activity: format!("2026-11-25T00:00:{second:02}Z"),
            working: false,
            from_claude: false,
            context_percent: Some(f64::from(second)),
            context_approx: false,
            subagents: vec![],
            more_subagents: 0,
        }
    }

    #[test]
    fn codex_in_parent_or_child_folder_joins_the_claude_project() {
        let mut claude = chat(5, Provider::Claude);
        claude.cwd = Some(r"C:\Code\App\src-tauri".into());
        let mut root = chat(3, Provider::Codex);
        root.cwd = Some(r"C:\Code\App".into());
        let mut deeper = chat(4, Provider::Codex);
        deeper.cwd = Some(r"C:\Code\App\src-tauri\src".into());
        let mut other = chat(2, Provider::Codex);
        other.cwd = Some(r"C:\Code\Apple".into());
        let projects = group(vec![claude, root, deeper, other]);
        assert_eq!(projects.len(), 2, "C:\\Code\\Apple is not inside C:\\Code\\App");
        let app = projects.iter().find(|p| p.boss.is_some()).unwrap();
        assert_eq!(app.crew.len(), 2);
        assert!(projects.iter().any(|p| p.boss.is_none() && p.name == "Apple"));
    }

    #[test]
    fn groups_by_folder_ignoring_case_and_trailing_slash() {
        let mut claude = chat(1, Provider::Claude);
        claude.cwd = Some(r"C:\Code\App\".into());
        let mut codex = chat(3, Provider::Codex);
        let codex_cwd = if cfg!(windows) {
            r"c:\code\app"
        } else {
            r"C:\Code\App"
        };
        codex.cwd = Some(codex_cwd.into());
        let projects = group(vec![claude, codex]);
        assert_eq!(projects.len(), 1);
        let project = &projects[0];
        assert_eq!(project.id, codex_cwd);
        assert_eq!(project.name, "App");
        assert_eq!(project.boss.as_ref().unwrap().id, "1");
        assert_eq!(project.crew.len(), 1);
        assert_eq!(project.crew[0].kind, CrewKind::Codex);
        assert_eq!(project.crew[0].context_percent, Some(3.0));
    }

    #[test]
    fn boss_subagents_become_crew() {
        let mut boss = chat(2, Provider::Claude);
        boss.subagents = vec![subagent(3, false), subagent(4, true)];
        let projects = group(vec![boss]);
        let project = &projects[0];
        assert!(project.boss.as_ref().unwrap().subagents.is_empty());
        assert_eq!(project.crew.len(), 2);
        assert!(project
            .crew
            .iter()
            .all(|member| member.kind == CrewKind::ClaudeSub));
        assert_eq!(project.crew[0].name, "Agente 4");
        assert!(project.working);
        assert_eq!(project.last_activity, project.crew[0].last_activity);
    }

    #[test]
    fn codex_only_project_has_no_boss() {
        let mut codex = chat(1, Provider::Codex);
        codex.subagents.push(subagent(2, false));
        let projects = group(vec![codex, chat(3, Provider::Codex)]);
        assert_eq!(projects.len(), 1);
        let project = &projects[0];
        assert!(project.boss.is_none());
        assert_eq!(project.id, "sin-carpeta");
        assert_eq!(project.name, "Sin carpeta");
        assert!(project.cwd.is_none());
        let sub = &project.crew[1];
        assert_eq!(sub.kind, CrewKind::CodexSub);
        assert_eq!(sub.name, "Agente 2");
        assert_eq!(sub.context_percent, None);
    }

    #[test]
    fn crew_is_working_first_and_capped() {
        let mut all = vec![chat(11, Provider::Claude)];
        all.extend((1..=10).map(|i| {
            let mut chat = chat(i, Provider::Claude);
            chat.working = i <= 2;
            chat
        }));
        let projects = group(all);
        let project = &projects[0];
        assert_eq!(project.boss.as_ref().unwrap().id, "11");
        let ids: Vec<_> = project
            .crew
            .iter()
            .map(|member| member.id.as_str())
            .collect();
        assert_eq!(ids, ["2", "1", "10", "9", "8", "7", "6", "5"]);
        assert!(project
            .crew
            .iter()
            .all(|member| member.kind == CrewKind::ClaudeChat));
        assert_eq!(project.more_crew, 2);
    }

    #[test]
    fn keeps_four_most_recent_projects() {
        let mut all: Vec<_> = (1..=6)
            .map(|i| {
                let mut chat = chat(i, Provider::Claude);
                chat.cwd = Some(format!("/code/project-{i}"));
                chat
            })
            .collect();
        all[0].subagents.push(subagent(10, false));
        let projects = group(all);
        let names: Vec<_> = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        assert_eq!(names, ["project-1", "project-6", "project-5", "project-4"]);
    }

    fn subagent(second: u8, working: bool) -> Subagent {
        Subagent {
            id: second.to_string(),
            name: format!("Agente {second}"),
            kind: None,
            last_activity: chat(second, Provider::Claude).last_activity,
            working,
        }
    }
}
