//! Self-update from GitHub releases.
//!
//! Flow: ask the GitHub API for the latest release, download the Linux binary
//! and its minisign signature, verify the signature against the public key
//! compiled into this binary, check that the new binary actually starts, then
//! swap it in place (a rename, which is safe while the old one is running).
//! Anything that fails leaves the installed version untouched.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

pub const REPO: &str = "LinCastr/lindesk";
/// Release signing key (public half). The private half only exists as a CI secret.
pub const PUBLIC_KEY: &str = "RWRGwnBOFH4dnWuk6H0kLQmOaUC7sTY/dKJE8k7OSyrhVQegRZGazwrt";
pub const ASSET: &str = "lindesk-linux-x86_64";
const MAX_BINARY: u64 = 300 << 20;

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub sig_url: String,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v');
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let v = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    it.next().is_none().then_some(v)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .user_agent(concat!("lindesk/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// The newest release if it is newer than `current`.
pub fn check(current: &str) -> Result<Option<Release>> {
    let cur = parse_version(current).context("bad current version")?;
    let api = std::env::var("LINDESK_API").unwrap_or_else(|_| "https://api.github.com".into());
    let url = format!("{api}/repos/{REPO}/releases/latest");
    let rel: ApiRelease = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .context("could not reach GitHub")?
        .body_mut()
        .read_json()
        .context("unexpected GitHub response")?;
    let Some(latest) = parse_version(&rel.tag_name) else {
        bail!("release tag '{}' is not a version", rel.tag_name);
    };
    if latest <= cur {
        return Ok(None);
    }
    let find = |name: &str| rel.assets.iter().find(|a| a.name == name);
    let bin = find(ASSET).with_context(|| format!("release {} has no {ASSET}", rel.tag_name))?;
    let sig = find(&format!("{ASSET}.minisig")).with_context(|| format!("release {} has no signature", rel.tag_name))?;
    Ok(Some(Release {
        tag: rel.tag_name,
        url: bin.browser_download_url.clone(),
        sig_url: sig.browser_download_url.clone(),
    }))
}

/// Checks `data` against a minisign signature file.
pub fn verify(public_key_b64: &str, data: &[u8], signature_text: &str) -> Result<()> {
    let pk = minisign_verify::PublicKey::from_base64(public_key_b64).map_err(|e| anyhow::anyhow!("bad public key: {e}"))?;
    let sig = minisign_verify::Signature::decode(signature_text).map_err(|e| anyhow::anyhow!("bad signature file: {e}"))?;
    pk.verify(data, &sig, false).map_err(|e| anyhow::anyhow!("signature check failed: {e}"))
}

fn download(url: &str, limit: u64, progress: &dyn Fn(f32)) -> Result<Vec<u8>> {
    let mut resp = agent().get(url).call().context("download failed")?;
    let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let mut reader = resp.body_mut().with_config().limit(limit).reader();
    let mut out = Vec::with_capacity(total.min(limit) as usize);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if total > 0 {
            progress((out.len() as f32 / total as f32).min(1.0));
        }
    }
    Ok(out)
}

/// Downloads, verifies and installs `rel` over the running executable.
pub fn install(rel: &Release, progress: &dyn Fn(f32)) -> Result<()> {
    let exe = installed_exe();
    let dir = exe.parent().context("executable has no directory")?;
    ensure!(
        is_writable(dir),
        "{} is not writable; reinstall with the install script (it uses ~/.local/bin)",
        dir.display()
    );
    let sig = download(&rel.sig_url, 64 << 10, &|_| {})?;
    let bin = download(&rel.url, MAX_BINARY, progress)?;
    verify(PUBLIC_KEY, &bin, std::str::from_utf8(&sig).context("signature is not text")?)?;

    let staged = dir.join(".lindesk.new");
    std::fs::write(&staged, &bin).context("write update")?;
    let result = (|| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
        // The new build must start on this system (libraries, glibc) before it replaces the old one.
        let out = std::process::Command::new(&staged).arg("--version").output().context("new version does not start")?;
        let said = String::from_utf8_lossy(&out.stdout);
        ensure!(out.status.success() && said.contains(rel.tag.trim_start_matches('v')), "new version failed its self-check");
        swap(&exe, &staged)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}

fn swap(exe: &Path, staged: &Path) -> Result<()> {
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).context("move current version aside")?;
    if let Err(e) = std::fs::rename(staged, exe) {
        let _ = std::fs::rename(&old, exe); // roll back
        return Err(e).context("install new version");
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

fn is_writable(dir: &Path) -> bool {
    let probe: PathBuf = dir.join(".lindesk.probe");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

static EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Path of the running executable, captured on first use. It must be read
/// before an update swaps the file: afterwards the kernel reports the old,
/// renamed inode as "<path>.old (deleted)" and the path would be wrong.
pub fn installed_exe() -> PathBuf {
    EXE.get_or_init(|| std::env::current_exe().unwrap_or_else(|_| "lindesk".into())).clone()
}

/// Starts the freshly installed binary with the same arguments and ends this process.
pub fn restart() -> ! {
    use std::os::unix::process::CommandExt;
    let exe = installed_exe();
    let err = std::process::Command::new(exe).args(std::env::args_os().skip(1)).exec();
    eprintln!("could not restart: {err}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("1.10.0-beta.1"), Some((1, 10, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
    }

    #[test]
    fn signature_roundtrip_and_rejection() {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let data = b"binary contents";
        let sig = minisign::sign(None, &kp.sk, std::io::Cursor::new(&data[..]), None, None).unwrap().to_string();
        let pk = kp.pk.to_base64();
        assert!(verify(&pk, data, &sig).is_ok());
        assert!(verify(&pk, b"tampered", &sig).is_err());
        let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        assert!(verify(&other.pk.to_base64(), data, &sig).is_err());
    }

    #[test]
    fn swap_replaces_and_rolls_back() {
        let d = std::env::temp_dir().join(format!("lindesk-swap-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (exe, new) = (d.join("app"), d.join("new"));
        std::fs::write(&exe, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        // staged file missing: the installed one must survive
        assert!(swap(&exe, &d.join("missing")).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        let _ = std::fs::remove_dir_all(d);
    }
}
