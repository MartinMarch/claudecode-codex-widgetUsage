//! Finds running Claude Code and Codex CLI sessions.

use crate::model::{AgentProcess, Provider};
use std::collections::HashMap;
use std::path::Path;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub struct ProcessSource {
    sys: System,
}

fn base_name(s: &str) -> String {
    Path::new(s)
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default()
        .trim_end_matches(".exe")
        .to_string()
}

/// Decides whether a process is a CLI agent session.
/// Desktop apps (Claude.app, the Codex desktop app) are deliberately excluded.
fn classify(name: &str, exe: &str, cmd: &[String]) -> Option<Provider> {
    let exe_l = exe.to_lowercase();
    if exe_l.contains("claude.app")
        || exe_l.contains("anthropicclaude")
        // Windows Store builds of the desktop apps, and the Claude Code copy the
        // Claude desktop app bundles under `Claude-Data\anthropicpbc.claude_…`.
        || exe_l.contains("anthropicpbc.claude")
        || exe_l.contains("\\windowsapps\\")
        || exe_l.contains("codex.app")
        || exe_l.contains("agent-gauge")
        || exe_l.contains("agent gauge")
    {
        return None;
    }
    let mut candidates = vec![base_name(name), base_name(exe)];
    // Node-launched CLIs: `node /…/@anthropic-ai/claude-code/cli.js` or `node /…/bin/codex`.
    candidates.extend(cmd.iter().take(3).map(|a| base_name(a)));
    let joined = cmd.join(" ").to_lowercase();

    if candidates.iter().any(|c| c == "claude") || joined.contains("@anthropic-ai/claude-code") {
        return Some(Provider::Claude);
    }
    if candidates.iter().any(|c| c == "codex" || c.starts_with("codex-")) || joined.contains("@openai/codex") {
        return Some(Provider::Codex);
    }
    None
}

impl ProcessSource {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }

    pub fn read(&mut self) -> Vec<AgentProcess> {
        // CPU usage is a delta between two refreshes; the widget polls, so the
        // previous poll provides the baseline (the first reading shows 0%).
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_cwd(UpdateKind::OnlyIfNotSet),
        );
        let me = sysinfo::get_current_pid().ok();

        let mut matched: HashMap<Pid, Provider> = HashMap::new();
        for (pid, p) in self.sys.processes() {
            if Some(*pid) == me || p.thread_kind().is_some() {
                continue;
            }
            let name = p.name().to_string_lossy();
            let exe = p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            let cmd: Vec<String> = p.cmd().iter().map(|a| a.to_string_lossy().to_string()).collect();
            if let Some(provider) = classify(&name, &exe, &cmd) {
                matched.insert(*pid, provider);
            }
        }

        let mut out: Vec<AgentProcess> = matched
            .iter()
            // A wrapper (npm shim → native binary) shows up twice; keep the outermost.
            .filter(|(pid, provider)| {
                let parent = self.sys.process(**pid).and_then(|p| p.parent());
                parent.and_then(|pp| matched.get(&pp)) != Some(provider)
            })
            .filter_map(|(pid, provider)| {
                let p = self.sys.process(*pid)?;
                // Sum the subtree's CPU so a busy child shows on its session.
                let cpu: f32 = self
                    .sys
                    .processes()
                    .values()
                    .filter(|c| c.parent() == Some(*pid) || c.pid() == *pid)
                    .map(|c| c.cpu_usage())
                    .sum();
                Some(AgentProcess {
                    pid: pid.as_u32(),
                    provider: *provider,
                    cwd: p.cwd().map(|c| c.to_string_lossy().to_string()),
                    cpu,
                    memory_mb: p.memory() as f64 / 1_048_576.0,
                    started_at: p.start_time(),
                })
            })
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.started_at));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn detects_native_and_node_cli() {
        assert_eq!(classify("claude", "/usr/local/bin/claude", &s(&["claude"])), Some(Provider::Claude));
        assert_eq!(
            classify("node", "/usr/bin/node", &s(&["node", "/x/node_modules/@anthropic-ai/claude-code/cli.js"])),
            Some(Provider::Claude)
        );
        assert_eq!(classify("codex", "/opt/codex", &s(&["codex", "exec"])), Some(Provider::Codex));
        assert_eq!(classify("codex.exe", "C:\\bin\\codex.exe", &s(&[])), Some(Provider::Codex));
    }

    #[test]
    fn ignores_desktop_apps_and_unrelated() {
        assert_eq!(
            classify("Claude", "/Applications/Claude.app/Contents/MacOS/Claude", &s(&[])),
            None
        );
        assert_eq!(
            classify(
                "claude.exe",
                "C:\\Program Files\\WindowsApps\\AnthropicPBC.Claude_6.1.0.0_x64__abc\\app\\Claude.exe",
                &s(&[])
            ),
            None
        );
        assert_eq!(
            classify(
                "claude.exe",
                "C:\\Users\\me\\AppData\\Local\\Claude-Data\\anthropicpbc.claude_abc\\claude-code\\2.1.0\\x\\claude.exe",
                &s(&[])
            ),
            None
        );
        assert_eq!(classify("zsh", "/bin/zsh", &s(&["zsh", "-c", "echo claude"])), None);
    }
}
