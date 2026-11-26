use super::*;

#[test]
fn ranges_from_the_player_map_onto_the_file() {
    assert_eq!(parse_range("bytes=0-1"), Some((0, Some(1))));
    assert_eq!(parse_range("bytes=1024-"), Some((1024, None)));
    assert_eq!(parse_range("items=0-1"), None);

    assert_eq!(response_range(Some((0, Some(1))), 100), Some((0, 1)));
    assert_eq!(response_range(Some((10, None)), 100), Some((10, 99)));
    assert_eq!(response_range(Some((10, Some(500))), 100), Some((10, 99)));
    assert_eq!(response_range(None, 100), Some((0, 99)));
    assert_eq!(response_range(Some((100, None)), 100), None);
    assert_eq!(response_range(None, 0), None);
}

#[test]
fn stream_urls_keep_the_extension_and_escape_the_name() {
    let url = stream_url(
        vec![PathBuf::from("/tmp/socket")],
        "/home/user/Sáng tạo/clip 1.mp4",
    )
    .expect("server starts");
    let url = url.to_string_lossy().into_owned();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert!(url.ends_with("/clip%201.mp4"), "{url}");
    assert!(crate::util::openable_file_type::is_video_file(
        std::path::Path::new(&url)
    ));
}

#[test]
#[ignore = "needs SPIRIT_TEST_SSH_SOCKET and SPIRIT_TEST_REMOTE_VIDEO pointing at a live SSH master"]
fn streams_byte_ranges_of_a_real_remote_file() {
    let socket = PathBuf::from(std::env::var("SPIRIT_TEST_SSH_SOCKET").unwrap());
    let remote = std::env::var("SPIRIT_TEST_REMOTE_VIDEO").unwrap();
    let url = stream_url(vec![socket.clone()], &remote).unwrap();
    let url = url.to_string_lossy().into_owned();

    let fetch = |range: &str| {
        Command::new("curl")
            .args(["-s", "-D", "-", "-r", range, &url])
            .output()
            .unwrap()
            .stdout
    };
    let expected = |start: u64, length: u64| {
        ssh_command(
            &socket,
            &format!(
                "tail -c +{} {} | head -c {length}",
                start + 1,
                shell_quote(&remote)
            ),
        )
        .stdout(Stdio::piped())
        .output()
        .unwrap()
        .stdout
    };
    let body = |response: Vec<u8>| {
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (
            String::from_utf8_lossy(&response[..split]).into_owned(),
            response[split + 4..].to_vec(),
        )
    };

    let (headers, bytes) = body(fetch("0-1"));
    assert!(headers.starts_with("HTTP/1.1 206"), "{headers}");
    assert!(headers.contains("Content-Range: bytes 0-1/"), "{headers}");
    assert_eq!(bytes, expected(0, 2));

    let (_, bytes) = body(fetch("100000-165535"));
    assert_eq!(bytes.len(), 65536);
    assert_eq!(bytes, expected(100000, 65536));
}

#[test]
#[ignore = "manual: serves a live remote video for 40s so a player can be pointed at it"]
fn serve_a_real_remote_file_for_a_player() {
    let socket = PathBuf::from(std::env::var("SPIRIT_TEST_SSH_SOCKET").unwrap());
    let remote = std::env::var("SPIRIT_TEST_REMOTE_VIDEO").unwrap();
    let url = stream_url(vec![socket], &remote).unwrap();
    std::fs::write(
        std::env::var("SPIRIT_TEST_URL_FILE").unwrap(),
        url.to_string_lossy().as_bytes(),
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(40));
}
