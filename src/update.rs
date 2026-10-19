//! Checking for a newer release once a day and, when unau came from its
//! installer script, installing it in the background so the next start runs
//! it. Homebrew and cargo installs are told the command instead: updating
//! those unasked would mean running the user's package manager for them.
//!
//! Like `git`, the network is reached by shelling out (`curl`), so unau links
//! no HTTP library. Every failure is silent: no network, no notice.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How long a check stays fresh before GitHub is asked again.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// A lock older than this belongs to an install that died; it is taken over.
const STALE_LOCK: Duration = Duration::from_secs(10 * 60);

const CACHE_FILE: &str = "update.json";
const LOCK_FILE: &str = "update.lock";

/// How the running binary was installed, which decides how it is updated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    Homebrew,
    /// `cargo install`, from crates.io or a checkout.
    Cargo,
    /// The release's installer script, into cargo's bin folder.
    Installer,
    /// A development build, or a copy placed by hand.
    Unknown,
}

impl Install {
    /// The command that updates this kind of install, when there is one.
    fn command(self) -> Option<&'static str> {
        match self {
            Self::Homebrew => Some("brew upgrade unau"),
            Self::Cargo => Some("cargo install unau"),
            Self::Installer | Self::Unknown => None,
        }
    }
}

/// What the status line says about updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    /// A newer release exists; `command` updates this kind of install.
    Available {
        version: String,
        command: Option<&'static str>,
    },
    /// The newer release is installed and runs from the next start.
    Installed,
}

impl UpdateStatus {
    pub fn notice(&self) -> String {
        match self {
            Self::Installed => "Update available, restart to update".to_owned(),
            Self::Available {
                version,
                command: Some(command),
            } => format!("Update {version}: {command}"),
            Self::Available {
                version,
                command: None,
            } => format!("Update {version} available"),
        }
    }
}

/// The last check, kept in the state folder so the network is asked at most
/// once a day and a failed install is not retried on every start.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Cache {
    /// Seconds since the epoch.
    checked: u64,
    latest: String,
    /// The version the installer last ran for.
    tried: String,
}

impl Cache {
    fn fresh(&self, now: u64) -> bool {
        !self.latest.is_empty() && now.saturating_sub(self.checked) < CHECK_EVERY.as_secs()
    }
}

/// Starts the check on its own thread. The receiver yields at most one
/// status, and none when unau is up to date or the check failed.
pub fn spawn() -> mpsc::Receiver<UpdateStatus> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        if let Some(status) = check() {
            let _ = sender.send(status);
        }
    });
    receiver
}

fn check() -> Option<UpdateStatus> {
    let dir = state_dir()?;
    let cache_path = dir.join(CACHE_FILE);
    let now = seconds(SystemTime::now());
    let mut cache: Cache = fs::read_to_string(&cache_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if !cache.fresh(now) {
        cache.latest = latest_release().ok()?;
        cache.checked = now;
        write_cache(&cache_path, &cache);
    }
    if !newer(&cache.latest, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let cargo_bin = cargo_home()?.join("bin");
    let records = fs::read_to_string(cargo_home()?.join(".crates2.json")).ok();
    let install = detect(&exe, &cargo_bin, records.as_deref());
    let latest = cache.latest.clone();
    if install == Install::Installer && cache.tried != latest {
        cache.tried.clone_from(&latest);
        write_cache(&cache_path, &cache);
        if run_installer(&dir, &latest).is_ok() {
            return Some(UpdateStatus::Installed);
        }
    }
    Some(UpdateStatus::Available {
        version: latest,
        command: install.command(),
    })
}

fn write_cache(path: &Path, cache: &Cache) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(cache) {
        let _ = fs::write(path, text);
    }
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `$XDG_STATE_HOME/unau`, else `~/.local/state/unau`.
fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("unau"))
}

/// `$CARGO_HOME`, else `~/.cargo`: where the installer script puts unau.
fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
}

/// The repository's address on GitHub, from `Cargo.toml`.
fn repository() -> &'static str {
    env!("CARGO_PKG_REPOSITORY").trim_end_matches('/')
}

/// The newest release's version, from GitHub's API.
fn latest_release() -> Result<String> {
    let api = repository().replacen("https://github.com/", "https://api.github.com/repos/", 1);
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "5"])
        .args(["-H", "Accept: application/vnd.github+json"])
        .arg(format!("{api}/releases/latest"))
        .stdin(Stdio::null())
        .output()
        .context("run curl")?;
    if !output.status.success() {
        bail!("curl failed");
    }
    release_version(&String::from_utf8_lossy(&output.stdout)).context("no tag in the release")
}

/// `tag_name` without its `v`, from a release in GitHub's JSON.
fn release_version(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = value["tag_name"].as_str()?;
    Some(tag.strip_prefix('v').unwrap_or(tag).to_owned())
}

/// Whether `latest` is a later `major.minor.patch` than `current`.
fn newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> {
        let numbers: Option<Vec<u64>> = v.split('.').map(|p| p.parse().ok()).collect();
        numbers.filter(|n| n.len() == 3)
    };
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

/// How the binary at `exe` (with links resolved) was installed. cargo records
/// every `cargo install` in `.crates2.json`; a unau in cargo's bin folder that
/// is not recorded there came from the installer script.
fn detect(exe: &Path, cargo_bin: &Path, cargo_records: Option<&str>) -> Install {
    if exe.components().any(|c| c.as_os_str() == "Cellar") {
        return Install::Homebrew;
    }
    let in_cargo_bin = exe.parent().is_some_and(|dir| {
        dir == cargo_bin || cargo_bin.canonicalize().is_ok_and(|bin| dir == bin)
    });
    if !in_cargo_bin {
        return Install::Unknown;
    }
    let recorded = cargo_records
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|value| {
            value["installs"]
                .as_object()
                .map(|installs| installs.keys().any(|k| k.starts_with("unau ")))
        })
        .unwrap_or(false);
    if recorded {
        Install::Cargo
    } else {
        Install::Installer
    }
}

/// Runs `version`'s own installer script, quietly and without touching the
/// shell profile. The lock keeps two unau windows from installing at once.
fn run_installer(dir: &Path, version: &str) -> Result<()> {
    fs::create_dir_all(dir)?;
    let lock = dir.join(LOCK_FILE);
    let stale = fs::metadata(&lock)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > STALE_LOCK));
    if stale {
        let _ = fs::remove_file(&lock);
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .context("another unau is installing")?;
    let url = format!(
        "{}/releases/download/v{version}/unau-installer.sh",
        repository()
    );
    let status = Command::new("sh")
        .arg("-c")
        .arg("curl --proto '=https' --tlsv1.2 -LsSf \"$0\" | sh -s -- -q")
        .arg(&url)
        .env("UNAU_NO_MODIFY_PATH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = fs::remove_file(&lock);
    if !status.context("run the installer")?.success() {
        bail!("the installer failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_later_three_part_version_counts_as_newer() {
        assert!(newer("0.2.1", "0.2.0"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0.0", "0.99.0"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
        assert!(!newer("0.3.0-beta", "0.2.0"));
        assert!(!newer("", "0.2.0"));
    }

    #[test]
    fn the_release_tag_is_read_without_its_v() {
        assert_eq!(
            release_version(r#"{"tag_name": "v0.2.1", "name": "0.2.1"}"#).as_deref(),
            Some("0.2.1")
        );
        assert_eq!(release_version(r#"{"message": "Not Found"}"#), None);
        assert_eq!(release_version("not json"), None);
    }

    #[test]
    fn the_install_is_told_apart_by_where_the_binary_is_and_what_cargo_recorded() {
        let bin = Path::new("/home/me/.cargo/bin");
        let cellar = Path::new("/opt/homebrew/Cellar/unau/0.2.0/bin/unau");
        assert_eq!(detect(cellar, bin, None), Install::Homebrew);
        let cargo = r#"{"installs": {"unau 0.2.0 (registry+https://github.com/rust-lang/crates.io-index)": {}}}"#;
        let other = r#"{"installs": {"ripgrep 14.1.0 (registry+…)": {}}}"#;
        let in_bin = Path::new("/home/me/.cargo/bin/unau");
        assert_eq!(detect(in_bin, bin, Some(cargo)), Install::Cargo);
        assert_eq!(detect(in_bin, bin, Some(other)), Install::Installer);
        assert_eq!(detect(in_bin, bin, None), Install::Installer);
        let dev = Path::new("/home/me/src/unau/target/release/unau");
        assert_eq!(detect(dev, bin, Some(cargo)), Install::Unknown);
    }

    #[test]
    fn the_notice_names_the_command_or_asks_for_a_restart() {
        let brew = UpdateStatus::Available {
            version: "0.2.1".into(),
            command: Install::Homebrew.command(),
        };
        assert_eq!(brew.notice(), "Update 0.2.1: brew upgrade unau");
        let unknown = UpdateStatus::Available {
            version: "0.2.1".into(),
            command: Install::Unknown.command(),
        };
        assert_eq!(unknown.notice(), "Update 0.2.1 available");
        assert_eq!(
            UpdateStatus::Installed.notice(),
            "Update available, restart to update"
        );
    }

    #[test]
    fn a_check_stays_fresh_for_a_day() {
        let cache = Cache {
            checked: 1_000,
            latest: "0.2.0".into(),
            tried: String::new(),
        };
        assert!(cache.fresh(1_000 + CHECK_EVERY.as_secs() - 1));
        assert!(!cache.fresh(1_000 + CHECK_EVERY.as_secs()));
        assert!(!Cache::default().fresh(0), "nothing known yet");
    }
}
