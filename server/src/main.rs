//! Augmented View's model server. Endpoints (JSON Lines, <https://jsonlines.org/>):
//!
//!   POST /emphasis  {"paras": [{"id", "text"}], "top_k": 12, "diversity": 0.4} -> one line per paragraph as it is done,
//!                   {"id", "spans": [{"k": "term"|"phrase", "t", "w", "v", "at": [[s, e], ...]}]}, then {"done": true}
//!   POST /colours   the same with "colours": [{"i", "name", "description"}]: each sentence's colour, kind "colour" with
//!                   "c" (the colour's "i") and "w" (its probability); a sentence that fits none is left out; with
//!                   "phrases": [...] (the same) also those colours' phrases (GLiNER2; "w" its confidence); with
//!                   "stress": true the contrast words
//!   GET  /health    {"ready", "protocol", "model", "claims", "claims_model", "device", "devices", "cuda", "notes"}
//!   GET  /stats     the GPU's and the computer's load and memory, and the server's own (native-kit's hw)
//!   POST /shutdown  stops the server
//!
//! Offsets are UTF-16 code units of the text sent; `v` is a span's visible text (markup blanked, spaces collapsed).
//! Each layer has its own thread holding its models, so terms never wait behind a whole request for points; their GPU
//! work takes turns (native-kit's hw). Each model goes on the GPU when there is one with room for it, else on the CPU, and every
//! such fallback is reported with its reason in `notes`. On start, when model files are missing, the server prints one
//! line, {"missing": [{"what", "from", "mb"}], "total_mb", "folder"}, and exits with code 4, unless it was started with
//! --download (the user agreed to the download in the plugin's dialog); then it prints `{"progress": ...}` lines while it
//! downloads them. Once the models are loaded it prints one line, {"ready": true, ...}; with --idle-minutes N it
//! exits after N minutes without a request, freeing the GPU. Browsers ask before cross-origin POSTs, so OPTIONS is
//! answered (<https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CORS#preflighted_requests>), for Obsidian's
//! origins and any given with --origin. HTTP: axum (<https://docs.rs/axum>).

mod claims;
mod embed;
mod meaning;
mod models;
mod phrases;
mod rules;
mod text;

use native_kit::hw; // the hardware: devices, readings, pacing (../../native-kit, shared with Quarry's service)

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc as tmpsc, Notify};

const PROTOCOL: u32 = 8; // 2: spans carry "v"; 3: UDPipe's parse; 4: quotations and definitions; 5: contrast words, no key phrases; 6: terms ranked for specificity, quoted words as terms; 7: acronyms, lists drawn whole, named terms weigh 2; 8: /colours. The plugin keys its cache by it, so
                         // results from another server are never mixed in
/// Obsidian's own origins: desktop, iOS, Android (<https://capacitorjs.com/docs/config>, `server.hostname`).
const ORIGINS: [&str; 3] = ["app://obsidian.md", "capacitor://localhost", "http://localhost"];
pub const PARSER: &str = "english-ewt-ud-2.5-191206.udpipe"; // UD 2.5 models, CC BY-NC-SA 4.0 (https://hdl.handle.net/11234/1-3131)
const PARSER_URL: &str = "https://lindat.mff.cuni.cz/repository/server/api/core/bitstreams/8d51e3d3-1f87-47fe-8975-c197ddf7907b/content";
/// About how much memory each layer's models take (measured: README, "GPU memory"), on the GPU and on the CPU.
const TERMS_MB: (u64, u64) = (600, 1200);
const POINTS_MB: (u64, u64) = (3500, 4000);

#[derive(Deserialize)]
struct Para {
    id: String,
    text: String,
}

#[derive(Deserialize)]
struct Req {
    paras: Vec<Para>,
    #[serde(default = "top_k")]
    top_k: usize,
    #[serde(default = "diversity")]
    diversity: f32,
    /// /colours: the colours to classify the sentences by.
    #[serde(default)]
    colours: Vec<claims::Category>,
    /// /colours: the phrase colours, found by GLiNER2 (phrases.rs).
    #[serde(default)]
    phrases: Vec<claims::Category>,
    /// /colours: also the contrast words.
    #[serde(default)]
    stress: bool,
}
fn top_k() -> usize {
    12
}
fn diversity() -> f32 {
    0.4
}

type Line = Result<String, std::convert::Infallible>;
struct Job(Req, tmpsc::Sender<Line>);

struct App {
    fast: mpsc::Sender<Job>,
    claims: Option<mpsc::Sender<Job>>,
    model: String,
    claims_model: String,
    devices: Value, // {"terms": "CUDA, …", "points": "CPU"}
    cuda: bool,
    start: Instant,
    last: AtomicU64, // seconds since start of the last request
    stop: Notify,
}

impl App {
    fn touch(&self) {
        self.last.store(self.start.elapsed().as_secs(), Ordering::Relaxed);
    }

    /// What /health and the ready line report: the models, where each runs, and why anything is not on the GPU.
    fn info(&self) -> Value {
        json!({ "ready": true, "protocol": PROTOCOL, "model": self.model, "claims": self.claims.is_some(),
            "claims_model": self.claims_model, "device": self.devices["terms"], "devices": self.devices, "cuda": self.cuda,
            "notes": hw::notes(), "pid": std::process::id(), "recommended": recommended() })
    }
}

/// The settings that suit this computer (the plugin's Optimize For Current Hardware, user 2026-10-27): looked at once.
fn recommended() -> Value {
    static R: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let h = hw::probe();
        let r = hw::recommend(&h);
        json!({ "hardware": h.summary(), "settings": { "useGpu": r.use_gpu, "contextParagraphs": r.context_paragraphs } })
    })
    .clone()
}

struct Args {
    port: u16,
    model: String,
    claims_model: String,
    no_claims: bool,
    idle_minutes: f64,
    cache: PathBuf,
    use_gpu: bool,
    /// When the GPU has no room for the models: "ask" stops at once with a `gpu_shortage` line for the plugin to ask
    /// the user, "free" has the user's other services free their GPU memory first, "" places them where they fit.
    gpu: String,
    gpu_limit: u32,
    origins: Vec<String>,
    /// The user chose to download the model files this computer lacks (the plugin's dialog, which showed their sizes).
    download: bool,
    /// The note's colour codes have phrase colours, so GLiNER2 is needed too (checked at start with the others).
    phrases: bool,
}

fn args() -> Args {
    let mut a = Args {
        port: 8790,
        model: "BAAI/bge-base-en-v1.5".into(),
        claims_model: "Qwen/Qwen3-4B-Instruct-2507".into(),
        no_claims: false,
        idle_minutes: 0.0,
        cache: models::default_cache(),
        use_gpu: true,
        gpu: String::new(),
        gpu_limit: 100,
        origins: ORIGINS.iter().map(|o| o.to_string()).collect(),
        download: false,
        phrases: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_default();
        match k.as_str() {
            "--port" => a.port = v().parse().unwrap_or(a.port),
            "--model" => a.model = v(),
            "--claims-model" => a.claims_model = v(),
            "--no-claims" => a.no_claims = true,
            "--idle-minutes" => a.idle_minutes = v().parse().unwrap_or(0.0),
            "--cache" => a.cache = v().into(),
            "--device" => a.use_gpu = v() != "cpu", // auto (the GPU when it can) or cpu
            "--gpu-limit" => a.gpu_limit = v().parse().unwrap_or(100),
            "--gpu" => a.gpu = v(),
            "--origin" => a.origins.push(v()),
            "--download" => a.download = true,
            "--phrases" => a.phrases = true,
            _ => eprintln!("unknown argument {k}"),
        }
    }
    a
}

static PARSER_PATH: OnceLock<PathBuf> = OnceLock::new();
static ALLOWED: OnceLock<Vec<String>> = OnceLock::new();

thread_local! {
    static UDPIPE: RefCell<Option<udpipe_rs::Model>> = const { RefCell::new(None) };
}

/// Parse on the calling thread with that thread's own UDPipe model (a model may move between threads but not be shared,
/// so each worker of rayon's pool, <https://docs.rs/rayon>, loads one the first time), from one copy of the file. The
/// first model is loaded alone (main): when all threads loaded their first at once, one occasionally failed.
fn parse(text: &str) -> text::Doc {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    UDPIPE.with(|m| {
        let mut m = m.borrow_mut();
        let model = m.get_or_insert_with(|| {
            let bytes = BYTES.get_or_init(|| std::fs::read(PARSER_PATH.get().expect("parser path")).unwrap_or_else(|e| fail(format!("UDPipe model: {e}"))));
            udpipe_rs::Model::load_from_memory(bytes).unwrap_or_else(|e| fail(format!("UDPipe model: {}", e.message)))
        });
        text::Doc::parse(text, model)
    })
}

/// A layer's thread: loads its models (`load` returns them and the device they went on), then answers jobs. A job's
/// paragraphs are parsed in parallel on the CPU while the models work through them in order, each line sent as soon as
/// it is done; a client that has gone stops the job. Returns once the models are loaded, or why they could not be.
fn layer<M: 'static>(
    name: &'static str,
    load: impl FnOnce() -> Result<(M, String), String> + Send + 'static,
    work: fn(&mut M, &text::Doc, &Req) -> candle_core::Result<Vec<Value>>,
) -> Result<(mpsc::Sender<Job>, String), String> {
    let (tx, rx) = mpsc::channel::<Job>();
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut model = match load() {
            Ok((m, device)) => {
                let _ = ready_tx.send(Ok(device));
                m
            }
            Err(e) => {
                let _ = ready_tx.send(Err(format!("{name}: {e}")));
                return;
            }
        };
        for Job(req, out) in rx {
            let t0 = Instant::now();
            let (dtx, drx) = mpsc::channel::<(usize, text::Doc)>();
            for (i, p) in req.paras.iter().enumerate() {
                let (dtx, text) = (dtx.clone(), p.text.clone());
                rayon::spawn(move || _ = dtx.send((i, parse(&text))));
            }
            drop(dtx);
            let (mut parsed, mut next) = (BTreeMap::new(), 0);
            'job: for (i, d) in drx {
                parsed.insert(i, d);
                while let Some(d) = parsed.remove(&next) {
                    let spans = work(&mut model, &d, &req).unwrap_or_else(|e| {
                        eprintln!("{name}: {e}");
                        Vec::new()
                    });
                    if out.blocking_send(Ok(json!({ "id": req.paras[next].id, "spans": spans }).to_string() + "\n")).is_err() {
                        break 'job;
                    }
                    next += 1;
                }
            }
            let _ = out.blocking_send(Ok(json!({ "done": true, "ms": t0.elapsed().as_millis() as u64 }).to_string() + "\n"));
        }
    });
    let device = ready_rx.recv().map_err(|_| format!("{name}: the model thread stopped"))??;
    Ok((tx, device))
}

/// CORS headers for the request's origin when it is allowed (Obsidian's by default), else for desktop Obsidian's.
async fn cors(req: Request, next: Next) -> Response {
    let origin = req.headers().get(header::ORIGIN).cloned();
    let allowed = ALLOWED.get().map(Vec::as_slice).unwrap_or_default();
    // any web page open in a browser on this PC can send a simple POST to 127.0.0.1 without asking first (CORS only
    // hides the answer), and /shutdown and /gpu/release need no body: a request that names a browser origin acts only
    // if it is Obsidian's or one allowed (user, 2026-10-27: stopping services is locked down). Callers without an
    // Origin (Obsidian's Node requests, the MCP server, the Ignis relay, which checks the origin itself) pass.
    if req.method() != Method::GET && req.method() != Method::OPTIONS && origin.as_ref().is_some_and(|o| !allowed.iter().any(|a| o.as_bytes() == a.as_bytes())) {
        return (StatusCode::FORBIDDEN, "requests from this web origin are refused").into_response();
    }
    let mut r = next.run(req).await;
    let origin = origin.filter(|o| allowed.iter().any(|a| o.as_bytes() == a.as_bytes())).unwrap_or(HeaderValue::from_static(ORIGINS[0]));
    let h = r.headers_mut();
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST"));
    h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("Content-Type"));
    r
}

fn stream(app: &App, tx: Option<&mpsc::Sender<Job>>, req: Req) -> Response {
    app.touch();
    let (out, rx) = tmpsc::channel(64);
    match tx.map(|t| t.send(Job(req, out))) {
        Some(Ok(())) => Response::builder().header(header::CONTENT_TYPE, "application/x-ndjson")
            .body(Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx))).unwrap_or_default(),
        _ => (StatusCode::NOT_FOUND, "running without sentence colours").into_response(),
    }
}

/// The default models' download sizes in MB (decimal, from Hugging Face's file listings), for the dialog that asks the
/// user before anything is downloaded. A model set by hand has no known size (0).
const SIZES_MB: [(&str, u64); 3] = [("BAAI/bge-base-en-v1.5", 440), ("Qwen/Qwen3-4B-Instruct-2507", 8070), (phrases::REPO, 845)];
const PARSER_MB: u64 = 16;

fn size_mb(repo: &str) -> u64 {
    SIZES_MB.iter().find(|(r, _)| *r == repo).map_or(0, |(_, mb)| *mb)
}

/// What this run needs and does not have on disk, each with where it comes from and its download size: nothing is
/// downloaded until the user has seen this list and agreed (`--download`).
fn missing(a: &Args) -> Vec<Value> {
    let mut out = Vec::new();
    if !a.cache.join(PARSER).exists() {
        out.push(json!({ "what": "UDPipe English parser (LINDAT, CC BY-NC-SA 4.0)", "from": "lindat.mff.cuni.cz", "mb": PARSER_MB }));
    }
    if ["config.json", "model.safetensors", "tokenizer.json"].iter().any(|f| !models::have(&a.model, f)) {
        out.push(json!({ "what": format!("{} (key terms)", a.model), "from": "huggingface.co", "mb": size_mb(&a.model) }));
    }
    if !a.no_claims {
        let gguf = a.cache.join(format!("{}-q4k.gguf", a.claims_model.replace('/', "--")));
        let tokenizer = ["tokenizer.json", "tokenizer_config.json"].iter().all(|f| models::have(&a.claims_model, f));
        // the official weights already in a cache (the Hugging Face one) are converted without downloading anything
        let official = models::local(&a.claims_model, "model.safetensors.index.json")
            .and_then(|p| serde_json::from_str::<Value>(&std::fs::read_to_string(p).ok()?).ok())
            .is_some_and(|index| index["weight_map"].as_object().into_iter().flatten().all(|(_, f)| f.as_str().is_some_and(|f| models::have(&a.claims_model, f))));
        if !gguf.exists() && !(official && tokenizer && models::have(&a.claims_model, "config.json")) {
            let what = format!("{} (sentence colours; converted once to a 2.4 GB 4-bit file, and the download deleted)", a.claims_model);
            out.push(json!({ "what": what, "from": "huggingface.co", "mb": size_mb(&a.claims_model) }));
        } else if !tokenizer {
            out.push(json!({ "what": format!("{} tokenizer", a.claims_model), "from": "huggingface.co", "mb": 12 }));
        }
    }
    if a.phrases && phrases::FILES.iter().any(|f| !models::have(phrases::REPO, f)) {
        out.push(json!({ "what": format!("{} (phrase colours)", phrases::REPO), "from": "huggingface.co", "mb": size_mb(phrases::REPO) }));
    }
    out
}

/// Stop with a message the plugin shows (its last line of stderr).
fn fail(msg: String) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

fn main() {
    let a = args();
    hw::set_limit(a.gpu_limit);
    _ = models::CACHE.set(a.cache.clone());
    _ = ALLOWED.set(a.origins.clone());
    models::DOWNLOAD.store(a.download, Ordering::Relaxed);
    // Model files this computer lacks are downloaded only when the user chose to: otherwise the server lists them, with
    // their sizes, for the plugin to ask (exit 4), and stops.
    let lacking = missing(&a);
    if !lacking.is_empty() && !a.download {
        let total: u64 = lacking.iter().filter_map(|m| m["mb"].as_u64()).sum();
        println!("{}", json!({ "missing": lacking, "total_mb": total, "folder": a.cache }));
        eprintln!("the models are not downloaded yet (about {total} MB): start the server with --download to download them into {}", a.cache.display());
        std::process::exit(4);
    }
    let parser = a.cache.join(PARSER);
    if !parser.exists() {
        if let Err(e) = models::download(PARSER_URL, &parser, "the UDPipe English model") {
            fail(format!("{e} (or download {PARSER} from https://hdl.handle.net/11234/1-3131 into {})", a.cache.display()));
        }
    }
    PARSER_PATH.set(parser).expect("set once");
    // One parser alone first, then every worker's in parallel, now rather than on its first request (and the server
    // stops here, with the reason, if the model is unreadable).
    let first = "A first sentence loads the parser on this thread.";
    parse(first);
    rayon::broadcast(|_| _ = parse(first));

    // room on the GPU (native_kit::hw::claims): the plugin asks the user before anything else is stopped
    let mut use_gpu = a.use_gpu;
    let need = TERMS_MB.0 + if a.no_claims { 0 } else { POINTS_MB.0 };
    if use_gpu {
        // decided as every GPU service of the workspace decides (native_kit::hw::claims::room); "" places them where
        // they fit, as before
        let mode = if a.gpu.is_empty() { "place" } else { a.gpu.as_str() };
        match hw::claims::room("augmented-view", "the emphasis models", need, mode) {
            hw::claims::Room::Ask(short) if mode == "ask" => {
                println!("{}", json!({ "gpu_shortage": short }));
                std::process::exit(3);
            }
            hw::claims::Room::Cpu => use_gpu = false,
            _ => {}
        }
    }
    let model = a.model.clone();
    let (fast, terms_on) = layer("key terms", move || {
        let (dev, on) = hw::place("key terms", use_gpu, TERMS_MB.0, TERMS_MB.1)?;
        Ok((embed::Embedder::load(&model, &dev).map_err(|e| e.to_string())?, on))
    }, |m, d, r| rules::rank(d, m, r.top_k, r.diversity))
    .unwrap_or_else(|e| fail(e));
    let claims = if a.no_claims {
        None
    } else {
        let (model, cache) = (a.claims_model.clone(), a.cache.clone());
        layer("sentence colours", move || {
            let (dev, on) = hw::place("sentence colours", use_gpu, POINTS_MB.0, POINTS_MB.1)?;
            Ok((claims::Claims::load(&model, &cache, &dev).map_err(|e| e.to_string())?, on))
        }, |m, d, r| m.colour(d, &r.colours, &r.phrases, r.stress))
        .map_err(|e| hw::note(format!("{e}; running without sentence colours")))
        .ok()
    };
    let mut devices = json!({ "terms": terms_on });
    if let Some((_, on)) = &claims {
        devices["points"] = json!(on);
    }
    let cuda = [&devices["terms"], &devices["points"]].iter().any(|d| d.as_str().is_some_and(|s| s.starts_with("CUDA")));
    let on = |d: &Value| d.as_str().is_some_and(|s| s.starts_with("CUDA"));
    let held = if on(&devices["terms"]) { TERMS_MB.0 } else { 0 } + if on(&devices["points"]) { POINTS_MB.0 } else { 0 };
    if held > 0 {
        hw::claims::hold("augmented-view", a.port, held, "/gpu/release");
    }
    let app = Arc::new(App {
        fast,
        claims_model: if claims.is_some() { a.claims_model.clone() } else { String::new() },
        claims: claims.map(|(tx, _)| tx),
        model: a.model.clone(),
        devices,
        cuda,
        start: Instant::now(),
        last: AtomicU64::new(0),
        stop: Notify::new(),
    });

    let router = Router::new()
        .route("/health", get(|State(app): State<Arc<App>>| async move { Json(app.info()) }))
        .route("/stats", get(|State(app): State<Arc<App>>| async move {
            let mut s = tokio::task::spawn_blocking(move || hw::stats(app.cuda)).await.unwrap_or_default();
            s["notes"] = json!(hw::notes());
            Json(s)
        }))
        .route("/emphasis", post(|State(app): State<Arc<App>>, Json(req): Json<Req>| async move { stream(&app, Some(&app.fast), req) })
            .options(|| async { StatusCode::NO_CONTENT }))
        .route("/colours", post(|State(app): State<Arc<App>>, Json(req): Json<Req>| async move { stream(&app, app.claims.as_ref(), req) })
            .options(|| async { StatusCode::NO_CONTENT }))
        .route("/gpu/release", post(|State(app): State<Arc<App>>| async move {
            // another of the user's services needs the GPU: this server stops, and says why
            println!("{}", json!({ "stopping": "its GPU memory was freed for other work; start it again when you need it" }));
            hw::claims::drop_claim("augmented-view");
            app.stop.notify_one();
            Json(json!({ "stopping": true }))
        }))
        .route("/shutdown", post(|State(app): State<Arc<App>>| async move {
            app.stop.notify_one();
            Json(json!({ "stopping": true }))
        }))
        .layer(axum::middleware::from_fn(cors))
        .with_state(app.clone());

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    rt.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", a.port)).await {
            Ok(l) => l,
            Err(e) => fail(format!("port {}: {e}", a.port)),
        };
        let mut ready = app.info();
        ready["port"] = json!(a.port);
        println!("{ready}");
        if a.idle_minutes > 0.0 {
            let app = app.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    if (app.start.elapsed().as_secs() - app.last.load(Ordering::Relaxed)) as f64 > 60.0 * a.idle_minutes {
                        println!("{}", json!({ "stopping": "idle" }));
                        app.stop.notify_one();
                    }
                }
            });
        }
        let stop = app.clone();
        let _ = axum::serve(listener, router).with_graceful_shutdown(async move { stop.stop.notified().await }).await;
    });
    hw::claims::drop_claim("augmented-view");
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing is fetched without --download: what is missing is listed, with sizes where they are known.
    #[test]
    fn missing_models_are_listed_not_downloaded() {
        let cache = std::env::temp_dir().join(format!("augmented-view-test-{}", std::process::id()));
        let mut a = args();
        (a.cache, a.model, a.claims_model) = (cache.clone(), "nobody/no-embedding".into(), "nobody/no-instruct".into());
        let m = missing(&a);
        let whats: Vec<&str> = m.iter().filter_map(|i| i["what"].as_str()).collect();
        assert!(whats[0].starts_with("UDPipe"), "{whats:?}");
        assert!(whats.iter().any(|w| w.starts_with("nobody/no-embedding")) && whats.iter().any(|w| w.starts_with("nobody/no-instruct")), "{whats:?}");
        assert!(m.iter().filter(|i| i["what"].as_str().unwrap_or("").starts_with("nobody")).all(|i| i["mb"] == 0), "unknown models have no size");
        a.no_claims = true;
        assert!(!missing(&a).iter().any(|i| i["what"].as_str().unwrap_or("").starts_with("nobody/no-instruct")));
        assert!(!models::DOWNLOAD.load(Ordering::Relaxed));
        assert!(models::need("nobody/no-embedding", "config.json").is_err_and(|e| e.to_string().contains(models::MISSING)));
        assert!(!cache.exists(), "nothing was written");
    }
}
