use chrono::{DateTime, SecondsFormat, Utc};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

pub const ACTIVE_SECS: u64 = 600;
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

pub fn modified_secs_ago(path: &Path, now: SystemTime) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(now.duration_since(modified).unwrap_or_default().as_secs())
}

pub fn activity(path: &Path, now: SystemTime) -> Option<(String, bool)> {
    let age = modified_secs_ago(path, now)?;
    if age > ACTIVE_SECS {
        return None;
    }
    let modified = DateTime::<Utc>::from(fs::metadata(path).ok()?.modified().ok()?);
    Some((
        modified.to_rfc3339_opts(SecondsFormat::Nanos, true),
        age <= WORKING_SECS,
    ))
}

pub fn folder_name(cwd: &str) -> Option<&str> {
    cwd.rsplit(['/', '\\']).find(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
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
