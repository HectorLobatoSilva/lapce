use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use lapce_proxy::{directory::Directory, VERSION};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub target_commitish: String,
    pub assets: Vec<ReleaseAsset>,
    #[serde(skip)]
    pub version: String,
}

#[derive(Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
}

pub fn get_latest_release() -> Result<ReleaseInfo> {
    let version = *VERSION;
    let url = match version {
        "debug" => {
            return Err(anyhow!("no release for debug"));
        }
        version if version.starts_with("nightly") => {
            "https://api.github.com/repos/lapce/lapce/releases/tags/nightly"
        }
        _ => "https://api.github.com/repos/lapce/lapce/releases/latest",
    };

    let resp = reqwest::blocking::ClientBuilder::new()
        .user_agent("Lapce")
        .build()?
        .get(url)
        .send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("get release info failed {}", resp.text()?));
    }
    let mut release: ReleaseInfo = serde_json::from_str(&resp.text()?)?;

    release.version = match release.tag_name.as_str() {
        "nightly" => format!("nightly-{}", &release.target_commitish[..7]),
        _ => release.tag_name[1..].to_string(),
    };

    Ok(release)
}

pub fn download_release(release: &ReleaseInfo) -> Result<PathBuf> {
    let dir =
        Directory::updates_directory().ok_or_else(|| anyhow!("no directory"))?;
    let name = match std::env::consts::OS {
        "macos" => "Lapce-macos.dmg",
        "linux" => "Lapce-linux.tar.gz",
        "windows" => "Lapce-windows-portable.zip",
        _ => return Err(anyhow!("os not supported")),
    };
    let file_path = dir.join(name);

    for asset in &release.assets {
        if asset.name == name {
            let mut resp = reqwest::blocking::get(&asset.browser_download_url)?;
            if !resp.status().is_success() {
                return Err(anyhow!("download file error {}", resp.text()?));
            }
            let mut out = std::fs::File::create(&file_path)?;
            resp.copy_to(&mut out)?;
            return Ok(file_path);
        }
    }

    Err(anyhow!("can't download release"))
}

#[cfg(target_os = "macos")]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let info = dmg::Attach::new(src).with()?;
    let dest = process_path.parent().ok_or_else(|| anyhow!("no parent"))?;
    let dest = if dest.file_name().and_then(|s| s.to_str()) == Some("MacOS") {
        dest.parent().unwrap().parent().unwrap().parent().unwrap()
    } else {
        dest
    };
    let _ = std::fs::remove_dir_all(dest.join("Lapce.app"));
    fs_extra::copy_items(
        &[info.mount_point.join("Lapce.app")],
        dest,
        &fs_extra::dir::CopyOptions {
            overwrite: true,
            skip_exist: false,
            buffer_size: 64000,
            copy_inside: true,
            content_only: false,
            depth: 0,
        },
    )?;
    Ok(dest.join("Lapce.app"))
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let tar_gz = std::fs::File::open(src)?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("no parent"))?;
    archive.unpack(parent)?;
    std::fs::remove_file(process_path)?;
    std::fs::copy(parent.join("Lapce").join("lapce"), process_path)?;
    Ok(process_path.to_path_buf())
}

#[cfg(target_os = "windows")]
pub fn extract(src: &Path, process_path: &Path) -> Result<PathBuf> {
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("no parent"))?;
    {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(src)?)?;
        archive.extract(parent)?;
    }
    std::fs::remove_file(process_path)?;
    std::fs::copy(parent.join("lapce.exe"), process_path)?;
    Ok(process_path.to_path_buf())
}

#[cfg(target_os = "macos")]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    std::process::Command::new("open")
        .arg("-n")
        .arg(path)
        .arg("--args")
        .arg("-n")
        .exec();
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(path).arg("-n").exec();
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn restart(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x00000008;
    let process_id = std::process::id();
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("can't get path to str"))?;
    std::process::Command::new("cmd")
        .arg("/C")
        .arg(format!("taskkill /PID {} & start {} -n", process_id, path))
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}
