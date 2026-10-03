//! Znimok's update check (ZK-122), by the rules of the security review (docs/security-review-v1.md,
//! «Вимоги до оновлювача»):
//!
//! 1. Only when the person turned daily checks on (`updates.check_daily`, off by default) — the
//!    caller decides when; this crate never runs by itself.
//! 2. Only `github.com/V-Plum/znimok` releases, over HTTPS through the OS stack
//!    ([`znimok_models::http`]).
//! 3. `SHA256SUMS` is verified with the **committed** ECDSA P-256 key ([`RELEASE_KEY`]) before
//!    anything else is downloaded; then the installer's SHA-256 against that verified list.
//! 4. Strictly newer than what runs; the installer goes into the person's own folder, not a
//!    shared temporary one.
//!
//! Installing (Windows: `msiexec` after the app exits, with a way back) and macOS (Sparkle with our
//! window) build on this.

pub mod apply;
pub mod sha256;
pub mod sig;
pub mod version;

use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use version::Version;
use znimok_models::http::Transport;

include!(concat!(env!("OUT_DIR"), "/release_key.rs"));

pub const REPO: &str = "V-Plum/znimok";
const API: &str = "https://api.github.com/repos/V-Plum/znimok/releases/latest";
/// A release file through the API.
const ASSETS: &str = "https://api.github.com/repos/V-Plum/znimok/releases/assets/";
const SMALL: usize = 1 << 20;
/// Largest installer accepted (today ~50 MB).
const INSTALLER_MAX: usize = 512 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// No release key committed yet (ZK-111): updates are not set up in this build.
    NotConfigured,
    Network(String),
    /// GitHub answered something unexpected (status, JSON, a missing file).
    Release(String),
    /// `SHA256SUMS.sig` does not verify with the release key — nothing was installed.
    BadSignature,
    /// The installer is not the file listed in the signed `SHA256SUMS`.
    BadChecksum,
    Io(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "updates are not set up in this build"),
            Self::Network(m) => write!(f, "update check: {m}"),
            Self::Release(m) => write!(f, "update: {m}"),
            Self::BadSignature => write!(f, "update refused: the release signature is not valid"),
            Self::BadChecksum => write!(
                f,
                "update refused: the installer does not match its checksum"
            ),
            Self::Io(m) => write!(f, "update: {m}"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// Which installer this machine takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    WindowsX64,
    MacArm64,
}

impl Platform {
    pub fn current() -> Option<Self> {
        if cfg!(all(windows, target_arch = "x86_64")) {
            Some(Self::WindowsX64)
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Self::MacArm64)
        } else {
            None
        }
    }

    /// The installer's name in a release (release.yml).
    pub fn installer(self, version: &str) -> String {
        match self {
            Self::WindowsX64 => format!("Znimok-{version}-windows-x64.msi"),
            Self::MacArm64 => format!("Znimok-{version}-macos-arm64.dmg"),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    /// The same file through the API (`…/releases/assets/<id>`): the way round when
    /// github.com's own download answers 5xx (ZK-254).
    #[serde(default)]
    url: String,
    size: u64,
}

/// Where a release file is: its download address, and the API's address of the same file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    web: String,
    api: String,
}

impl From<&GhAsset> for Source {
    fn from(a: &GhAsset) -> Self {
        Self {
            web: a.browser_download_url.clone(),
            api: a.url.clone(),
        }
    }
}

/// A newer release for this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Available {
    pub tag: String,
    pub version: String,
    /// The release page (notes).
    pub page: String,
    installer: (String, Source, u64),
    sums: Source,
    sig: Source,
}

impl Available {
    pub fn installer_name(&self) -> &str {
        &self.installer.0
    }
    pub fn installer_size(&self) -> u64 {
        self.installer.2
    }
}

fn headers(current: &str, url: &str) -> [(&'static str, String); 2] {
    // The API gives a file itself (not its description) only when asked for the bytes.
    let accept = if url.starts_with(ASSETS) {
        "application/octet-stream"
    } else {
        "application/vnd.github+json"
    };
    [
        ("user-agent", format!("Znimok/{current}")),
        ("accept", accept.into()),
    ]
}

/// A release file: from its download address, and — when github.com fails (a 5xx, no answer) —
/// from the API's address of the same file (ZK-254: github.com answered 503 for every release
/// download while the API worked). What comes is verified all the same.
fn fetch(
    t: &dyn Transport,
    src: &Source,
    current: &str,
    max: usize,
    timeout: Duration,
) -> Result<Vec<u8>, UpdateError> {
    let first = match get_status(t, &src.web, current, max, timeout) {
        Ok((200, body)) => return Ok(body),
        Ok((status, _)) if status < 500 && status != 429 => {
            return Err(UpdateError::Release(format!(
                "HTTP {status} for {}",
                src.web
            )));
        }
        Ok((status, _)) => UpdateError::Release(format!("HTTP {status} for {}", src.web)),
        Err(e @ UpdateError::Network(_)) => e,
        Err(e) => return Err(e),
    };
    if src.api.is_empty() {
        return Err(first);
    }
    // The first failure is the one worth reading.
    get(t, &src.api, current, max, timeout).map_err(|_| first)
}

fn get(
    t: &dyn Transport,
    url: &str,
    current: &str,
    max: usize,
    timeout: Duration,
) -> Result<Vec<u8>, UpdateError> {
    get_status(t, url, current, max, timeout).and_then(|(status, body)| {
        if status == 200 {
            Ok(body)
        } else {
            Err(UpdateError::Release(format!("HTTP {status} for {url}")))
        }
    })
}

fn get_status(
    t: &dyn Transport,
    url: &str,
    current: &str,
    max: usize,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), UpdateError> {
    // Only our repository's release files, only HTTPS.
    let ours = url.starts_with(API)
        || url.starts_with(ASSETS)
        || url.starts_with(&format!("https://github.com/{REPO}/releases/download/"));
    if !ours {
        return Err(UpdateError::Release(format!(
            "not a {REPO} release URL: {url}"
        )));
    }
    let h = headers(current, url);
    let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let r = t
        .get(url, &h, timeout, max)
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    Ok((r.status, r.body))
}

/// Whether this build can check for updates at all (a release key is committed, ZK-111).
pub fn configured() -> bool {
    RELEASE_KEY.is_some()
}

/// The latest release, if it is newer than `current` and has this platform's installer, the
/// checksums and their signature. `Ok(None)` = up to date.
pub fn check(
    t: &dyn Transport,
    current: &str,
    platform: Platform,
) -> Result<Option<Available>, UpdateError> {
    if RELEASE_KEY.is_none() {
        return Err(UpdateError::NotConfigured);
    }
    let (status, body) = get_status(t, API, current, SMALL, Duration::from_secs(20))?;
    match status {
        200 => pick(&body, current, platform),
        // No release published yet.
        404 => Ok(None),
        s => Err(UpdateError::Release(format!(
            "HTTP {s} for the latest release"
        ))),
    }
}

/// The decision part of [`check`], without the network.
pub fn pick(
    json: &[u8],
    current: &str,
    platform: Platform,
) -> Result<Option<Available>, UpdateError> {
    let rel: GhRelease = serde_json::from_slice(json)
        .map_err(|e| UpdateError::Release(format!("release JSON: {e}")))?;
    let (Some(latest), Some(now)) = (Version::parse(&rel.tag_name), Version::parse(current)) else {
        return Err(UpdateError::Release(format!(
            "version {} / {current}",
            rel.tag_name
        )));
    };
    // `releases/latest` never returns drafts or pre-releases; checked anyway.
    if rel.draft || rel.prerelease || latest.is_prerelease() || latest <= now {
        return Ok(None);
    }
    let version = rel.tag_name.trim_start_matches('v').to_string();
    let find = |name: &str| rel.assets.iter().find(|a| a.name == name);
    let want = platform.installer(&version);
    let (Some(inst), Some(sums), Some(sig)) =
        (find(&want), find("SHA256SUMS"), find("SHA256SUMS.sig"))
    else {
        return Err(UpdateError::Release(format!(
            "{} lacks {want}, SHA256SUMS or its signature",
            rel.tag_name
        )));
    };
    Ok(Some(Available {
        tag: rel.tag_name.clone(),
        version,
        page: rel.html_url,
        installer: (inst.name.clone(), inst.into(), inst.size),
        sums: sums.into(),
        sig: sig.into(),
    }))
}

/// The SHA-256 `SHA256SUMS` lists for `name`.
pub fn listed_digest(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (digest, file) = l.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| digest.to_ascii_lowercase())
    })
}

/// Checks the signed list, then the installer against it; the order matters (rule 3).
pub fn verify_download(
    key_pem: &str,
    sums: &[u8],
    sig: &[u8],
    installer_name: &str,
    installer: &[u8],
) -> Result<(), UpdateError> {
    match sig::verify(key_pem, sums, sig) {
        Ok(true) => {}
        Ok(false) => return Err(UpdateError::BadSignature),
        Err(e) => return Err(UpdateError::Release(e)),
    }
    let sums = std::str::from_utf8(sums).map_err(|_| UpdateError::BadChecksum)?;
    let want = listed_digest(sums, installer_name).ok_or(UpdateError::BadChecksum)?;
    if sha256::hex(&sha256::digest(installer)) != want {
        return Err(UpdateError::BadChecksum);
    }
    Ok(())
}

/// Downloads the signed list, verifies it, then the installer, verifies it and writes it into
/// `dir` (the person's own folder, e.g. `%LOCALAPPDATA%\Znimok\updates`). Returns its path.
pub fn download(
    t: &dyn Transport,
    a: &Available,
    current: &str,
    dir: &Path,
) -> Result<PathBuf, UpdateError> {
    download_with(
        t,
        a,
        current,
        dir,
        RELEASE_KEY.ok_or(UpdateError::NotConfigured)?,
    )
}

/// [`download`] with a given key (tests; the app always uses the committed one).
fn download_with(
    t: &dyn Transport,
    a: &Available,
    current: &str,
    dir: &Path,
    key: &str,
) -> Result<PathBuf, UpdateError> {
    let sums = fetch(t, &a.sums, current, SMALL, Duration::from_secs(30))?;
    let sig = fetch(t, &a.sig, current, 4096, Duration::from_secs(30))?;
    // The signature before the installer is even downloaded.
    match sig::verify(key, &sums, &sig) {
        Ok(true) => {}
        Ok(false) => return Err(UpdateError::BadSignature),
        Err(e) => return Err(UpdateError::Release(e)),
    }
    let (name, src, _) = &a.installer;
    let bytes = fetch(t, src, current, INSTALLER_MAX, Duration::from_secs(600))?;
    verify_download(key, &sums, &sig, name, &bytes)?;
    std::fs::create_dir_all(dir).map_err(|e| UpdateError::Io(e.to_string()))?;
    let path = dir.join(name);
    let tmp = dir.join(format!("{name}.part"));
    std::fs::write(&tmp, &bytes).map_err(|e| UpdateError::Io(e.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| UpdateError::Io(e.to_string()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = include_str!("../tests/fixtures/test-key.pub");
    const SUMS: &[u8] = include_bytes!("../tests/fixtures/SHA256SUMS");
    const SIG: &[u8] = include_bytes!("../tests/fixtures/SHA256SUMS.sig");

    /// ZK-111: the committed release key verifies what CI signs with the private key
    /// (`openssl dgst -sha256 -sign`, as release.yml does); a changed byte does not. On a key
    /// rotation this fixture is signed again with the new key (docs/RELEASE.md).
    #[test]
    fn committed_release_key_verifies_a_ci_signature() {
        let key = RELEASE_KEY.expect("keys/znimok-release-p256.pub.pem is committed");
        let sums = include_bytes!("../tests/release-key/SHA256SUMS");
        let sig = include_bytes!("../tests/release-key/SHA256SUMS.sig");
        assert!(sig::verify(key, sums, sig).unwrap());
        let mut bad = sums.to_vec();
        bad[0] ^= 1;
        assert!(!sig::verify(key, &bad, sig).unwrap());
        assert!(configured());
    }

    fn release(tag: &str, pre: bool, names: &[&str]) -> Vec<u8> {
        let assets: Vec<String> = names
            .iter()
            .map(|n| {
                format!(r#"{{"name":"{n}","browser_download_url":"https://github.com/V-Plum/znimok/releases/download/{tag}/{n}","url":"https://api.github.com/repos/V-Plum/znimok/releases/assets/{n}","size":10}}"#)
            })
            .collect();
        format!(
            r#"{{"tag_name":"{tag}","draft":false,"prerelease":{pre},"html_url":"https://github.com/V-Plum/znimok/releases/tag/{tag}","assets":[{}]}}"#,
            assets.join(",")
        )
        .into_bytes()
    }

    #[test]
    fn only_strictly_newer_full_releases() {
        let all = [
            "Znimok-1.2.0-windows-x64.msi",
            "SHA256SUMS",
            "SHA256SUMS.sig",
        ];
        let r = release("v1.2.0", false, &all);
        let a = pick(&r, "1.1.9", Platform::WindowsX64).unwrap().unwrap();
        assert_eq!(a.version, "1.2.0");
        assert_eq!(a.installer_name(), "Znimok-1.2.0-windows-x64.msi");
        assert_eq!(pick(&r, "1.2.0", Platform::WindowsX64).unwrap(), None);
        assert_eq!(
            pick(&r, "1.3.0", Platform::WindowsX64).unwrap(),
            None,
            "never older"
        );
        assert_eq!(
            pick(
                &release("v1.3.0", true, &all),
                "1.2.0",
                Platform::WindowsX64
            )
            .unwrap(),
            None
        );
        assert_eq!(
            pick(
                &release("v1.3.0-rc.1", false, &all),
                "1.2.0",
                Platform::WindowsX64
            )
            .unwrap(),
            None
        );
        // A release without this platform's installer or without the signature is not offered.
        assert!(pick(&r, "1.0.0", Platform::MacArm64).is_err());
        let unsigned = release(
            "v1.2.0",
            false,
            &["Znimok-1.2.0-windows-x64.msi", "SHA256SUMS"],
        );
        assert!(pick(&unsigned, "1.0.0", Platform::WindowsX64).is_err());
    }

    #[test]
    fn sums_lines() {
        let s =
            "aa  x\nd24ff570a26e4f466ce267ee848c785d7ad6b4bfefc6d5854bed7f5bb739b894 *Znimok.msi\n";
        assert_eq!(listed_digest(s, "x"), None, "short digest");
        assert_eq!(
            listed_digest(s, "Znimok.msi").as_deref(),
            Some("d24ff570a26e4f466ce267ee848c785d7ad6b4bfefc6d5854bed7f5bb739b894")
        );
        assert_eq!(listed_digest(s, "Znimok"), None);
    }

    /// The signature is checked first; then the installer against the signed list.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn signature_then_checksum() {
        let name = "Znimok-1.2.0-windows-x64.msi";
        // The fixture lists a digest we do not have the bytes of: a wrong file fails the checksum.
        assert_eq!(
            verify_download(KEY, SUMS, SIG, name, b"not it"),
            Err(UpdateError::BadChecksum)
        );
        let mut forged = SUMS.to_vec();
        forged[0] = if forged[0] == b'0' { b'1' } else { b'0' };
        assert_eq!(
            verify_download(KEY, &forged, SIG, name, b""),
            Err(UpdateError::BadSignature)
        );
        assert_eq!(
            verify_download(KEY, SUMS, SIG, "other.msi", b""),
            Err(UpdateError::BadChecksum)
        );
    }

    /// A fake GitHub: the release files, and a log of what was asked for.
    #[cfg(any(windows, target_os = "macos"))]
    struct Fake {
        files: Vec<(String, Vec<u8>)>,
        asked: std::sync::Mutex<Vec<String>>,
        /// github.com answers this for every download (the API still works).
        web_status: Option<u16>,
    }

    #[cfg(any(windows, target_os = "macos"))]
    impl Transport for Fake {
        fn post_json(
            &self,
            _: &str,
            _: &[(&str, &str)],
            _: &[u8],
            _: Duration,
        ) -> Result<znimok_models::http::Response, znimok_models::http::HttpError> {
            unreachable!()
        }
        fn get(
            &self,
            url: &str,
            _: &[(&str, &str)],
            _: Duration,
            _: usize,
        ) -> Result<znimok_models::http::Response, znimok_models::http::HttpError> {
            self.asked.lock().unwrap().push(url.to_string());
            if let Some(status) = self
                .web_status
                .filter(|_| url.starts_with("https://github.com/"))
            {
                return Ok(znimok_models::http::Response {
                    status,
                    body: b"<title>Unicorn!</title>".to_vec(),
                    retry_after: None,
                });
            }
            let body = self
                .files
                .iter()
                .find(|(n, _)| url.ends_with(&format!("/{n}")));
            Ok(znimok_models::http::Response {
                status: if body.is_some() { 200 } else { 404 },
                body: body.map(|(_, b)| b.clone()).unwrap_or_default(),
                retry_after: None,
            })
        }
    }

    /// The whole path on a fake release: signed list → installer → verified file in the folder;
    /// a swapped installer is refused; a forged signature stops before the installer is fetched.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn download_verifies_in_order() {
        const INSTALLER: &[u8] = include_bytes!("../tests/fixtures/installer.bin");
        let name = "Znimok-1.2.0-windows-x64.msi";
        let rel = release("v1.2.0", false, &[name, "SHA256SUMS", "SHA256SUMS.sig"]);
        let a = pick(&rel, "1.1.0", Platform::WindowsX64).unwrap().unwrap();
        let dir = std::env::temp_dir().join(format!("znimok-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fake = |installer: &[u8], sig: &[u8]| Fake {
            files: vec![
                (name.into(), installer.to_vec()),
                ("SHA256SUMS".into(), SUMS.to_vec()),
                ("SHA256SUMS.sig".into(), sig.to_vec()),
            ],
            asked: Default::default(),
            web_status: None,
        };

        let good = fake(INSTALLER, SIG);
        let path = download_with(&good, &a, "1.1.0", &dir, KEY).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), INSTALLER);
        assert!(path.starts_with(&dir));

        // ZK-254: github.com answers 503 for every download — the files come through the API,
        // verified all the same; a 404 is not worked around.
        let mut down = fake(INSTALLER, SIG);
        down.web_status = Some(503);
        let path = download_with(&down, &a, "1.1.0", &dir, KEY).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), INSTALLER);
        let asked = down.asked.lock().unwrap().clone();
        assert!(asked.iter().any(|u| u.starts_with(ASSETS)), "{asked:?}");
        let mut missing = fake(INSTALLER, SIG);
        missing.web_status = Some(404);
        assert!(matches!(
            download_with(&missing, &a, "1.1.0", &dir, KEY),
            Err(UpdateError::Release(m)) if m.starts_with("HTTP 404")
        ));
        assert!(
            !missing
                .asked
                .lock()
                .unwrap()
                .iter()
                .any(|u| u.starts_with(ASSETS))
        );
        let mut evil = fake(b"evil", SIG);
        evil.web_status = Some(503);
        assert_eq!(
            download_with(&evil, &a, "1.1.0", &dir, KEY),
            Err(UpdateError::BadChecksum)
        );

        let swapped = fake(b"evil", SIG);
        assert_eq!(
            download_with(&swapped, &a, "1.1.0", &dir, KEY),
            Err(UpdateError::BadChecksum)
        );

        let mut forged = SIG.to_vec();
        let last = forged.len() - 1;
        forged[last] ^= 1;
        let bad = fake(INSTALLER, &forged);
        assert_eq!(
            download_with(&bad, &a, "1.1.0", &dir, KEY),
            Err(UpdateError::BadSignature)
        );
        assert!(
            !bad.asked.lock().unwrap().iter().any(|u| u.ends_with(name)),
            "the installer must not be fetched before the signature verifies"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn foreign_urls_are_refused() {
        struct Never;
        impl Transport for Never {
            fn post_json(
                &self,
                _: &str,
                _: &[(&str, &str)],
                _: &[u8],
                _: Duration,
            ) -> Result<znimok_models::http::Response, znimok_models::http::HttpError> {
                unreachable!()
            }
        }
        for url in [
            "http://github.com/V-Plum/znimok/releases/download/v1/x",
            "https://github.com/evil/znimok/releases/download/v1/x",
            "https://example.com/V-Plum/znimok/releases/download/v1/x",
        ] {
            assert!(matches!(
                get(&Never, url, "1.0.0", 10, Duration::from_secs(1)),
                Err(UpdateError::Release(_))
            ));
        }
    }
}
