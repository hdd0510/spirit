use std::path::{Path, PathBuf};

use remote_server::setup::{
    CHECK_TIMEOUT, RemotePlatform, remote_server_binary, remote_server_dir,
};
use remote_server::ssh::{run_ssh_command, scp_upload};
use remote_server::transport::{Error, InstallOutcome, InstallSource};

const UPLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1200);

fn local_binary_path(platform: &RemotePlatform) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".warp-oss")
            .join("remote-binaries")
            .join(format!(
                "{}-{}",
                platform.os.as_str(),
                platform.arch.as_str()
            ))
            .join("warp-oss"),
    )
}

fn relative_to_home(path: &str) -> &str {
    path.strip_prefix("~/").unwrap_or(path)
}

fn compress_binary(binary: &Path) -> Result<PathBuf, Error> {
    let compressed = binary.with_extension("gz");
    let is_fresh = match (std::fs::metadata(binary), std::fs::metadata(&compressed)) {
        (Ok(source), Ok(target)) => match (source.modified(), target.modified()) {
            (Ok(source_time), Ok(target_time)) => target_time >= source_time,
            _ => false,
        },
        _ => false,
    };
    if is_fresh {
        return Ok(compressed);
    }
    let partial = binary.with_extension("gz.partial");
    let output = std::fs::File::create(&partial).map_err(|err| Error::Other(err.into()))?;
    let status = std::process::Command::new("gzip")
        .arg("-c")
        .arg(binary)
        .stdout(output)
        .status()
        .map_err(|err| Error::Other(err.into()))?;
    if !status.success() {
        return Err(Error::Other(anyhow::anyhow!(
            "gzip failed for {}",
            binary.display()
        )));
    }
    std::fs::rename(&partial, &compressed).map_err(|err| Error::Other(err.into()))?;
    Ok(compressed)
}

async fn upload_binary(socket_path: &Path) -> Result<(), Error> {
    let platform = super::detect_remote_platform(socket_path).await?;
    let local = local_binary_path(&platform)
        .filter(|path| path.is_file())
        .ok_or_else(|| {
            Error::Other(anyhow::anyhow!(
                "No local remote-server binary for {}-{}. Put one at \
                 ~/.warp-oss/remote-binaries/{}-{}/warp-oss",
                platform.os.as_str(),
                platform.arch.as_str(),
                platform.os.as_str(),
                platform.arch.as_str(),
            ))
        })?;

    let dir = remote_server_dir();
    let binary = remote_server_binary();
    let compressed = compress_binary(&local)?;
    let staging = format!("{}.upload.gz", relative_to_home(&binary));

    let mkdir = run_ssh_command(socket_path, &format!("mkdir -p {dir}"), CHECK_TIMEOUT).await?;
    if !mkdir.status.success() {
        return Err(Error::Other(anyhow::anyhow!(
            "mkdir {dir} failed: {}",
            String::from_utf8_lossy(&mkdir.stderr)
        )));
    }

    scp_upload(socket_path, &compressed, &staging, UPLOAD_TIMEOUT)
        .await
        .map_err(Error::Other)?;

    let finalize = format!(
        "gunzip -f {dir}/warp-oss.upload.gz && chmod 755 {dir}/warp-oss.upload && mv -f {dir}/warp-oss.upload {binary}"
    );
    let output = run_ssh_command(socket_path, &finalize, CHECK_TIMEOUT).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Other(anyhow::anyhow!(
            "finalizing remote-server binary failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

pub(super) async fn install_binary(socket_path: &Path) -> InstallOutcome {
    InstallOutcome {
        source: Some(InstallSource::Client),
        result: upload_binary(socket_path).await,
    }
}
