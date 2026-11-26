use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
const PLAYER_APPS: &[&str] = &["IINA", "VLC", "mpv", "Infuse"];

fn run_quiet(command: &mut Command) -> bool {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn installed_player() -> Option<&'static str> {
    PLAYER_APPS.iter().copied().find(|name| {
        ["/Applications", "/System/Applications"]
            .iter()
            .any(|dir| Path::new(dir).join(format!("{name}.app")).exists())
            || dirs::home_dir().is_some_and(|home| {
                home.join("Applications")
                    .join(format!("{name}.app"))
                    .exists()
            })
    })
}

#[cfg(target_os = "macos")]
fn find_ffmpeg() -> Option<PathBuf> {
    [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

#[cfg(target_os = "macos")]
fn converted_path(input: &Path) -> Option<PathBuf> {
    use std::hash::{DefaultHasher, Hash as _, Hasher as _};

    let metadata = std::fs::metadata(input).ok()?;
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata.modified().ok()?.hash(&mut hasher);
    let dir = dirs::cache_dir()?.join("spirit-video");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{:016x}.mp4", hasher.finish())))
}

pub fn plays_natively(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["mp4", "m4v", "mov"].contains(&extension.to_ascii_lowercase().as_str())
        })
}

#[cfg(target_os = "macos")]
pub fn prepare_for_playback(path: &Path) -> Option<PathBuf> {
    if plays_natively(path) {
        return Some(path.to_owned());
    }
    convert_to_mp4(path)
}

#[cfg(not(target_os = "macos"))]
pub fn prepare_for_playback(_path: &Path) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
fn convert_to_mp4(input: &Path) -> Option<PathBuf> {
    let ffmpeg = find_ffmpeg()?;
    let output = converted_path(input)?;
    if output.is_file() {
        return Some(output);
    }
    let partial = output.with_extension("partial.mp4");
    if run_quiet(
        Command::new(&ffmpeg)
            .args(["-y", "-i"])
            .arg(input)
            .args(["-c", "copy", "-movflags", "+faststart"])
            .arg(&partial),
    ) {
        return std::fs::rename(&partial, &output).ok().map(|()| output);
    }
    if run_quiet(
        Command::new(&ffmpeg)
            .args(["-y", "-i"])
            .arg(input)
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "23",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
            ])
            .arg(&partial),
    ) {
        return std::fs::rename(&partial, &output).ok().map(|()| output);
    }
    let _ = std::fs::remove_file(&partial);
    None
}

#[cfg(target_os = "macos")]
pub fn open_video(path: PathBuf) {
    std::thread::spawn(move || {
        if let Some(player) = installed_player() {
            if run_quiet(Command::new("open").arg("-a").arg(player).arg(&path)) {
                return;
            }
        }
        let target = prepare_for_playback(&path).unwrap_or(path);
        run_quiet(Command::new("open").arg(&target));
    });
}

#[cfg(not(target_os = "macos"))]
pub fn open_video(path: PathBuf) {
    std::thread::spawn(move || {
        for player in ["mpv", "vlc"] {
            if run_quiet(Command::new(player).arg(&path)) {
                return;
            }
        }
        run_quiet(Command::new("xdg-open").arg(&path));
    });
}

#[cfg(test)]
#[path = "video_player_tests.rs"]
mod tests;
