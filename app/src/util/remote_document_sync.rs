use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime};

const POLL_INTERVAL: Duration = Duration::from_secs(1);

static WATCHED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);

pub fn upload_on_save(local_path: PathBuf, control_sockets: Vec<PathBuf>, remote_path: String) {
    let is_new = WATCHED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(local_path.clone());
    if !is_new {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("remote-document-sync".into())
        .spawn(move || watch(&local_path, &control_sockets, &remote_path));
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

fn watch(local_path: &Path, control_sockets: &[PathBuf], remote_path: &str) {
    let mut uploaded = modified(local_path);
    loop {
        std::thread::sleep(POLL_INTERVAL);
        let Some(current) = modified(local_path) else {
            continue;
        };
        if Some(current) == uploaded {
            continue;
        }
        std::thread::sleep(POLL_INTERVAL);
        if modified(local_path) != Some(current) {
            continue;
        }
        if upload(local_path, control_sockets, remote_path) {
            log::info!("Uploaded edited document back to the remote host");
            uploaded = Some(current);
        } else {
            log::warn!("Uploading edited document back to the remote host failed");
        }
    }
}

fn upload(local_path: &Path, control_sockets: &[PathBuf], remote_path: &str) -> bool {
    control_sockets.iter().any(|socket| {
        Command::new("scp")
            .arg("-o")
            .arg(format!("ControlPath={}", socket.display()))
            .args(["-o", "ControlMaster=no", "-o", "BatchMode=yes", "-q"])
            .arg(local_path)
            .arg(format!("placeholder@placeholder:{remote_path}"))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}
