use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;

const MAX_ENTRIES: usize = 80;
static LOG: Mutex<ActivityLog> = Mutex::new(ActivityLog {
    entries: VecDeque::new(),
});

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub at: String,
    pub source: String,
    pub level: LogLevel,
    pub message: String,
    pub count: u32,
}

#[derive(Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Ok,
    Warn,
    Error,
}

#[derive(Default)]
struct ActivityLog {
    entries: VecDeque<LogEntry>,
}

impl ActivityLog {
    fn record(&mut self, source: &str, level: LogLevel, message: String) {
        let at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let previous = self.entries.iter().position(|entry| entry.source == source);
        if let Some(index) = previous
            .filter(|&i| self.entries[i].level == level && self.entries[i].message == message)
        {
            if let Some(mut entry) = self.entries.remove(index) {
                entry.at = at;
                entry.count = entry.count.saturating_add(1);
                self.entries.push_front(entry);
                return;
            }
        }
        self.entries.push_front(LogEntry {
            at,
            source: source.into(),
            level,
            message,
            count: 1,
        });
        self.entries.truncate(MAX_ENTRIES);
    }

    fn entries(&self) -> Vec<LogEntry> {
        self.entries.iter().cloned().collect()
    }
}

pub fn record(source: &str, level: LogLevel, message: impl Into<String>) {
    LOG.lock()
        .unwrap_or_else(|err| err.into_inner())
        .record(source, level, message.into());
}

pub fn entries() -> Vec<LogEntry> {
    LOG.lock().unwrap_or_else(|err| err.into_inner()).entries()
}

pub fn record_usage(source: &str, primary: Option<f64>, secondary: Option<f64>) {
    let percent = |value: Option<f64>| value.map_or_else(|| "—".into(), |p| format!("{p:.0} %"));
    record(
        source,
        LogLevel::Ok,
        format!(
            "Uso leído: {} (5 h), {} (semana)",
            percent(primary),
            percent(secondary)
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::{ActivityLog, LogLevel};

    #[test]
    fn deduplicates_the_latest_entry_for_each_source() {
        let mut log = ActivityLog::default();
        log.record("Claude", LogLevel::Ok, "Uso leído".into());
        log.record("Codex", LogLevel::Ok, "Uso leído".into());
        log.record("Claude", LogLevel::Ok, "Uso leído".into());
        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source, "Claude");
        assert_eq!(entries[0].count, 2);
        assert_eq!(entries[1].count, 1);

        log.record("Claude", LogLevel::Ok, "Login renovado".into());
        log.record("Claude", LogLevel::Warn, "Login renovado".into());
        let entries = log.entries();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].message, "Login renovado");
        assert!(entries[0].level == LogLevel::Warn);
        assert_eq!(entries[0].count, 1);
    }

    #[test]
    fn keeps_only_the_latest_eighty_entries() {
        let mut log = ActivityLog::default();
        for i in 0..85 {
            log.record("App", LogLevel::Ok, i.to_string());
        }
        let entries = log.entries();
        assert_eq!(entries.len(), 80);
        assert_eq!(entries[0].message, "84");
        assert_eq!(entries[79].message, "5");
    }
}
