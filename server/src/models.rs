//! The model files the server needs, where they go, whether they are on disk, and their download, which only the user
//! starts. Obsidian's developer policies forbid plugins that "install or update themselves or their dependencies"
//! (<https://docs.obsidian.md/Developer+policies>), so the plugin never downloads a model and never starts a download
//! (user, 2026-10-28):
//!
//! - the server does not start while a model it needs is missing ([`missing`]): it prints one `{"missing": [...]}`
//!   line (each model's id, purpose, page, licence, folder, the files still missing and their size) and exits with
//!   code 4 ([`EXIT_MISSING`]). The plugin then says which, and its Models section shows the command below;
//! - `augmented-view-server --download all` (or `--download terms,phrases`: ids) is the download-only run the user
//!   starts in a terminal ([`download_only`]): it fetches the missing files with its progress printed to the terminal
//!   and exits. Each file goes through a `.part` file, is resumed when a run was interrupted (Ctrl+C, a lost
//!   connection), and is checked against its size and the checksum Hugging Face publishes (native-kit's `download`);
//!   the parser, from LINDAT, against the SHA-256 of the file whose MD5 LINDAT publishes;
//! - a server the user starts to serve by hand, in a terminal ([`may_ask`]: its input and output are terminals, and
//!   no `--no-prompt`), lists the missing models and asks `Download them now? [y/N]` ([`ask`]); on a yes it downloads
//!   them as `--download all` would and then starts, on anything else it exits as above with the command to run
//!   later. Started by the plugin (input ignored, output piped) it is never asked.
//!
//! The manifest pins each default model to a commit, with its files' sizes and checksums from the Hugging Face API
//! (`https://huggingface.co/api/models/<repo>?blobs=true`, checked 2026-10-28). A model set by hand (`--model`,
//! `--claims-model`) is listed from the same API when it is downloaded. Files are read from the local Hugging Face
//! cache when they are there (<https://huggingface.co/docs/huggingface_hub/guides/manage-cache>), else from the
//! server's cache folder (native-kit's `models`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use candle_core::{Error, Result};
use native_kit::download::{self, FileSpec, HUB};
use serde_json::{json, Value};

/// The server's cache folder (set once at start-up).
pub static CACHE: OnceLock<PathBuf> = OnceLock::new();

/// The start of the error for a model file that is not on disk.
pub use native_kit::models::MISSING;

/// The server's exit code when it will not start because model files are missing (Quarry's service uses the same).
pub const EXIT_MISSING: i32 = 4;

/// The default cache folder: the operating system's per-user cache (native-kit's `default_cache`).
pub fn default_cache() -> PathBuf {
    native_kit::models::default_cache("augmented-view")
}

fn cache() -> PathBuf {
    CACHE.get().cloned().unwrap_or_else(default_cache)
}

/// Where this server keeps a model file it downloaded.
pub fn ours(repo: &str, file: &str) -> PathBuf {
    native_kit::models::ours(&cache(), repo, file)
}

/// A model file on disk, or the error naming it (`MISSING`). Never downloads.
pub fn need(repo: &str, file: &str) -> Result<PathBuf> {
    native_kit::models::need(&cache(), repo, file).map_err(Error::Msg)
}

/// The UDPipe model's download (LINDAT's bitstream of <https://hdl.handle.net/11234/1-3131>).
const PARSER_URL: &str = "https://lindat.mff.cuni.cz/repository/server/api/core/bitstreams/8d51e3d3-1f87-47fe-8975-c197ddf7907b/content";
/// Its size and SHA-256, of the file whose MD5 LINDAT publishes (4284eb5c16348e716d739b8ecec7de6d).
const PARSER_BYTES: u64 = 16_309_608;
const PARSER_SHA256: &str = "784bd0fa85e3d831fd02a55290d0acfd05c953159dc38cc33d52e1b28add9957";

/// What a run needs, from its arguments.
#[derive(Clone, Debug)]
pub struct Needs {
    pub cache: PathBuf,
    pub model: String,
    pub claims_model: String,
    /// Sentence colours (not `--no-claims`).
    pub claims: bool,
    /// Phrase colours (`--phrases`: a colour code has them).
    pub phrases: bool,
}

/// Where a model comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A Hugging Face repository at a commit ("main" for a model set by hand).
    Hub { repo: String, revision: String },
    /// One file from elsewhere (the parser, from LINDAT).
    Url(&'static str),
}

/// A model of the manifest.
#[derive(Clone, Debug)]
pub struct Model {
    pub id: &'static str,
    pub purpose: &'static str,
    /// Its name: the repository, or the parser's.
    pub name: String,
    /// Its page, opened in the browser from the plugin's Models section.
    pub page: String,
    pub source: Source,
    pub licence: String,
    pub licence_url: String,
    /// The files, or None for a model set by hand (listed from the Hub's API when it is downloaded).
    pub files: Option<Vec<FileSpec>>,
}

type Row = (&'static str, u64, Option<&'static str>, &'static str);

/// A file of the manifest: (name, bytes, SHA-256 of a Git LFS file, Git blob SHA-1 of a small one).
fn specs(rows: &[Row]) -> Vec<FileSpec> {
    rows.iter()
        .map(|(n, b, sha, git)| FileSpec { name: n.to_string(), bytes: *b, sha256: sha.map(String::from), git_sha1: sha.is_none().then(|| git.to_string()) })
        .collect()
}

pub const TERMS_REPO: &str = "BAAI/bge-base-en-v1.5";
pub const CLAIMS_REPO: &str = "Qwen/Qwen3-4B-Instruct-2507";

/// The default models (sizes and checksums from the Hub's API for the pinned commits, 2026-10-28).
const TERMS: (&str, &[Row]) = ("a5beb1e3e68b9ab74eb54cfd186867f64f240e1a", &[
    ("config.json", 777, None, "09a8200d8a549a3b9dba53b67156ca26810cc720"),
    ("tokenizer.json", 711396, None, "688882a79f44442ddc1f60d70334a7ff5df0fb47"),
    ("model.safetensors", 437955512, Some("c7c1988aae201f80cf91a5dbbd5866409503b89dcaba877ca6dba7dd0a5167d7"), ""),
]);
const CLAIMS: (&str, &[Row]) = ("cdbee75f17c01a7cc42f958dc650907174af0554", &[
    ("config.json", 727, None, "6988f134db143052042f2bd6e0c897bc6a605189"),
    ("tokenizer.json", 11422654, Some("aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4"), ""),
    ("tokenizer_config.json", 9377, None, "51c1be0d9192e7f6e6596de71d0f07d58fbc32ac"),
    ("model.safetensors.index.json", 32819, None, "4747b0297d3109f14db49886972e3369c9a00b2a"),
    ("model-00001-of-00003.safetensors", 3957900840, Some("75311d91bb08cf0b882913da464a1e722a31fb44db35208663487efb7a3d8ed6"), ""),
    ("model-00002-of-00003.safetensors", 3987450520, Some("0b48adbb1f60e901153d91907ba11ce63bd4b8b584482e730f48808d055dfba1"), ""),
    ("model-00003-of-00003.safetensors", 99630640, Some("7dd39ccca5e4de123c74c14af44c9bf2eb75df33b4614382af0134528e060d5d"), ""),
]);
const PHRASES: (&str, &[Row]) = ("f9634218e53580c56edf0de97ca1a7d3f1c2354e", &[
    ("config.json", 236, None, "01f0a2ca100d65c3303b1724f4447ba73cfa225f"),
    ("encoder_config/config.json", 823, None, "1842021ed80c7a06b5682b7efd1a85d5ef827403"),
    ("tokenizer.json", 8650701, None, "a0235ed3d37fd39d6f2acd76f48632a7bb7238de"),
    ("model.safetensors", 833938108, Some("845fc4bd93c525b86124c58ab4f56c9eacf8587953086b14c501fab25957c007"), ""),
]);

const APACHE: &str = "https://www.apache.org/licenses/LICENSE-2.0";

/// Every model id, in the order they are listed.
pub const IDS: [&str; 4] = ["parser", "terms", "colours", "phrases"];

/// A Hugging Face model: pinned with its files when `repo` is the default, else listed when downloaded.
fn hub(id: &'static str, purpose: &'static str, repo: &str, default: (&str, (&str, &[Row])), licence: (&str, &str)) -> Model {
    let (default_repo, (revision, rows)) = default;
    let own = repo == default_repo;
    Model {
        id,
        purpose,
        name: repo.into(),
        page: format!("{HUB}/{repo}"),
        source: Source::Hub { repo: repo.into(), revision: if own { revision.into() } else { "main".into() } },
        licence: if own { licence.0.into() } else { "see the model's page".into() },
        licence_url: if own { licence.1.into() } else { format!("{HUB}/{repo}") },
        files: own.then(|| specs(rows)),
    }
}

/// Every model the server can use, for these arguments (the embedding and instruct models they name).
pub fn manifest(n: &Needs) -> Vec<Model> {
    vec![
        Model {
            id: "parser",
            purpose: "Sentence structure: the parse that key terms, quotations and definitions are read from (UDPipe 1, Universal Dependencies 2.5 English EWT)",
            name: "UDPipe English EWT (UD 2.5)".into(),
            page: "https://hdl.handle.net/11234/1-3131".into(),
            source: Source::Url(PARSER_URL),
            licence: "CC BY-NC-SA 4.0 (personal, non-commercial use)".into(),
            licence_url: "https://creativecommons.org/licenses/by-nc-sa/4.0/".into(),
            files: Some(vec![FileSpec { name: crate::PARSER.into(), bytes: PARSER_BYTES, sha256: Some(PARSER_SHA256.into()), git_sha1: None }]),
        },
        hub("terms", "Key terms: ranks each paragraph's words and phrases for emphasis (an embedding model)", &n.model, (TERMS_REPO, TERMS), ("MIT", "https://opensource.org/license/mit")),
        hub("colours", "Sentence colours: picks each sentence's colour from the code's descriptions, and the contrast words (converted once on this computer into a 2.4 GB 4-bit file, after which the download is deleted)", &n.claims_model, (CLAIMS_REPO, CLAIMS), ("Apache-2.0", APACHE)),
        hub("phrases", "Phrase colours: finds the phrases each phrase colour describes (GLiNER2, on the CPU)", crate::phrases::REPO, (crate::phrases::REPO, PHRASES), ("Apache-2.0", APACHE)),
    ]
}

/// The ids this run needs: the parser and key terms always, sentence colours unless `--no-claims`, phrase colours with
/// `--phrases`.
pub fn needed(n: &Needs) -> Vec<&'static str> {
    IDS.into_iter().filter(|id| match *id {
        "colours" => n.claims,
        "phrases" => n.phrases,
        _ => true,
    }).collect()
}

/// The folder a model's files go in.
pub fn folder(m: &Model, cache: &Path) -> PathBuf {
    match &m.source {
        Source::Url(_) => cache.to_path_buf(),
        Source::Hub { repo, .. } => native_kit::models::ours(cache, repo, ""),
    }
}

/// Where a file of a model is downloaded to.
fn dest(m: &Model, cache: &Path, name: &str) -> PathBuf {
    folder(m, cache).join(name)
}

/// Where a file of a model is on disk, if it is there whole (a file this server downloaded is also checked for size).
fn present(m: &Model, cache: &Path, f: &FileSpec) -> Option<PathBuf> {
    let whole = |p: &Path| f.bytes == 0 || std::fs::metadata(p).is_ok_and(|md| md.len() == f.bytes);
    let ours = Some(dest(m, cache, &f.name)).filter(|p| whole(p));
    match &m.source {
        Source::Url(_) => ours,
        Source::Hub { repo, .. } => native_kit::models::hf_cache(repo, &f.name).or(ours),
    }
}

/// The instruct model's 4-bit file (claims.rs), which once made replaces the official weights.
pub fn gguf(cache: &Path, repo: &str) -> PathBuf {
    cache.join(format!("{}-q4k.gguf", repo.replace('/', "--")))
}

/// The files of a model set by hand that it needs, as claims.rs and embed.rs read them, by name.
fn wanted(id: &str) -> fn(&str) -> bool {
    match id {
        "terms" => |f| matches!(f, "config.json" | "tokenizer.json" | "model.safetensors"),
        _ => |f| matches!(f, "config.json" | "tokenizer.json" | "tokenizer_config.json" | "model.safetensors.index.json") || (f.ends_with(".safetensors") && !f.contains('/')),
    }
}

/// The files of a model set by hand that are on disk, by the names `wanted` keeps (a sharded model's shards from its
/// index): (all there, any there).
fn custom_on_disk(m: &Model, cache: &Path) -> (bool, bool) {
    let Source::Hub { repo, .. } = &m.source else { return (false, false) };
    let has = |f: &str| native_kit::models::local(cache, repo, f).is_some();
    let mut need: Vec<String> = match m.id {
        "terms" => vec!["config.json".into(), "tokenizer.json".into(), "model.safetensors".into()],
        _ => vec!["config.json".into(), "tokenizer.json".into(), "tokenizer_config.json".into(), "model.safetensors.index.json".into()],
    };
    if m.id != "terms" {
        if let Some(index) = native_kit::models::local(cache, repo, "model.safetensors.index.json") {
            let v: Value = std::fs::read_to_string(index).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            let mut shards: Vec<String> = v["weight_map"].as_object().into_iter().flatten().filter_map(|(_, f)| f.as_str().map(String::from)).collect();
            shards.sort();
            shards.dedup();
            need.extend(shards);
        }
    }
    let n = need.iter().filter(|f| has(f)).count();
    (n == need.len(), n > 0)
}

/// Where one model stands on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    /// "present" (every file), "partial" (some, or a download begun) or "missing".
    pub status: &'static str,
    /// Whether the server can run it without downloading anything (for the instruct model, its 4-bit file and
    /// tokenizer are enough).
    pub ready: bool,
    /// The files still to download, and their bytes (0 when unknown, for a model set by hand).
    pub files: Vec<String>,
    pub bytes: u64,
}

/// Where `m` stands.
pub fn standing(m: &Model, cache: &Path) -> Standing {
    let converted = m.id == "colours" && matches!(&m.source, Source::Hub { repo, .. } if gguf(cache, repo).exists());
    match &m.files {
        Some(files) => {
            // once the 4-bit file is made, only the tokenizer's files are still read
            let still: Vec<&FileSpec> = files.iter().filter(|f| !converted || f.name.starts_with("tokenizer")).collect();
            let gone: Vec<&FileSpec> = still.iter().copied().filter(|f| present(m, cache, f).is_none()).collect();
            let begun = files.iter().any(|f| download::part_of(&dest(m, cache, &f.name)).exists());
            let status = if gone.is_empty() { "present" } else if gone.len() < still.len() || begun { "partial" } else { "missing" };
            Standing { status, ready: gone.is_empty(), files: gone.iter().map(|f| f.name.clone()).collect(), bytes: gone.iter().map(|f| f.bytes).sum() }
        }
        None => {
            let (all, any) = custom_on_disk(m, cache);
            let ready = all || (converted && ["tokenizer.json", "tokenizer_config.json"].iter().all(|f| matches!(&m.source, Source::Hub { repo, .. } if native_kit::models::local(cache, repo, f).is_some())));
            Standing { status: if all { "present" } else if any { "partial" } else { "missing" }, ready, files: Vec::new(), bytes: 0 }
        }
    }
}

/// A model as the plugin shows it.
fn describe(m: &Model, cache: &Path) -> Value {
    json!({
        "id": m.id, "purpose": m.purpose, "name": m.name, "page": m.page, "licence": m.licence, "licence_url": m.licence_url,
        "from": match &m.source { Source::Hub { .. } => HUB.to_string(), Source::Url(u) => u.split('/').take(3).collect::<Vec<_>>().join("/") },
        "folder": folder(m, cache).display().to_string(),
        "bytes": m.files.iter().flatten().map(|f| f.bytes).sum::<u64>(),
    })
}

/// The models this run needs that are not ready, as the `{"missing": [...]}` line lists them: each described, with
/// the files still missing and their bytes. Empty when the server can start.
pub fn missing(n: &Needs) -> Vec<Value> {
    let ids = needed(n);
    manifest(n)
        .iter()
        .filter(|m| ids.contains(&m.id))
        .filter_map(|m| {
            let st = standing(m, &n.cache);
            (!st.ready).then(|| {
                let mut v = describe(m, &n.cache);
                v["files"] = json!(st.files);
                v["missing_bytes"] = json!(st.bytes);
                v["note"] = json!(if m.files.is_none() { "Set by hand: its files and size are listed from Hugging Face when it is downloaded." } else { "" });
                v
            })
        })
        .collect()
}

/// GET /models: every model this run needs, with where it stands.
pub fn report(n: &Needs) -> Value {
    let ids = needed(n);
    let models: Vec<Value> = manifest(n)
        .iter()
        .filter(|m| ids.contains(&m.id))
        .map(|m| {
            let st = standing(m, &n.cache);
            let mut v = describe(m, &n.cache);
            v["status"] = json!(st.status);
            v["ready"] = json!(st.ready);
            v["files"] = json!(m.files.iter().flatten().map(|f| f.name.clone()).collect::<Vec<_>>());
            v["missing_bytes"] = json!(if st.ready { 0 } else { st.bytes });
            v
        })
        .collect();
    json!({ "models": models })
}

/// Bytes as the messages give them ("about 8.9 GB").
pub fn gb(bytes: u64) -> String {
    match bytes {
        b if b >= 100_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        b => format!("{} MB", b.div_ceil(1_000_000)),
    }
}

/// The models a `--download` names: "all" (or nothing) for those this run needs, else ids separated by commas.
pub fn chosen(n: &Needs, ids: &str) -> std::result::Result<Vec<Model>, String> {
    let all = manifest(n);
    let want: Vec<&str> = if ids.trim().is_empty() || ids.trim() == "all" { needed(n) } else { ids.split(',').map(str::trim).filter(|s| !s.is_empty()).collect() };
    if let Some(bad) = want.iter().find(|id| !IDS.contains(id)) {
        return Err(format!("no model {bad:?}: the ids are {}, or all", IDS.join(", ")));
    }
    Ok(all.into_iter().filter(|m| want.contains(&m.id)).collect())
}

/// Set by Ctrl+C: the download stops after its current read, keeping its `.part` for the next run.
static CANCEL: AtomicBool = AtomicBool::new(false);
/// A download is running: Ctrl+C stops it gently. Otherwise (a server that downloaded at the terminal's question and
/// went on to serve) Ctrl+C ends the program at once, as it does a server that never downloaded.
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// Ctrl+C during a download, once: stop after the current read (the next run resumes); twice: stop at once. Outside a
/// download: stop at once. (tokio's ctrl_c, <https://docs.rs/tokio/latest/tokio/signal/fn.ctrl_c.html>,
/// SetConsoleCtrlHandler on Windows, SIGINT elsewhere.)
fn on_ctrl_c() {
    std::thread::spawn(|| {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        rt.block_on(async {
            while tokio::signal::ctrl_c().await.is_ok() {
                if !DOWNLOADING.load(Ordering::SeqCst) || CANCEL.swap(true, Ordering::SeqCst) {
                    std::process::exit(130);
                }
                eprintln!("\nStopping: what was downloaded is kept, and running the command again resumes it. (Ctrl+C again stops at once.)");
            }
        });
    });
}

/// Whether a start to serve with models missing may ask, in the terminal, to download them (user, 2026-10-28): only
/// when a person started the server by hand in a terminal, that is when its input, its output and its error output
/// are all terminals, and never with `--no-prompt`. The plugin starts the server with its input ignored and its output
/// piped (app.rs, spawn_server), so a start from Obsidian is never asked and never downloads.
pub fn may_ask(stdin_terminal: bool, stdout_terminal: bool, stderr_terminal: bool, no_prompt: bool) -> bool {
    stdin_terminal && stdout_terminal && stderr_terminal && !no_prompt
}

/// The answer to `Download them now? [y/N]`: "y" or "yes" in any case, spaces around allowed; anything else, an empty
/// line or the end of the input, is no.
pub fn accepts(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// The command that downloads the missing models later, as the user would type it: this program, `--download all`,
/// then the arguments this run was given (so it downloads what this run needs: `--phrases`, `--model` …). On Windows,
/// PowerShell's call operator `&` runs a quoted path.
pub fn later_command(program: &str, args: &[String], windows: bool) -> String {
    let quote = |s: &str| if s.is_empty() || s.contains([' ', '\'']) { format!("\"{s}\"") } else { s.to_string() };
    let mut c = format!("{}\"{program}\" --download all", if windows { "& " } else { "" });
    for a in args.iter().filter(|a| *a != "--no-prompt") {
        c.push(' ');
        c.push_str(&quote(a));
    }
    c
}

/// The terminal's question: the missing models listed (each one's size, page and licence), their total, the folder
/// they go in, then `Download them now? [y/N]`. True on a yes.
pub fn ask(lacking: &[Value], total: u64, folder: &Path) -> bool {
    println!("Augmented View's server needs these models, which are not on this computer yet:");
    for m in lacking {
        let bytes = m["missing_bytes"].as_u64().unwrap_or(0);
        let size = if bytes > 0 { gb(bytes) } else { "size listed when downloaded".into() };
        println!("  - {} ({}): {}", m["name"].as_str().unwrap_or("?"), size, m["purpose"].as_str().unwrap_or(""));
        println!("      page {}  licence {} ({})", m["page"].as_str().unwrap_or(""), m["licence"].as_str().unwrap_or(""), m["licence_url"].as_str().unwrap_or(""));
        println!("      into {}", m["folder"].as_str().unwrap_or(""));
    }
    println!("In all about {}, from Hugging Face (huggingface.co) and LINDAT (lindat.mff.cuni.cz), into {}.", gb(total), folder.display());
    println!("Each file is checked against its published size and checksum; Ctrl+C stops, and running again resumes.");
    print!("Download them now? [y/N] ");
    _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok() && accepts(&answer)
}

/// The download-only run (`--download <ids|all>`), which the user runs in a terminal: the chosen models' missing
/// files, fetched one by one with the progress on the terminal, then the run exits (0 done, 1 failed, 2 a wrong id,
/// 130 stopped with Ctrl+C).
pub fn download_only(n: &Needs, ids: &str) -> ! {
    std::process::exit(download(n, ids, false))
}

/// The chosen models' missing files, fetched one by one with the progress on the terminal; the exit code of the run
/// (0 done, 1 failed, 2 a wrong id, 130 stopped with Ctrl+C). `then_serve`: the user said yes to the terminal's
/// question, and the server starts once the files are here.
pub fn download(n: &Needs, ids: &str, then_serve: bool) -> i32 {
    let models = match chosen(n, ids) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    on_ctrl_c();
    DOWNLOADING.store(true, Ordering::SeqCst);
    let code = fetch_all(n, &models, then_serve);
    DOWNLOADING.store(false, Ordering::SeqCst);
    code
}

fn fetch_all(n: &Needs, models: &[Model], then_serve: bool) -> i32 {
    let agent = download::agent();
    // the files to fetch: a model set by hand is listed from the Hub's API first
    let mut todo: Vec<(Model, FileSpec, String)> = Vec::new();
    for m in models {
        let st = standing(m, &n.cache);
        if st.ready {
            continue;
        }
        let files: Vec<(FileSpec, String)> = match &m.source {
            Source::Url(u) => m.files.clone().unwrap_or_default().into_iter().map(|f| (f, u.to_string())).collect(),
            Source::Hub { repo, revision } => {
                let files: Vec<FileSpec> = match &m.files {
                    Some(f) => f.iter().filter(|f| st.files.contains(&f.name)).cloned().collect(),
                    None => {
                        let listed = match download::list_files(&agent, HUB, repo, &wanted(m.id)) {
                            Ok(l) => l,
                            Err(e) => {
                                eprintln!("{e}");
                                return 1;
                            }
                        };
                        let listed = Model { files: Some(listed), ..m.clone() };
                        let st = standing(&listed, &n.cache);
                        listed.files.unwrap_or_default().into_iter().filter(|f| st.files.contains(&f.name)).collect()
                    }
                };
                files.into_iter().map(|f: FileSpec| {
                    let url = download::hub_url(HUB, repo, revision, &f.name);
                    (f, url)
                }).collect()
            }
        };
        for (f, url) in files {
            todo.push((m.clone(), f, url));
        }
    }
    if todo.is_empty() {
        println!("Every model asked for is on this computer already: nothing to download.");
        return 0;
    }
    let total: u64 = todo.iter().map(|(_, f, _)| f.bytes).sum();
    let mut names: Vec<&str> = todo.iter().map(|(m, _, _)| m.name.as_str()).collect();
    names.dedup();
    println!("Augmented View's server: downloading {} file{} of {} (about {}), into {}.", todo.len(), if todo.len() == 1 { "" } else { "s" }, names.join(", "), gb(total), n.cache.display());
    println!("Each file is checked against its published size and checksum. Ctrl+C stops; running the command again resumes.");
    let mut done_before = 0u64;
    for (i, (m, f, url)) in todo.iter().enumerate() {
        let what = format!("{} {}", m.name, f.name);
        let mut last = Instant::now() - Duration::from_secs(1);
        let mut on = |got: u64| {
            if last.elapsed() >= Duration::from_millis(500) || got == f.bytes {
                last = Instant::now();
                let pct = ((done_before + got) * 100).checked_div(total).unwrap_or(0);
                eprint!("\r[{}/{}] {what}: {} of {} ({pct}% in all)   ", i + 1, todo.len(), gb(got), if f.bytes > 0 { gb(f.bytes) } else { "?".into() });
                _ = std::io::stderr().flush();
            }
        };
        match download::fetch(&agent, url, &dest(m, &n.cache, &f.name), f, &CANCEL, &what, &mut on) {
            Ok(()) => eprintln!(),
            Err(e) if e == download::CANCELLED => {
                eprintln!("\nStopped. Run the same command again to resume.");
                return 130;
            }
            Err(e) => {
                eprintln!("\nThe download stopped: {e}");
                return 1;
            }
        }
        done_before += f.bytes;
    }
    println!("Done: every file is on this computer and checked.");
    if todo.iter().any(|(m, f, _)| m.id == "colours" && f.name.ends_with(".safetensors")) {
        println!("At its first start the server converts {} once into a 2.4 GB 4-bit file (about two minutes) and deletes the official weights it downloaded.", n.claims_model);
    }
    if then_serve {
        println!("Starting the server.");
    } else {
        println!("Start the server from Obsidian: Settings → Augmented View → Models → Check again (or click AugView in the status bar).");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn needs(cache: &Path) -> Needs {
        Needs { cache: cache.to_path_buf(), model: TERMS_REPO.into(), claims_model: CLAIMS_REPO.into(), claims: true, phrases: false }
    }

    /// The manifest: every default model pinned to a commit, each file with a size and a checksum, the weights by
    /// SHA-256; the sizes as the Hub listed them.
    #[test]
    fn the_manifest_pins_every_default_model() {
        let n = needs(Path::new("/nowhere"));
        let m = manifest(&n);
        assert_eq!(m.iter().map(|m| m.id).collect::<Vec<_>>(), IDS);
        for x in &m {
            let files = x.files.as_ref().expect("default models list their files");
            assert!(files.iter().all(|f| f.bytes > 0 && (f.sha256.is_some() || f.git_sha1.as_ref().is_some_and(|s| s.len() == 40))), "{}", x.name);
            assert!(files.iter().filter(|f| f.name.ends_with(".safetensors") || f.name.ends_with(".udpipe")).all(|f| f.sha256.as_ref().is_some_and(|s| s.len() == 64)));
            assert!(x.page.starts_with("https://") && x.licence_url.starts_with("https://") && !x.licence.is_empty());
            if let Source::Hub { revision, .. } = &x.source {
                assert_eq!(revision.len(), 40, "{} is pinned to a commit", x.name);
            }
        }
        let bytes = |id: &str| m.iter().find(|x| x.id == id).unwrap().files.as_ref().unwrap().iter().map(|f| f.bytes).sum::<u64>();
        assert_eq!((bytes("parser"), bytes("terms"), bytes("colours"), bytes("phrases")), (16309608, 438667685, 8056447577, 842589868));
        // a model set by hand: listed by name, its files left to the Hub's API, its licence to its page
        let other = manifest(&Needs { model: "someone/other".into(), ..n.clone() });
        assert!(other[1].files.is_none() && other[1].source == Source::Hub { repo: "someone/other".into(), revision: "main".into() } && other[1].licence_url.ends_with("someone/other"));
    }

    /// With nothing on disk every model this run needs is missing, with its files, size, page and folder; phrase
    /// colours only with --phrases, sentence colours not with --no-claims; nothing is written.
    #[test]
    fn missing_models_are_listed_not_downloaded() {
        let cache = std::env::temp_dir().join(format!("augmented-view-models-{}", std::process::id()));
        let n = Needs { model: "nobody/no-embedding".into(), claims_model: "nobody/no-instruct".into(), ..needs(&cache) };
        let m = missing(&n);
        let ids: Vec<&str> = m.iter().filter_map(|i| i["id"].as_str()).collect();
        assert_eq!(ids, ["parser", "terms", "colours"]);
        assert_eq!(m[0]["missing_bytes"], 16309608u64);
        assert_eq!(m[0]["files"][0], crate::PARSER);
        assert!(m[0]["from"] == "https://lindat.mff.cuni.cz" && m[1]["page"] == "https://huggingface.co/nobody/no-embedding");
        assert!(m[1]["folder"].as_str().unwrap().contains("nobody--no-embedding"));
        let with_phrases = missing(&Needs { phrases: true, claims: false, ..n.clone() });
        let ids: Vec<&str> = with_phrases.iter().filter_map(|i| i["id"].as_str()).collect();
        // GLiNER2 may be in this machine's Hugging Face cache already
        assert!(ids.starts_with(&["parser", "terms"]) && !ids.contains(&"colours"), "{ids:?}");
        assert!(need("nobody/no-embedding", "config.json").is_err_and(|e| e.to_string().contains(MISSING)));
        assert!(!cache.exists(), "nothing was written");
    }

    /// The instruct model once converted needs only its tokenizer; a `.part` makes a model partial; a file of the
    /// wrong size does not count.
    #[test]
    fn standing_follows_the_files_on_disk() {
        let cache = std::env::temp_dir().join(format!("augmented-view-standing-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&cache);
        std::fs::create_dir_all(&cache).unwrap();
        let n = Needs { claims_model: "nobody/instruct".into(), ..needs(&cache) };
        let parser = &manifest(&n)[0];
        std::fs::write(cache.join(crate::PARSER), b"too short").unwrap();
        assert_eq!(standing(parser, &cache).status, "missing");
        std::fs::write(download::part_of(&cache.join(crate::PARSER)), b"begun").unwrap();
        assert_eq!(standing(parser, &cache).status, "partial");
        // a converted model set by hand, with its tokenizer in the server's folder
        let colours = &manifest(&n)[2];
        assert!(!standing(colours, &cache).ready);
        std::fs::write(gguf(&cache, "nobody/instruct"), b"q4k").unwrap();
        for f in ["tokenizer.json", "tokenizer_config.json"] {
            let p = native_kit::models::ours(&cache, "nobody/instruct", f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"{}").unwrap();
        }
        assert!(standing(colours, &cache).ready);
        _ = std::fs::remove_dir_all(&cache);
    }

    /// `--download`'s ids: all is what this run needs; ids pick models; a wrong id is refused with the list.
    #[test]
    fn download_ids() {
        let n = needs(Path::new("/nowhere"));
        let ids = |s: &str| chosen(&n, s).map(|m| m.iter().map(|m| m.id).collect::<Vec<_>>());
        assert_eq!(ids("all").unwrap(), ["parser", "terms", "colours"]);
        assert_eq!(ids("").unwrap(), ["parser", "terms", "colours"]);
        assert_eq!(ids("phrases, terms").unwrap(), ["terms", "phrases"]);
        assert!(ids("everything").unwrap_err().contains("parser, terms, colours, phrases"));
    }

    /// The terminal's question is asked only when input, output and error output are all terminals and `--no-prompt`
    /// is not given: a start from the plugin (input ignored, output piped) is never asked.
    #[test]
    fn the_question_only_in_a_terminal() {
        assert!(may_ask(true, true, true, false), "a terminal the user opened");
        assert!(!may_ask(true, true, true, true), "--no-prompt");
        assert!(!may_ask(false, false, false, false), "the plugin: input ignored, output piped");
        assert!(!may_ask(false, true, true, false), "input not a terminal (a script, a pipe)");
        assert!(!may_ask(true, false, true, false), "output redirected to a file");
        assert!(!may_ask(true, true, false, false), "error output redirected");
    }

    /// Only "y" or "yes" (any case, spaces around) is a yes; an empty line, the end of the input or anything else is no.
    #[test]
    fn only_yes_downloads() {
        for yes in ["y", "Y", "yes", "YES", "Yes", " y \n", "yes\r\n"] {
            assert!(accepts(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "N", "yeah", "ye", "yes please", "y y", "1", "ok"] {
            assert!(!accepts(no), "{no:?}");
        }
    }

    /// The command to run later: this program quoted, `--download all`, then this run's own arguments (not
    /// `--no-prompt`); PowerShell's `&` on Windows.
    #[test]
    fn the_command_for_later() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(later_command("/opt/av/augmented-view-server", &args(&["--phrases", "--port", "9000"]), false), r#""/opt/av/augmented-view-server" --download all --phrases --port 9000"#);
        assert_eq!(later_command(r"D:\Apps\AV\augmented-view-server.exe", &args(&["--model", "some one/model", "--no-prompt"]), true), r#"& "D:\Apps\AV\augmented-view-server.exe" --download all --model "some one/model""#);
    }
}
