use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const EXPIRY: Duration = Duration::from_secs(5 * 60);
const HEARTBEAT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResumeHint {
    pub ssh_command: Option<String>,
    pub agent_command: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct StoredFile {
    saved_at: u64,
    panes: HashMap<String, ResumeHint>,
}

struct Store {
    restorable: HashMap<String, ResumeHint>,
    current: Mutex<HashMap<String, ResumeHint>>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_path() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".warp-oss")
            .join("session-resume.json"),
    )
}

fn read_fresh() -> HashMap<String, ResumeHint> {
    let Some(path) = file_path() else {
        return HashMap::new();
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let _ = std::fs::remove_file(&path);
    let Ok(stored) = serde_json::from_str::<StoredFile>(&contents) else {
        return HashMap::new();
    };
    if now_secs().saturating_sub(stored.saved_at) > EXPIRY.as_secs() {
        return HashMap::new();
    }
    stored.panes
}

fn write_current(current: &HashMap<String, ResumeHint>) {
    let Some(path) = file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stored = StoredFile {
        saved_at: now_secs(),
        panes: current.clone(),
    };
    if let Ok(json) = serde_json::to_string(&stored) {
        let _ = std::fs::write(&path, json);
    }
}

fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| {
        let restorable = read_fresh();
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(HEARTBEAT);
                let guard = store().current.lock().unwrap_or_else(|e| e.into_inner());
                write_current(&guard);
            }
        });
        Store {
            restorable,
            current: Mutex::new(HashMap::new()),
        }
    })
}

fn key(uuid: &[u8]) -> String {
    hex::encode(uuid)
}

pub fn record(uuid: &[u8], hint: Option<ResumeHint>) {
    let store = store();
    let mut current = store.current.lock().unwrap_or_else(|e| e.into_inner());
    let key = key(uuid);
    let changed = match hint {
        Some(hint) => current.insert(key, hint.clone()) != Some(hint),
        None => current.remove(&key).is_some(),
    };
    if changed {
        write_current(&current);
    }
}

pub fn take_restorable(uuid: &[u8]) -> Option<ResumeHint> {
    store().restorable.get(&key(uuid)).cloned()
}
