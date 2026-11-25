# Agent Gauge v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the process list with live Claude/Codex chats (with running subagents and a context-saturation meter), let the widget refresh the Claude OAuth token itself, and restyle the widget as lighter frosted glass with clean corners.

**Architecture:** Two new Rust readers (`chats_claude.rs`, `chats_codex.rs`) scan the local transcript folders and return `Chat` values; `chats.rs` merges and trims them; `lib.rs` puts them in `Snapshot.chats` instead of `processes` (`procs.rs` and `sysinfo` go away). `claude.rs` gains a token refresh step before the usage call. The frontend renders the new list and the new glass.

**Tech Stack:** Tauri 2, Rust (serde_json, reqwest, chrono), TypeScript without framework, Vite.

**Spec:** `docs/superpowers/specs/2026-11-25-agent-gauge-v2-design.md`

## Global Constraints

- UI copy in Spanish, sentence case, errors say what to do.
- Never print, log or send the OAuth token anywhere except Anthropic (`platform.claude.com/v1/oauth/token`, `api.anthropic.com/api/oauth/usage`).
- Never read or show conversation text: from transcripts read only metadata (title, cwd, model, usage numbers, timestamps).
- Formats are internal and change: every field optional, a bad line is skipped, never panic.
- `src-tauri/src/model.rs` and `src/types.ts` change together.
- Thresholds: active = transcript modified ≤ 600 s ago; working = ≤ 20 s; max 5 chats, max 3 subagents per chat; red at ≥ 80 % (Claude) / ≥ 85 % (Codex).
- Done means: `npm run build`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (in `src-tauri/`) all pass.
- Git identity is not configured on this machine: commit with `git -c user.name="mmarc" -c user.email="mmarchrovira@gmail.com" commit …` and end messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- A transcript of hundreds of MB must not stall the poll: only the last 64 KB is read (test in Task 2).
- A transcript caught mid-write ends in a half line: that line is skipped, the chat still shows (test in Task 2).
- A brand-new chat with no assistant turn yet: shows with `contextPercent: null`, not dropped (test in Task 2).
- A Codex subagent whose parent session is not active: still listed, as its own row titled with its nickname (test in Task 3).
- Writing refreshed credentials must keep every other key (`mcpOAuth`, `scopes`, …) byte-for-byte equivalent in value (test in Task 5).

---

### Task 1: Shared types for chats

**Files:**
- Modify: `src-tauri/src/model.rs` (replace `AgentProcess`, change `Snapshot`)
- Modify: `src/types.ts` (mirror)
- Modify: `src/mock.ts` (sample chats)
- Modify: `src-tauri/src/lib.rs` (temporarily return `chats: vec![]`; keep building)

**Interfaces:**
- Produces (Rust, `#[serde(rename_all = "camelCase")]`, all `Clone, Debug, Serialize`):
  ```rust
  pub struct Subagent { pub id: String, pub name: String, pub kind: Option<String>,
                        pub last_activity: String /* RFC 3339 */, pub working: bool }
  pub struct Chat { pub id: String, pub provider: Provider, pub title: String,
                    pub cwd: Option<String>, pub last_activity: String, pub working: bool,
                    pub from_claude: bool, pub context_percent: Option<f64>,
                    pub context_approx: bool, pub subagents: Vec<Subagent>,
                    pub more_subagents: u32 }
  pub struct Snapshot { pub claude: ProviderUsage, pub codex: ProviderUsage,
                        pub chats: Vec<Chat>, pub taken_at: String }
  ```
- Produces (TS): `Subagent`, `Chat` with the same camelCase fields; `Snapshot.chats: Chat[]`; `AgentProcess` removed.

- [ ] **Step 1:** Edit `model.rs` and `types.ts` as above. In `lib.rs::snapshot`, set `chats: vec![]` and stop calling `procs` (leave `procs.rs` in place until Task 4).
- [ ] **Step 2:** Update `mock.ts`: default demo has 3 chats — Claude "Contexto de la herramienta" (cwd `…\claudecode-codex-widgetUsage`, working, 68 %, one subagent `{name: "Read graphify docs", kind: "web-fetch", working: true}`), Codex "graphify" (`fromClaude: true`, 91 %), Claude "landing" (not working, 23 %). `?demo=empty` has `chats: []`.
- [ ] **Step 3:** Make `src/main.ts` compile against the new type with the smallest change: render `s.chats.length` in the existing heading and drop the `proc()` rows (Task 6 redoes this section).
- [ ] **Step 4:** Run `npm run build` and `cargo clippy --all-targets -- -D warnings` in `src-tauri/`. Expected: both pass (clippy may flag `procs` as dead code: add `#[allow(dead_code)]` on the `mod procs;` line for now).
- [ ] **Step 5:** Commit `feat: chat types replace process list`.

### Task 2: Claude chats reader

**Files:**
- Create: `src-tauri/src/chats_claude.rs` (with `#[cfg(test)] mod tests`)
- Create: `src-tauri/src/tail.rs`
- Modify: `src-tauri/src/lib.rs` (`mod chats_claude; mod tail;`)

**Interfaces:**
- Consumes: `Chat`, `Subagent`, `Provider` (Task 1).
- Produces:
  - `tail::read_tail(path: &Path, max_bytes: u64) -> String` — last `max_bytes` of the file, lossy UTF-8, first (possibly partial) line dropped when the read did not start at byte 0.
  - `tail::modified_secs_ago(path: &Path, now: SystemTime) -> Option<u64>`
  - `chats_claude::read(root: &Path, now: SystemTime) -> Vec<Chat>` where `root` is the Claude config dir (`~/.claude`). Returns every active chat with all its running subagents; sorting and trimming happen in Task 4.
  - `chats_claude::context_window(model: &str) -> (u64, bool)` — `(tokens, approx)`.
  - Shared constants in `tail.rs`: `pub const ACTIVE_SECS: u64 = 600; pub const WORKING_SECS: u64 = 20; pub const TAIL_BYTES: u64 = 64 * 1024;`

Rules (from spec §2): chats are `root/projects/*/<id>.jsonl`; `id` = file stem. Title = `customTitle` of the last `{"type":"custom-title"}` line in the tail, else the last path component of `cwd`, else the project folder name. `cwd` = `cwd` field of the last line that has one. Context = last line with `type == "assistant"` and `message.usage`: `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`, divided by `context_window(message.model)`; if tokens exceed the window, use 1 000 000 and set `context_approx = true`. Subagents: `root/projects/*/<id>/subagents/agent-<aid>.jsonl` with sibling `agent-<aid>.meta.json`; running = `finished != true` and jsonl modified ≤ `ACTIVE_SECS`; `name` = meta `description` (fallback `agentType`, then `aid`), `kind` = meta `agentType`, `working` = jsonl modified ≤ `WORKING_SECS`. `context_window`: model starting with `claude-` → `(200_000, false)`; anything else → `(200_000, true)`.

- [ ] **Step 1: Write failing tests** in `chats_claude.rs` using a temp dir (`std::env::temp_dir().join(format!("ag-claude-{}-{}", std::process::id(), name))`) and `std::fs::File::set_modified` to age files:
  - `active_chat_with_title_cwd_and_context`: one jsonl with a `user` line (`cwd`), an `assistant` line `{"type":"assistant","cwd":"C:\\x\\proj","message":{"model":"claude-opus-5-5","usage":{"input_tokens":4,"cache_creation_input_tokens":96,"cache_read_input_tokens":99900,"output_tokens":10}}}`, then `{"type":"custom-title","customTitle":"Mi chat"}` → one chat, `title == "Mi chat"`, `cwd == Some("C:\\x\\proj")`, `context_percent == Some(50.0)`, `!context_approx`, `working == true`, `provider == Claude`.
  - `old_chat_is_not_listed`: same file aged 601 s → empty vec.
  - `new_chat_without_assistant_turn`: only a `user` line → listed, `context_percent == None`, title = `"proj"`.
  - `half_written_last_line_is_skipped`: valid assistant line followed by `{"type":"assist` (no newline) → listed with the valid line's context.
  - `over_window_switches_to_1m`: usage summing 400 000 → `context_percent == Some(40.0)`, `context_approx == true`.
  - `running_and_finished_subagents`: two subagents, meta `{"agentType":"Explore","description":"Buscar tests","finished":false}` and `{"agentType":"web-fetch","finished":true}` → exactly one `Subagent { name: "Buscar tests", kind: Some("Explore"), .. }`.
  - `read_tail_only_reads_the_end` (in `tail.rs`): 1 MB file of `x\n` lines ending in `LAST\n`, `read_tail(p, 64)` has length ≤ 64 and ends with `"LAST\n"` and does not start mid-line.
- [ ] **Step 2:** Run `cargo test chats_claude tail`. Expected: FAIL (functions missing).
- [ ] **Step 3:** Implement `tail.rs` and `chats_claude.rs` per the rules above. Parse lines with `serde_json::from_str::<Value>` and `continue` on error. Never keep message text.
- [ ] **Step 4:** Run `cargo test chats_claude tail`. Expected: all PASS. Then `cargo clippy --all-targets -- -D warnings`.
- [ ] **Step 5:** Commit `feat: read active Claude chats, subagents and context from transcripts`.

### Task 3: Codex chats reader

**Files:**
- Create: `src-tauri/src/chats_codex.rs` (with tests)
- Modify: `src-tauri/src/lib.rs` (`mod chats_codex;`)

**Interfaces:**
- Consumes: `tail::{read_tail, modified_secs_ago, ACTIVE_SECS, WORKING_SECS, TAIL_BYTES}` (Task 2), `Chat`, `Subagent` (Task 1), `codex::home()` (existing).
- Produces: `chats_codex::read(home: &Path, now: SystemTime) -> Vec<Chat>`.

Rules (spec §2): scan `home/sessions/YYYY/MM/DD/*.jsonl` of the 2 newest day folders (ignore `.zst`); keep files modified ≤ `ACTIVE_SECS`. First line is `session_meta`: `payload.id` (session id), `payload.cwd`, `payload.originator`, and for subagents `payload.source.subagent.thread_spawn.{parent_thread_id, agent_nickname}`. Read the first line with a `BufReader` (one line only) and the tail with `read_tail`. Context = last `event_msg` with `payload.type == "token_count"`: `payload.info.last_token_usage.input_tokens / payload.info.model_context_window * 100`. Title = `thread_name` from `home/session_index.jsonl` (map `id → thread_name`, last wins) keyed by `payload.id`; if absent, the last path component of `cwd`. `from_claude = originator == "Claude Code"`. Subagent sessions attach to the chat whose `id == parent_thread_id` if that chat is active; otherwise they become their own `Chat` with `title = agent_nickname` (fallback "Subagente"). `Subagent.name = agent_nickname`, `kind = None`, `working` from its own mtime.

- [ ] **Step 1: Write failing tests** (temp `CODEX_HOME`-style dir passed as `home`):
  - `chat_from_claude_with_context`: meta line `{"type":"session_meta","payload":{"id":"P1","cwd":"C:\\Users\\me\\Code","originator":"Claude Code"}}` + `{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":237500},"model_context_window":475000}}}` + index line `{"id":"P1","thread_name":"graphify"}` → one chat, `title == "graphify"`, `from_claude`, `context_percent == Some(50.0)`, provider Codex.
  - `subagent_attaches_to_active_parent`: P1 as above plus session S1 with `payload.source.subagent.thread_spawn = {"parent_thread_id":"P1","agent_nickname":"Pauli"}` → one chat with `subagents[0].name == "Pauli"`.
  - `orphan_subagent_is_its_own_row`: only S1 active (P1 file aged 601 s) → one chat titled `"Pauli"`.
  - `compressed_and_old_sessions_ignored`: a `.jsonl.zst` file and an aged `.jsonl` → empty vec.
- [ ] **Step 2:** `cargo test chats_codex` → FAIL.
- [ ] **Step 3:** Implement `chats_codex::read`.
- [ ] **Step 4:** `cargo test chats_codex` → PASS; clippy clean.
- [ ] **Step 5:** Commit `feat: read active Codex chats and subagents from session logs`.

### Task 4: Merge, trim and wire chats into the snapshot

**Files:**
- Create: `src-tauri/src/chats.rs` (with tests)
- Modify: `src-tauri/src/lib.rs` (`Sources` drops `procs`; `snapshot` fills `chats`)
- Delete: `src-tauri/src/procs.rs`; remove `sysinfo` from `Cargo.toml`

**Interfaces:**
- Consumes: `chats_claude::read`, `chats_codex::read`, `claude::config_dir()`, `codex::home()`.
- Produces:
  - `chats::select(all: Vec<Chat>) -> Vec<Chat>` — sort by `last_activity` desc; take 5; if both providers have chats and one provider is missing from the top 5, replace the 5th with that provider's most recent chat; for each chat sort subagents by `last_activity` desc, keep 3, set `more_subagents` to the rest.
  - `chats::read_all(now: SystemTime) -> Vec<Chat>` — both readers (missing dirs → empty) then `select`.

- [ ] **Step 1: Write failing tests**: `keeps_five_most_recent`, `guarantees_one_codex_chat` (6 Claude chats newer than 1 Codex → result has 5 with the Codex one in it), `trims_subagents_to_three` (5 subagents → 3 kept, `more_subagents == 2`).
- [ ] **Step 2:** `cargo test chats::` → FAIL.
- [ ] **Step 3:** Implement `chats.rs`; in `lib.rs::snapshot` run `chats::read_all` inside `tauri::async_runtime::spawn_blocking` next to `codex::read`. Delete `procs.rs`, its `mod`, and the `sysinfo` dependency.
- [ ] **Step 4:** `cargo test` (all) → PASS; clippy clean; `npm run build` passes.
- [ ] **Step 5:** Commit `feat: show active chats instead of processes`.

### Task 5: Claude token refresh

**Files:**
- Modify: `src-tauri/src/claude.rs`
- Modify: `AGENTS.md` (rule "No guardes, refresques ni reescribas el token" → new rule from spec §3), `README.md` (data-sources table and the "nunca renueva" sentence)

**Interfaces:**
- Produces (private to `claude.rs`, tested in its `mod tests`):
  - `const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";`
  - `const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";`
  - `#[derive(Deserialize)] struct RefreshResponse { access_token: String, refresh_token: Option<String>, expires_in: i64, refresh_token_expires_in: Option<i64> }`
  - `fn needs_refresh(expires_at_ms: Option<i64>, now_ms: i64) -> bool` — true when `expires_at_ms < now_ms + 300_000`; `None` → false.
  - `fn apply_refresh(creds: &mut serde_json::Value, r: &RefreshResponse, now_ms: i64)` — sets `claudeAiOauth.accessToken`, `.refreshToken` (if `Some`), `.expiresAt = now_ms + expires_in*1000`, `.refreshTokenExpiresAt = now_ms + refresh_token_expires_in*1000` (if `Some`); touches nothing else.
  - `fn write_atomic(path: &Path, value: &serde_json::Value) -> std::io::Result<()>` — write `path.with_extension("json.tmp-agent-gauge")`, then `fs::rename` over `path`.
  - `ClaudeSource` gains `last_refresh_attempt: Option<Instant>`; refresh at most once per 300 s.

Flow in `fetch_remote` (non-macOS only, `#[cfg(not(target_os = "macos"))]`): read the credentials file as `Value` (re-read every call); if `needs_refresh` and the 300 s guard allows → POST JSON `{grant_type:"refresh_token", refresh_token, client_id: CLIENT_ID, scope: scopes.join(" ")}` → on 200 `apply_refresh` + `write_atomic`, then call usage with the new token. If the usage call returns 401 with a token that was not expired, try the same refresh once. Refresh failure message: `"Tu sesión de Claude caducó del todo. Abre Claude Code y vuelve a iniciar sesión."`. macOS keeps today's read-only behaviour and message.

- [ ] **Step 1: Write failing tests**: `needs_refresh_window` (expired → true; expires in 4 min → true; in 10 min → false; `None` → false); `apply_refresh_keeps_other_keys` (input `{"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1,"scopes":["user:inference"],"subscriptionType":"pro"},"mcpOAuth":{"x":{"k":1}}}`, response `{access_token:"A2", refresh_token:Some("R2"), expires_in:3600, refresh_token_expires_in:Some(86400)}`, `now_ms = 1_000` → tokens become `A2`/`R2`, `expiresAt == 3_601_000`, `refreshTokenExpiresAt == 86_401_000`, `scopes`, `subscriptionType` and `mcpOAuth` unchanged); `write_atomic_replaces_file` (temp file, write, read back equals value, no `.tmp-agent-gauge` left).
- [ ] **Step 2:** `cargo test claude::` → FAIL.
- [ ] **Step 3:** Implement the functions and the flow. Never format the token into any error or log string.
- [ ] **Step 4:** `cargo test` → PASS; clippy clean.
- [ ] **Step 5:** Update `AGENTS.md` and `README.md` text. Commit `feat: refresh the Claude OAuth token when it expires`.

### Task 6: New glass, corners and chat list (Claude does this one — visual work)

**Files:**
- Modify: `src/styles.css`, `src/main.ts`, `src/gauge.ts`, `src/format.ts`
- Modify: `src-tauri/src/lib.rs` (Windows corner preference), `src-tauri/Cargo.toml` (`[target.'cfg(windows)'.dependencies] windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_Graphics_Dwm"] }`)

**Interfaces:**
- Consumes: `Chat`, `Subagent` (Task 1), `platformInfo.os` (existing `body[data-os]`).
- Produces: `format.ts` → `contextLevel(c: Chat): "ok" | "hot"` (hot at ≥ 80 Claude / ≥ 85 Codex).

- [ ] **Step 1:** CSS tokens: `--glass-native: rgba(18, 22, 36, 0.22)`; `--radius: 22px` with `body[data-os="windows"] { --radius: 8px; }`; ink gets `text-shadow: 0 1px 2px rgba(0, 0, 0, 0.35)`; rim and grain softer. Match the approved mockup `.superpowers/brainstorm/*/content/estilo-cristal.html`, card A.
- [ ] **Step 2:** `gauge.ts`: thinner arc (stroke ≈ 5 px in a 140 viewBox), ticks only at 0/50/100.
- [ ] **Step 3:** `main.ts`: section "Chats activos" with one row per chat (dot in provider colour, pulsing when `working`; title; cwd folder in faint text; "· desde Claude" when `fromClaude`; context bar + "%" with "≈" prefix when `contextApprox`, red when `contextLevel == "hot"`, "–" when null), subagent rows indented with "↳", "+N más" line when `moreSubagents > 0`, empty state "Ningún chat activo ahora mismo." Remove the `.proc*` CSS.
- [ ] **Step 4:** `lib.rs::apply_glass` on Windows also calls `DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND, 4)` (hwnd from `window.hwnd()`).
- [ ] **Step 5:** Verify: `npm run build`, clippy, tests; then in `npm run app:dev` capture the window (scratchpad `shot.ps1`) over a light and a dark background: no square corner, background visible through, text readable, a live chat of this session listed with its context %.
- [ ] **Step 6:** Commit `feat: lighter glass, clean corners and active chats list`.
