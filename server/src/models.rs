//! Model files: from the local Hugging Face cache when they are there
//! (<https://huggingface.co/docs/huggingface_hub/guides/manage-cache>), else from the server's cache folder, downloaded
//! into it first (from the Hub's `resolve` URLs, <https://huggingface.co/docs/hub/models-downloading>). The hf-hub crate
//! does this too, but brings a second HTTP client; this uses udpipe-rs's (ureq) with the operating system's certificate
//! store. Progress goes to stdout as `{"progress": ...}` lines, which the plugin shows.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use candle_core::{Error, Result};
use serde_json::json;

/// The server's cache folder (set once at start-up).
pub static CACHE: OnceLock<PathBuf> = OnceLock::new();

/// The default cache folder: the operating system's per-user cache (Windows `%LOCALAPPDATA%`, macOS `~/Library/Caches`,
/// elsewhere `$XDG_CACHE_HOME` or `~/.cache`, <https://specifications.freedesktop.org/basedir-spec/latest/>).
pub fn default_cache() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Caches"))
    } else {
        env("XDG_CACHE_HOME").or_else(|| env("HOME").map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("augmented-view")
}

/// A file of a model in the local Hugging Face cache: `{HF_HUB_CACHE or HF_HOME/hub}/models--{org}--{name}/snapshots/
/// {revision of refs/main}/{file}`.
fn hf_cache(repo: &str, file: &str) -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let home = env(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(|h| h.join(".cache/huggingface"));
    let hub = env("HF_HUB_CACHE").or_else(|| env("HF_HOME").or(home).map(|h| h.join("hub")))?;
    let dir = hub.join(format!("models--{}", repo.replace('/', "--")));
    let rev = std::fs::read_to_string(dir.join("refs/main")).ok()?;
    Some(dir.join("snapshots").join(rev.trim()).join(file)).filter(|p| p.exists())
}

/// Where this server keeps a model file it downloaded.
pub fn ours(repo: &str, file: &str) -> PathBuf {
    CACHE.get().cloned().unwrap_or_else(default_cache).join("models").join(repo.replace('/', "--")).join(file)
}

/// A model file, downloaded first when neither cache has it.
pub fn need(repo: &str, file: &str) -> Result<PathBuf> {
    if let Some(p) = hf_cache(repo, file) {
        return Ok(p);
    }
    let p = ours(repo, file);
    if !p.exists() {
        download(&format!("https://huggingface.co/{repo}/resolve/main/{file}"), &p, &format!("{repo} {file}"))?;
    }
    Ok(p)
}

/// Download `url` to `dest` (through a `.part` file, so an interrupted download is never taken for a whole one).
pub fn download(url: &str, dest: &Path, what: &str) -> Result<()> {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls).build().into();
    let mut res = agent.get(url).call().map_err(|e| Error::Msg(format!("downloading {what}: {e}")))?;
    let size = res.body().content_length();
    std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")))?;
    let part = dest.with_extension("part");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&part)?);
    let (mut body, mut buf, mut done, mut shown) = (res.body_mut().as_reader(), vec![0u8; 1 << 20], 0u64, u64::MAX);
    loop {
        let n = body.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        let pct = size.map_or(0, |s| done * 100 / s.max(1));
        if pct != shown && (pct % 5 == 0 || shown == u64::MAX) {
            shown = pct;
            let total = size.map_or(String::new(), |s| format!(" of {}", s >> 20));
            println!("{}", json!({ "progress": format!("downloading {what}: {}{total} MB", done >> 20) }));
        }
    }
    out.flush()?;
    drop(out);
    if size.is_some_and(|s| s != done) {
        return Err(Error::Msg(format!("downloading {what}: the connection closed after {done} bytes")));
    }
    std::fs::rename(part, dest)?;
    Ok(())
}
