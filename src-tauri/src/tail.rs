use chrono::{DateTime, SecondsFormat, Utc};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

pub const AWAKE_SECS: u64 = 600;
pub const LIST_SECS: u64 = 86_400;
pub const WORKING_SECS: u64 = 20;
pub const TAIL_BYTES: u64 = 64 * 1024;

pub fn read_tail(path: &Path, max_bytes: u64) -> String {
    fn bytes(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        let start = file.metadata()?.len().saturating_sub(max_bytes);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(max_bytes).read_to_end(&mut bytes)?;
        if start > 0 {
            let skip = bytes
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |i| i + 1);
            bytes.drain(..skip);
        }
        Ok(bytes)
    }
    String::from_utf8_lossy(&bytes(path, max_bytes).unwrap_or_default()).into_owned()
}

/// First `max_bytes` of the file, lossy UTF-8, with a trailing partial line dropped.
pub fn read_head(path: &Path, max_bytes: u64) -> String {
    let mut bytes = Vec::new();
    if let Ok(file) = File::open(path) {
        let _ = file.take(max_bytes).read_to_end(&mut bytes);
    }
    if bytes.len() as u64 == max_bytes {
        let keep = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        bytes.truncate(keep);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn activity(path: &Path, now: SystemTime) -> Option<(String, bool, bool)> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let age = now.duration_since(modified).unwrap_or_default().as_secs();
    if age > LIST_SECS {
        return None;
    }
    let modified = DateTime::<Utc>::from(modified);
    Some((
        modified.to_rfc3339_opts(SecondsFormat::Nanos, true),
        age <= WORKING_SECS,
        age > AWAKE_SECS,
    ))
}

pub fn folder_name(cwd: &str) -> Option<&str> {
    cwd.rsplit(['/', '\\']).find(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
    #[test]
    fn activity_distinguishes_working_awake_asleep_and_expired() {
        use std::time::{Duration, SystemTime};

        let dir = std::env::temp_dir().join(format!("ag-activity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chat.jsonl");
        let file = std::fs::File::create(&path).unwrap();
        let now = SystemTime::now();
        for (age, expected) in [
            (10, Some((true, false))),
            (300, Some((false, false))),
            (7200, Some((false, true))),
            (90_000, None),
        ] {
            file.set_modified(now - Duration::from_secs(age)).unwrap();
            assert_eq!(
                super::activity(&path, now).map(|(_, working, asleep)| (working, asleep)),
                expected
            );
        }
        drop(file);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn read_tail_only_reads_the_end() {
        let path = std::env::temp_dir().join(format!("ag-tail-{}", std::process::id()));
        std::fs::write(&path, "x\n".repeat(512 * 1024) + "LAST\n").unwrap();
        let tail = super::read_tail(&path, 64);
        std::fs::remove_file(path).unwrap();
        assert!(tail.len() <= 64);
        assert!(tail.ends_with("LAST\n"));
        assert!(tail.starts_with("x\n"));
    }
}
