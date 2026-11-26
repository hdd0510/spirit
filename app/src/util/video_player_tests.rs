use std::path::Path;

use super::*;

#[test]
fn only_quicktime_containers_play_natively() {
    for name in ["a.mp4", "a.MOV", "a.m4v"] {
        assert!(plays_natively(Path::new(name)), "{name}");
    }
    for name in ["a.mkv", "a.webm", "mp4"] {
        assert!(!plays_natively(Path::new(name)), "{name}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn unreadable_video_fails_without_leaving_files_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("broken.mkv");
    std::fs::write(&input, b"not a video").unwrap();
    assert_eq!(prepare_for_playback(&input), None);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
