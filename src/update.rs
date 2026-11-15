//! Update check against GitHub Releases. Runs in a background thread, once per host process.
//! A release must be tagged `vX.Y.Z` and carry a `*-setup.exe` asset (see release.ps1).

use std::sync::{Arc, Mutex, OnceLock};

pub const REPO: &str = "fggtf24-cyber/clean-voice";
const UA: &str = "clean-voice-updater";

#[derive(Clone, PartialEq)]
pub enum State {
    Checking,
    UpToDate,
    Available { version: String, url: String },
    Downloading,
    Launched,
    Failed,
}

pub type Shared = Arc<Mutex<State>>;

/// Starts the check on first call; later calls return the same shared state.
pub fn state() -> Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| {
        let s: Shared = Arc::new(Mutex::new(State::Checking));
        let s2 = s.clone();
        std::thread::spawn(move || {
            let r = check().unwrap_or(State::Failed);
            *s2.lock().unwrap() = r;
        });
        s
    })
    .clone()
}

fn parse(v: &str) -> Vec<u32> {
    v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn check() -> Option<State> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let json: serde_json::Value = ureq::get(&url).set("User-Agent", UA).call().ok()?.into_json().ok()?;
    let tag = json["tag_name"].as_str()?;
    if parse(tag) <= parse(env!("CARGO_PKG_VERSION")) {
        return Some(State::UpToDate);
    }
    let asset = json["assets"]
        .as_array()?
        .iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with("-setup.exe")))?;
    Some(State::Available { version: tag.trim_start_matches('v').to_string(), url: asset["browser_download_url"].as_str()?.to_string() })
}

/// Downloads the installer to %TEMP% and starts it (it asks for admin rights and offers to
/// close hosts that hold the plugin file).
pub fn install(s: Shared) {
    let State::Available { version, url } = s.lock().unwrap().clone() else { return };
    *s.lock().unwrap() = State::Downloading;
    std::thread::spawn(move || {
        let ok = (|| -> Option<()> {
            let mut body = Vec::new();
            std::io::Read::read_to_end(&mut ureq::get(&url).set("User-Agent", UA).call().ok()?.into_reader(), &mut body).ok()?;
            let path = std::env::temp_dir().join(format!("CleanVoice-{version}-setup.exe"));
            std::fs::write(&path, body).ok()?;
            crate::native::shell_open(path.to_str()?);
            Some(())
        })();
        *s.lock().unwrap() = if ok.is_some() { State::Launched } else { State::Failed };
    });
}
