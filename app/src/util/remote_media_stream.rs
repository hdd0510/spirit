use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex};

const COPY_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Clone)]
struct RemoteMedia {
    control_sockets: Vec<PathBuf>,
    remote_path: String,
    size: Option<u64>,
}

struct Server {
    port: u16,
    media: Arc<Mutex<HashMap<String, RemoteMedia>>>,
}

static SERVER: LazyLock<Option<Server>> = LazyLock::new(start_server);

fn start_server() -> Option<Server> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let media: Arc<Mutex<HashMap<String, RemoteMedia>>> = Arc::default();
    let shared = media.clone();
    std::thread::Builder::new()
        .name("remote-media-stream".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let media = shared.clone();
                let _ = std::thread::Builder::new()
                    .name("remote-media-request".into())
                    .spawn(move || {
                        if let Err(err) = serve(stream, &media) {
                            log::debug!("Remote media request ended: {err}");
                        }
                    });
            }
        })
        .ok()?;
    Some(Server { port, media })
}

pub fn stream_url(control_sockets: Vec<PathBuf>, remote_path: &str) -> Option<PathBuf> {
    let server = SERVER.as_ref()?;
    let token = media_token(&control_sockets, remote_path);
    let file_name = remote_path.rsplit('/').next().unwrap_or("video");
    server
        .media
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(token.clone())
        .or_insert_with(|| RemoteMedia {
            control_sockets,
            remote_path: remote_path.to_owned(),
            size: None,
        });
    Some(PathBuf::from(format!(
        "http://127.0.0.1:{}/{token}/{}",
        server.port,
        percent_encode(file_name)
    )))
}

fn media_token(control_sockets: &[PathBuf], remote_path: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    control_sockets.hash(&mut hasher);
    remote_path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

struct Request {
    is_head: bool,
    token: String,
    range: Option<(u64, Option<u64>)>,
}

fn read_request(stream: &TcpStream) -> std::io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let token = target
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("range")
        {
            range = parse_range(value.trim());
        }
    }
    Ok(Request {
        is_head: method.eq_ignore_ascii_case("HEAD"),
        token,
        range,
    })
}

pub(crate) fn parse_range(value: &str) -> Option<(u64, Option<u64>)> {
    let spec = value.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start, end) = spec.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = match end.trim() {
        "" => None,
        end => Some(end.parse().ok()?),
    };
    Some((start, end))
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn ssh_command(control_socket: &PathBuf, remote_command: &str) -> Command {
    let mut command = Command::new("ssh");
    command
        .arg("-o")
        .arg(format!("ControlPath={}", control_socket.display()))
        .args(["-o", "ControlMaster=no", "-o", "BatchMode=yes", "-T"])
        .arg("placeholder@placeholder")
        .arg(remote_command)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn remote_size(media: &RemoteMedia) -> Option<u64> {
    let remote_command = format!("wc -c < {}", shell_quote(&media.remote_path));
    media.control_sockets.iter().find_map(|socket| {
        let output = ssh_command(socket, &remote_command).output().ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    })
}

fn spawn_range_reader(media: &RemoteMedia, start: u64, length: u64) -> Option<Child> {
    let remote_command = format!(
        "tail -c +{} {} | head -c {length}",
        start + 1,
        shell_quote(&media.remote_path)
    );
    media.control_sockets.iter().find_map(|socket| {
        ssh_command(socket, &remote_command)
            .stdout(Stdio::piped())
            .spawn()
            .ok()
    })
}

fn content_type(remote_path: &str) -> &'static str {
    let extension = remote_path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "mov" => "video/quicktime",
        "m4v" => "video/x-m4v",
        _ => "video/mp4",
    }
}

fn media_with_size(
    media: &Mutex<HashMap<String, RemoteMedia>>,
    token: &str,
) -> Option<(RemoteMedia, u64)> {
    let entry = media
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(token)
        .cloned()?;
    if let Some(size) = entry.size {
        return Some((entry, size));
    }
    let size = remote_size(&entry)?;
    if let Some(stored) = media
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_mut(token)
    {
        stored.size = Some(size);
    }
    Some((entry, size))
}

fn serve(
    mut stream: TcpStream,
    media: &Mutex<HashMap<String, RemoteMedia>>,
) -> std::io::Result<()> {
    let request = read_request(&stream)?;
    let Some((entry, size)) = media_with_size(media, &request.token) else {
        return stream.write_all(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    };
    let Some((start, end)) = response_range(request.range, size) else {
        return stream.write_all(
            format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{size}\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        );
    };
    let length = end + 1 - start;
    let status = if request.range.is_some() {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    stream.write_all(
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: {}\r\nAccept-Ranges: bytes\r\n\
             Content-Length: {length}\r\nContent-Range: bytes {start}-{end}/{size}\r\n\
             Connection: close\r\n\r\n",
            content_type(&entry.remote_path)
        )
        .as_bytes(),
    )?;
    if request.is_head || length == 0 {
        return Ok(());
    }

    let Some(mut child) = spawn_range_reader(&entry, start, length) else {
        return Ok(());
    };
    let result = copy_to_client(&mut child, &mut stream);
    let _ = child.kill();
    let _ = child.wait();
    result
}

pub(crate) fn response_range(range: Option<(u64, Option<u64>)>, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    let (start, end) = range.unwrap_or((0, None));
    let end = end.map_or(size - 1, |end| end.min(size - 1));
    (start <= end).then_some((start, end))
}

fn copy_to_client(child: &mut Child, stream: &mut TcpStream) -> std::io::Result<()> {
    let Some(stdout) = child.stdout.as_mut() else {
        return Ok(());
    };
    let mut buffer = vec![0; COPY_CHUNK_BYTES];
    loop {
        let read = stdout.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        stream.write_all(&buffer[..read])?;
    }
}

#[cfg(test)]
#[path = "remote_media_stream_tests.rs"]
mod tests;
