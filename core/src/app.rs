//! The plugin on Obsidian's main thread: settings, status bar, menu, commands, the settings tab, the model server's
//! life cycle, and the bridge to the worker (fetch and the vault's file I/O, which only the main thread may do). It
//! never loops over a note's text; that is the worker's.
//!
//! Obsidian's API: <https://docs.obsidian.md/Reference/TypeScript+API/Plugin> (the plugin object main.js passes in) and the guides at
//! <https://docs.obsidian.md/Plugins/Getting+started/Build+a+plugin>.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use js_sys::{Array, Function, Uint8Array, JSON};
use wasm_bindgen::prelude::*;

use crate::editor::Ed;
use obsidian_kit::settings::{Ctl, Row};

use crate::js::{self, call, f0, f1, f2, f3, get, new, obj, set, str};
use crate::msg::{self, FromWorker, Local, Settings, ToWorker};
use crate::view::Reading;

const LOCAL_KEY: &str = "augmented-view"; // this device's settings (msg::Local)
const CANNOT_START: &str = "this device cannot start the server: set Server address (Settings, This device) to one running elsewhere";
const STATS_MS: f64 = 2000.0;
/// Windows' "a DLL was not found" exit status (0xC0000135), and the shell's "cannot run" (Linux, macOS): the server could
/// not start at all, so the CPU build is tried.
const CANNOT_RUN: [f64; 3] = [-1073741515.0, 3221225781.0, 127.0];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

pub struct App {
    pub plugin: JsValue,
    pub obs: JsValue,
    pub cmv: JsValue,
    pub cms: JsValue,
    require: Function,
    pub(crate) vault: JsValue,
    pub(crate) adapter: JsValue,
    dir: String,
    can_spawn: bool,
    pub settings: Settings,
    pub(crate) local: Local,
    worker: JsValue,
    state: State,
    detail: String,
    model: String,
    claims_model: String,
    has_claims: bool,
    /// Where the models run ("CUDA, …", "CPU"), whether CUDA is used, and why anything fell back (server and plugin).
    device: String,
    cuda: bool,
    notes: Vec<String>,
    fallback: Option<String>,
    /// The latest readings (/stats): the status bar's tooltip, and the warnings in force.
    stats: String,
    warned: HashSet<&'static str>,
    hot: u32,
    polling: bool,
    noticed: f64,
    child: Option<JsValue>,
    /// What the next server does when the GPU has no room ("ask", or the user's answer "free" or "cpu"), and the
    /// shortage the last one reported (native_kit::hw::claims, through its `gpu_shortage` line).
    gpu_mode: String,
    shortage: Option<serde_json::Value>,
    /// The models the last server lacked (its `missing` line, followed by exit 4): the plugin then says which and shows
    /// the Models section (models.rs). It never downloads them, nor starts the server's download.
    missing: Option<serde_json::Value>,
    told: String,
    status: JsValue,
    pub editors: HashMap<u32, Ed>,
    pub reading: Reading,
    pub marks: Vec<JsValue>,
    pub refresh: JsValue,
    docs: HashSet<String>,
    version: u32,
    pub next_id: u32,
    aborts: HashMap<u32, JsValue>,
    persist: Option<f64>,
    /// The settings tab, redrawn when a colour or code is added or removed.
    tab: JsValue,
    /// The floating key's element (key.rs), and whether choosing a code there links the note's folder.
    pub(crate) key: JsValue,
    pub(crate) key_folder: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Run `f` on the app. Never call into JavaScript that may call back into the plugin synchronously (an editor
/// dispatch) from inside `f`: return what is needed and make that call after.
pub fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

#[wasm_bindgen]
pub struct Handle;

#[wasm_bindgen]
impl Handle {
    pub fn unload(&self) {
        let child = with(|a| {
            a.aborts.values().for_each(|c| _ = call!(c, abort));
            call!(a.worker, terminate);
            a.reading.clear();
            a.drop_key();
            let style = call!(get(&js::global(), "document"), getElementById, STYLE_ID);
            if !style.is_null() {
                call!(style, remove);
            }
            a.child.take()
        });
        if let Some(Some(c)) = child {
            call!(c, kill); // a server this plugin started; an adopted one keeps running
        }
        APP.with(|a| a.borrow_mut().take());
    }
}

/// Called by main.js: the plugin object, the `obsidian` module, `require`, the compiled module and its bindings.
#[wasm_bindgen]
pub fn start(plugin: JsValue, obs: JsValue, require: Function, module: JsValue, glue: String) -> Handle {
    let req = |m: &str| require.call1(&JsValue::NULL, &m.into()).unwrap_or(JsValue::UNDEFINED);
    let (cmv, cms) = (req("@codemirror/view"), req("@codemirror/state"));
    let vault = get(&get(&plugin, "app"), "vault");
    let decoration = get(&cmv, "Decoration");
    // Starting a process needs Node's child_process and a vault on disk (desktop Obsidian); a browser or phone has neither.
    let spawn = get(&req("child_process"), "spawn");
    let local = call!(get(&plugin, "app"), loadLocalStorage, LOCAL_KEY);
    let app = App {
        adapter: get(&vault, "adapter"),
        dir: str(&get(&get(&plugin, "manifest"), "dir")),
        // desktop Obsidian only: Ignis has a child_process stand-in, which must not be asked to run a program
        can_spawn: js::desktop() && spawn.is_function() && get(&get(&vault, "adapter"), "getBasePath").is_function(),
        local: serde_wasm_bindgen::from_value(local).unwrap_or_default(),
        device: String::new(),
        cuda: false,
        notes: Vec::new(),
        fallback: None,
        stats: String::new(),
        warned: HashSet::new(),
        hot: 0,
        polling: false,
        noticed: 0.0,
        worker: spawn_worker(&glue, &module),
        marks: (0..crate::pick::KINDS as u8).map(|k| call!(decoration, mark, obj! { class: format!("av {}", crate::pick::class(k)) })).collect(),
        refresh: call!(get(&cms, "StateEffect"), define),
        reading: Reading::new(),
        settings: Settings::default(),
        state: State::Stopped,
        detail: String::new(),
        model: String::new(),
        claims_model: String::new(),
        has_claims: false,
        child: None,
        gpu_mode: "ask".into(),
        shortage: None,
        missing: None,
        told: String::new(),
        status: JsValue::UNDEFINED,
        editors: HashMap::new(),
        docs: HashSet::new(),
        version: 0,
        next_id: 1,
        aborts: HashMap::new(),
        persist: None,
        tab: JsValue::UNDEFINED,
        key: JsValue::UNDEFINED,
        key_folder: false,
        plugin,
        obs,
        cmv,
        cms,
        require,
        vault,
    };
    APP.with(|a| *a.borrow_mut() = Some(app));
    js::spawn(async {
        let Some(plugin) = with(|a| a.plugin.clone()) else { return };
        let data = js::wait(call!(plugin, loadData)).await.unwrap_or(JsValue::NULL);
        let settings = fixed(serde_wasm_bindgen::from_value::<Settings>(data).unwrap_or_default());
        with(|a| {
            a.settings = settings;
            // executables named in an older data.json move to this device once (desktop only, which has them)
            if js::desktop() && (!a.settings.server.is_empty() || !a.settings.cpu_server.is_empty()) {
                if a.local.server.is_empty() {
                    a.local.server = std::mem::take(&mut a.settings.server);
                }
                if a.local.cpu_server.is_empty() {
                    a.local.cpu_server = std::mem::take(&mut a.settings.cpu_server);
                }
                a.settings.server.clear();
                a.settings.cpu_server.clear();
                a.save_local();
                a.persist_soon();
            }
            crate::colours::ensure_ids(&mut a.settings.colour_codes);
            a.post(&ToWorker::Settings(a.settings.clone()));
            a.apply_colours();
            a.register();
            a.make_key();
            a.server_changed();
        });
    });
    Handle
}

/// The worker: the same module and bindings, started from a Blob, so nothing but main.js and the .wasm is shipped.
/// `Worker` (<https://developer.mozilla.org/en-US/docs/Web/API/Worker/Worker>), `Blob` and `URL.createObjectURL` (<https://developer.mozilla.org/en-US/docs/Web/API/URL/createObjectURL_static>);
/// a compiled `WebAssembly.Module` can be posted to a worker
/// (<https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/Module>).
fn spawn_worker(glue: &str, module: &JsValue) -> JsValue {
    let src = format!("{glue}\nself.onmessage = e => {{ self.onmessage = null; wasm_bindgen.initSync({{ module: e.data }}); wasm_bindgen.worker_start(); }};");
    let opts = js::empty();
    set(&opts, "type", "text/javascript");
    let blob = new(&get(&js::global(), "Blob"), &[Array::of1(&src.into()).into(), opts]);
    let url = call!(get(&js::global(), "URL"), createObjectURL, blob);
    let worker = new(&get(&js::global(), "Worker"), &[url]);
    call!(worker, postMessage, module.clone());
    set(&worker, "onmessage", f1(|e| {
        on_worker(&get(&e, "data"));
        JsValue::UNDEFINED
    }));
    worker
}

fn on_worker(data: &JsValue) {
    let Some(m) = msg::decode::<FromWorker>(&Uint8Array::new(data).to_vec()) else { return };
    match m {
        FromWorker::Marks { view, version, marks } => crate::editor::apply(view, version, &marks),
        FromWorker::Wrap { block, segs } => _ = with(|a| a.reading.queue(block, segs)),
        FromWorker::Fetch { req, endpoint, body, paras } => js::spawn(fetch(req, endpoint, body, paras)),
        FromWorker::Load { path, file } => js::spawn(load(path, file)),
        FromWorker::Save { file, bytes } => js::spawn(save(file, bytes)),
        FromWorker::RenameFile { from, to } => js::spawn(async move {
            let Some((ad, from, to)) = with(|a| (a.adapter.clone(), a.cache_path(&from), a.cache_path(&to))) else { return };
            if exists(&ad, &from).await {
                _ = js::wait(call!(ad, rename, from, to)).await;
            }
        }),
    }
}

/// The vault's file access: <https://docs.obsidian.md/Reference/TypeScript+API/DataAdapter> (exists, readBinary, writeBinary, mkdir, list, remove, rename).
async fn exists(adapter: &JsValue, path: &str) -> bool {
    js::wait(call!(adapter, exists, path)).await.is_ok_and(|v| v.is_truthy())
}

/// Send one planned request and stream its answer to the worker as it arrives: `fetch` with a readable body
/// (<https://developer.mozilla.org/en-US/docs/Web/API/Streams_API/Using_readable_streams>), decoded in pieces (<https://developer.mozilla.org/en-US/docs/Web/API/TextDecoder/decode>, `stream`),
/// cancelled through an `AbortController` (<https://developer.mozilla.org/en-US/docs/Web/API/AbortController>).
async fn fetch(req: u32, endpoint: String, body: String, paras: u32) {
    let ctl = new(&get(&js::global(), "AbortController"), &[]);
    let Some(url) = with(|a| {
        a.aborts.insert(req, ctl.clone());
        format!("{}{endpoint}", a.base())
    }) else { return };
    let headers = js::empty();
    set(&headers, "Content-Type", "application/json");
    let init = obj! { method: "POST", headers: headers, body: body, signal: get(&ctl, "signal") };
    let t0 = js::now();
    let result: Result<(), JsValue> = async {
        let res = js::wait(call!(js::global(), fetch, url, init)).await?;
        if !get(&res, "ok").is_truthy() {
            return Err(get(&res, "status"));
        }
        let reader = call!(get(&res, "body"), getReader);
        let decoder = new(&get(&js::global(), "TextDecoder"), &[]);
        loop {
            let r = js::wait(call!(reader, read)).await?;
            if get(&r, "done").is_truthy() {
                return Ok(());
            }
            let text = str(&call!(decoder, decode, get(&r, "value"), obj! { stream: true }));
            with(|a| a.post(&ToWorker::Chunk { req, text }));
        }
    }
    .await;
    let aborted = get(&get(&ctl, "signal"), "aborted").is_truthy();
    with(|a| {
        a.aborts.remove(&req);
        a.post(&ToWorker::End { req, ok: result.is_ok() });
        a.log(&format!("{endpoint} {paras} paragraphs: {:?} in {:.0} ms", result.as_ref().map_err(|e| e.as_string()), js::now() - t0));
    });
    if result.is_err() && !aborted {
        check().await;
    }
}

async fn load(path: String, file: String) {
    let Some((ad, full)) = with(|a| (a.adapter.clone(), a.cache_path(&file))) else { return };
    let mut bytes = None;
    if exists(&ad, &full).await {
        bytes = js::wait(call!(ad, readBinary, full)).await.ok().map(|b| Uint8Array::new(&b).to_vec());
    }
    with(|a| a.post(&ToWorker::Loaded { path, bytes }));
}

async fn save(file: String, bytes: Vec<u8>) {
    let Some((ad, dir, full)) = with(|a| (a.adapter.clone(), a.cache_dir(), a.cache_path(&file))) else { return };
    if !exists(&ad, &dir).await {
        _ = js::wait(call!(ad, mkdir, dir)).await;
    }
    _ = js::wait(call!(ad, writeBinary, full, Uint8Array::from(&bytes[..]).buffer())).await; // replaces the note's previous file
}

struct Info {
    model: String,
    claims: bool,
    claims_model: String,
    devices: String,
    cuda: bool,
    notes: Vec<String>,
    pid: f64,
}

/// The server's models, with its protocol in their names: results from a server that spoke another protocol (one
/// without the visible text Reading view needs) are then kept apart in the cache, never mixed in. Also where the
/// models run and why anything fell back from the GPU.
fn info(j: &JsValue) -> Info {
    let p = get(j, "protocol").as_f64().unwrap_or(1.0);
    let name = |k: &str| Some(str(&get(j, k))).filter(|m| !m.is_empty()).map(|m| format!("{m}#p{p}")).unwrap_or_default();
    let devs = get(j, "devices");
    let on = |k: &str, what: &str| Some(str(&get(&devs, k))).filter(|d| !d.is_empty()).map(|d| format!("{what}: {d}"));
    let devices = [on("terms", "terms"), on("points", "points")].into_iter().flatten().collect::<Vec<_>>().join("; ");
    Info {
        model: name("model"),
        claims: get(j, "claims").is_truthy(),
        claims_model: name("claims_model"),
        devices: if devices.is_empty() { str(&get(j, "device")) } else { devices },
        cuda: get(j, "cuda").is_truthy(),
        notes: get(j, "notes").dyn_into::<Array>().map(|a| a.iter().map(|n| str(&n)).collect()).unwrap_or_default(), // absent from older servers
        pid: get(j, "pid").as_f64().unwrap_or(0.0),
    }
}

thread_local! {
    static OPTIMIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The notes already asked about this session, so a note is asked about once.
    static ASKED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// When emphasis is on and the note just opened has no colour code, ask which code it takes (or none): for the note,
/// or for its folder. Asked once per note in a session; the floating key can change it later.
async fn ask_code() {
    let Some((path, codes, obs, app)) = with(|a| {
        let path = a.active_note()?;
        let unset = a.settings.enabled && a.state == State::Running && a.assigned(&path) == crate::colours::Assigned::Unset && !a.settings.colour_codes.is_empty();
        unset.then(|| (path, a.settings.colour_codes.iter().map(|c| (c.id.clone(), c.name.clone())).collect::<Vec<_>>(), a.obs.clone(), get(&a.plugin, "app")))
    }).flatten() else { return };
    if !ASKED.with(|s| s.borrow_mut().insert(path.clone())) {
        return;
    }
    let folder = crate::colours::folder_of(&path);
    let scope = if folder == "/" { "the whole vault".to_string() } else { folder.clone() };
    let mut labels: Vec<String> = codes.iter().map(|(_, n)| format!("{n}: this note")).collect();
    labels.extend(codes.iter().map(|(_, n)| format!("{n}: every note in {scope}")));
    labels.push("None: leave this note unmarked".into());
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let msg = "Augmented View is on, but this note has no colour code, so nothing is highlighted. Which code should it use?";
    let Some(i) = js::choose(&obs, &app, "Colour code for this note", msg, &refs).await else { return };
    let n = codes.len();
    with(|a| match i {
        i if i < n => a.set_link(&path, &codes[i].0, false),
        i if i < 2 * n => a.set_link(&path, &codes[i - n].0, true),
        _ => a.set_link(&path, "", false),
    });
}

/// The settings that suit this computer, from the server's /health (`recommended`): made by themselves once per load
/// of the plugin, when the server first answers (user, 2026-10-27: no button); a notice says what changed.
async fn optimize() {
    if OPTIMIZED.with(|o| o.replace(true)) {
        return;
    }
    let Some(base) = with(|a| a.base()) else { return };
    let signal = call!(get(&js::global(), "AbortSignal"), timeout, 5000);
    let got = match js::wait(call!(js::global(), fetch, format!("{base}/health"), obj! { signal: signal })).await {
        Ok(res) => js::wait(call!(res, json)).await.ok(),
        Err(_) => None,
    };
    let rec = got.map(|j| get(&j, "recommended")).filter(|r| r.is_object());
    let Some(rec) = rec else { return };
    let found: String = str(&get(&rec, "hardware")).chars().filter(|c| !c.is_control()).take(200).collect();
    let wanted = get(&rec, "settings");
    let names = [("useGpu", "Use the GPU"), ("contextParagraphs", "Context paragraphs")];
    let changes: Vec<(&str, JsValue, String)> = with(|a| {
        names.iter().filter_map(|(k, name)| {
            let (now, v) = (a.field(k), get(&wanted, k));
            // a switch for Use the GPU, a whole number within the slider's range for Context paragraphs: nothing else
            // from whatever answers on the server's port is taken
            let fits = match *k {
                "useGpu" => v.as_bool().is_some(),
                _ => v.as_f64().is_some_and(|n| n.fract() == 0.0 && (0.0..=50.0).contains(&n)),
            };
            if !fits || now == v || now.as_f64().zip(v.as_f64()).is_some_and(|(x, y)| x == y) {
                return None;
            }
            let show = |x: &JsValue| match x.as_bool() {
                Some(true) => "on".to_string(),
                Some(false) => "off".to_string(),
                None => x.as_f64().map(|n| n.to_string()).unwrap_or_default(),
            };
            Some((*k, v.clone(), format!("{name}: {} \u{2192} {}", show(&now), show(&v))))
        }).collect()
    }).unwrap_or_default();
    if changes.is_empty() {
        return;
    }
    let list = changes.iter().map(|(_, _, l)| l.as_str()).collect::<Vec<_>>().join("; ");
    with(|a| {
        a.notice(&format!("settings made to suit this computer ({found}): {list}. Use the GPU takes effect when the server next starts."), 12_000.0);
        for (k, v, _) in &changes {
            a.set_field(k, v.clone());
        }
    });
    // the settings tab drawn again with the new values
    if let Some(tab) = with(|a| a.tab.clone()) {
        if !tab.is_undefined() {
            call!(tab, display);
        }
    }
}

/// `GET /health` with a timeout (`AbortSignal.timeout`, <https://developer.mozilla.org/en-US/docs/Web/API/AbortSignal/timeout_static)>.
async fn health(base: String) -> Option<Info> {
    let signal = call!(get(&js::global(), "AbortSignal"), timeout, 1500);
    let res = js::wait(call!(js::global(), fetch, format!("{base}/health"), obj! { signal: signal })).await.ok()?;
    if !get(&res, "ok").is_truthy() {
        return None;
    }
    Some(info(&js::wait(call!(res, json)).await.ok()?))
}

/// After a failed request, or at start-up: is the server there? (A server already running is adopted.)
pub async fn check() {
    let Some(base) = with(|a| a.base()) else { return };
    match health(base).await {
        Some(i) => _ = with(|a| a.running(i)),
        None => _ = with(|a| {
            if a.state == State::Running && a.child.is_none() {
                let d = if a.remote() { format!("no server answers at {}", a.base()) } else { String::new() };
                a.set_state(State::Stopped, &d);
            }
        }),
    }
}

pub(crate) async fn start_server() {
    let Some(Some(base)) = with(|a| (!matches!(a.state, State::Starting | State::Running)).then(|| a.base())) else { return };
    match health(base).await {
        Some(i) => _ = with(|a| a.running(i)),
        None => _ = with(|a| match (a.remote(), a.can_spawn) {
            (true, false) => a.set_state(State::Stopped, &format!("no server answers at {}", a.base())),
            // A desktop with an address set: most often one meant for another device, such as Ignis's relay, which exists
            // only inside its sandbox
            (true, true) => a.set_state(State::Stopped, &format!("no server answers at {}, the Server address set on this device. To use this computer's own server, empty Settings \u{2192} This device \u{2192} Server address", a.base())),
            (false, false) => a.set_state(State::Stopped, CANNOT_START),
            (false, true) => {
                a.fallback = None;
                a.spawn_server(0);
            }
        }),
    }
}

async fn stop_server() {
    let Some(Some((base, remote))) = with(|a| (!matches!(a.state, State::Stopped | State::Failed)).then(|| (a.base(), a.remote()))) else { return };
    if remote {
        with(|a| a.set_state(State::Stopped, "not using the server elsewhere; it keeps running there")); // never /shutdown it
        return;
    }
    with(|a| a.set_state(State::Stopping, ""));
    let signal = call!(get(&js::global(), "AbortSignal"), timeout, 2000);
    _ = js::wait(call!(js::global(), fetch, format!("{base}/shutdown"), obj! { method: "POST", signal: signal })).await;
    with(|a| match a.child.clone() {
        Some(c) => _ = js::timeout(4000.0, move || if get(&c, "exitCode").is_null() { _ = call!(c, kill); }),
        None => a.set_state(State::Stopped, ""),
    });
}

/// The editor's context menu, shown only when a few words are selected (bold::term): "Always mark as a key term" (or
/// "Stop always marking as a key term" for one already listed) adds them to *Always mark*, or takes them out. The
/// menu: `editor-menu` (<https://docs.obsidian.md/Reference/TypeScript+API/Workspace/on('editor-menu')>); the selection:
/// <https://docs.obsidian.md/Reference/TypeScript+API/Editor> (somethingSelected, getSelection).
fn editor_menu(menu: JsValue, editor: JsValue, _view: JsValue) -> JsValue {
    if !call!(editor, somethingSelected).is_truthy() {
        return JsValue::UNDEFINED;
    }
    if let Some(term) = crate::bold::term(&str(&call!(editor, getSelection))) {
        let listed = with(|a| crate::bold::listed(&a.settings.bold_terms, &term)) == Some(true);
        let title = if listed { "Stop always marking as a key term" } else { "Always mark as a key term" };
        call!(menu, addItem, f1(move |item| {
            let term = term.clone();
            call!(call!(item, setTitle, title), setIcon, "highlighter");
            call!(item, onClick, f0(move || _ = with(|a| a.toggle_bold(&term))))
        }));
    }
    JsValue::UNDEFINED
}

fn toggle_server() {
    if with(|a| matches!(a.state, State::Running | State::Starting)) == Some(true) {
        js::spawn(stop_server());
    } else {
        js::spawn(start_server());
    }
}

const UI: &[Row] = &[
    ("", "Documentation", "How the plugin works, the server, its settings and fallbacks: ", Ctl::Link("https://github.com/Gameboard-dev/obsidian-augmented-view#readme")),
    ("", "Emphasis", "", Ctl::Head),
    ("enabled", "Show emphasis", "Off hides it without stopping the server.", Ctl::Toggle),
    ("sensitivity", "Sensitivity", "Left: only the surest colours and the strongest term. Right: every sentence and phrase the models coloured, up to one term per 20 words and every contrast word.", Ctl::Slider(0.0, 1.0, 0.05)),
    ("", "", "What is found is shown in the colours of the note's colour code (below): sentences by what each colour's description says, phrases by their kinds and rules. Bold, italics and underlines are left for you to write in the text.", Ctl::Note),
    ("boldTerms", "Always mark", "Words and phrases marked as key terms in every paragraph, each time they appear, whatever the sensitivity (while key terms are on): whole words, in any case, never in links, code, maths or citations. Separate them with commas. To add one, select it in a note, right-click and choose Always mark as a key term; choose Stop always marking as a key term to take it out.", Ctl::Text),
    ("everyOccurrence", "Every occurrence", "Emphasise a chosen term or contrast word each time it appears in the paragraph, not only the first time.", Ctl::Toggle),
    ("diversity", "Diversity", "How strongly the ranking avoids terms that mean much the same. Changing it asks the server again.", Ctl::Slider(0.0, 1.0, 0.05)),
    ("", "What to send", "", Ctl::Head),
    ("contextParagraphs", "Context paragraphs", "Paragraphs on screen are sent first, once you stop scrolling. Then, when the server is idle and you have stayed put for a second, this many paragraphs above and below, two at a time, so they are ready as you scroll. More: more ready ahead, and more work for the GPU.", Ctl::Slider(0.0, 50.0, 1.0)),
    ("", "", "Results are kept per note in the plugin's cache folder and read only when that note opens. A paragraph is sent again only when its text changes, and the note's file then holds only its current paragraphs.", Ctl::Note),
    ("debugLog", "Debug log", "Write what the plugin does to debug-desktop.log (debug-remote.log on a device using a server elsewhere) in the plugin's folder.", Ctl::Toggle),
    ("", "Server", "", Ctl::Head),
    ("", "", "The status bar shows where the models run (CUDA, Metal or CPU); its tooltip shows why anything is not on the GPU, and the GPU's and the computer's load and memory.", Ctl::Note),
    ("autoStart", "Start with Obsidian", "", Ctl::Toggle),
    ("idleMinutes", "Stop when idle", "Minutes without a request before the server exits and frees the GPU (0 = never).", Ctl::Number),
    ("useGpu", "Use the GPU", "CUDA (NVIDIA) or Metal (Apple) when the server's build supports it and the GPU has room for the models; otherwise, or off, the CPU (slower). Takes effect when the server next starts.", Ctl::Toggle),
    ("gpuLimit", "GPU load limit", "The most of the time the GPU may work for Augmented View, in percent: lower means less heat, fan and coil noise, and results more slowly. Takes effect when the server next starts.", Ctl::Slider(10.0, 100.0, 10.0)),
    ("warnPercent", "Warn at", "Warn when the GPU's load or memory, or the computer's memory, reaches this percent (all programs together).", Ctl::Slider(50.0, 100.0, 5.0)),
    ("model", "Model", "A BERT-type embedding model on Hugging Face. If it is not on this computer yet, the server does not start, and Models (below) shows the command that downloads it. Takes effect when the server next starts.", Ctl::Text),
    ("port", "Port", "", Ctl::Number),
    ("", "Models", "", Ctl::Head),
    ("", "", "", Ctl::Custom(crate::models::mark)),
    ("", "This device", "", Ctl::Head),
    ("local.server", "Server", "The server's executable for this computer. Empty: augmented-view-server in the plugin's folder. Kept on this device only.", Ctl::Text),
    ("local.cpuServer", "CPU server", "A CPU-only build, started when the server above cannot start at all (a CUDA build where CUDA's libraries are missing). Empty: augmented-view-server-cpu in the plugin's folder, if there. Kept on this device only.", Ctl::Text),
    ("local.serverUrl", "Server address", "Only for a device that cannot start the server itself (a sandbox, or a computer without one): the address of one running elsewhere. Empty: this computer's own server, on Port, started by the plugin; in Ignis (Obsidian in a browser), the server desktop Obsidian runs on the computer that hosts Ignis, through Ignis's relay. Kept on this device only.", Ctl::Text),
    ("local.maxParas", "Paragraphs per request", "Fewer show sooner where a proxy passes answers on only once they are complete (16 suits one; Ignis's relay streams, so it needs none). Kept on this device only.", Ctl::Number),
];


/// What each kind is called on a colour's row ("Shows: …").
const KIND_NAMES: [(&str, &str); 7] = [("main", "main points"), ("point", "critical points"), ("term", "key terms"), ("stress", "contrast words"), ("quote", "quotations"), ("definition", "definitions"), ("citation", "citations")];

const STYLE_ID: &str = "augmented-view-colours";

/// The kinds are always found and drawn (user, 2026-10-28): what shows is decided by the colour code, each colour shown
/// or hidden by its check box in the floating key. The old switches (terms, contrast, quotes, definitions, points)
/// stay in data.json only so that older files read.
fn fixed(mut s: Settings) -> Settings {
    (s.terms, s.contrast, s.quotes, s.definitions, s.points) = (true, true, true, true, true);
    s
}
/// Where the servers for each kind of computer are published (README, "Install").
const RELEASES: &str = "https://github.com/Gameboard-dev/obsidian-augmented-view/releases";

impl App {
    /// Whether this device uses a server elsewhere (its Server address, or in Ignis the relay), rather than this
    /// computer's own.
    pub(crate) fn remote(&self) -> bool {
        !self.local.server_url.trim().is_empty() || self.relayed()
    }

    /// In Ignis without a Server address: the server desktop Obsidian runs on the computer that hosts Ignis, through
    /// Ignis's relay on the page's own address (ignis/SETUP.md, "The relay"), which streams answers as they come.
    fn relayed(&self) -> bool {
        self.local.server_url.trim().is_empty() && js::in_ignis()
    }

    pub(crate) fn base(&self) -> String {
        if self.relayed() {
            return format!("{}/relay/augmented-view", str(&get(&get(&js::global(), "location"), "origin")));
        }
        match self.remote() {
            true => self.local.server_url.trim().trim_end_matches('/').to_string(),
            false => format!("http://127.0.0.1:{}", self.settings.port),
        }
    }

    fn cache_dir(&self) -> String {
        format!("{}/cache", self.dir)
    }

    fn cache_path(&self, file: &str) -> String {
        format!("{}/cache/{file}", self.dir)
    }

    /// To the worker, the bytes transferred rather than copied (<https://developer.mozilla.org/en-US/docs/Web/API/Worker/postMessage>).
    pub fn post(&self, m: &ToWorker) {
        let arr = Uint8Array::from(&msg::encode(m)[..]);
        call!(self.worker, postMessage, arr.clone(), Array::of1(&arr.buffer()));
    }

    /// A document for the worker; the version comes back with its marks.
    pub fn send_doc(&mut self, path: &str, text: String) -> u32 {
        self.version += 1;
        self.docs.insert(path.to_string());
        self.post(&ToWorker::Doc { path: path.to_string(), version: self.version, text });
        self.version
    }

    /// For a Reading view: the note's text from the file, once (and again when the file changes).
    pub fn ensure_doc(&mut self, path: &str) {
        if self.docs.insert(path.to_string()) {
            let file = call!(self.vault, getAbstractFileByPath, path);
            js::spawn(read_doc(file));
        }
    }

    fn log(&self, line: &str) {
        if !self.settings.debug_log {
            return;
        }
        let (ad, file) = (self.adapter.clone(), format!("{}/debug-{}.log", self.dir, if self.remote() { "remote" } else { "desktop" }));
        let line = format!("{} {line}\n", str(&call!(new(&get(&js::global(), "Date"), &[]), toISOString)));
        js::spawn(async move {
            _ = js::wait(if exists(&ad, &file).await { call!(ad, append, file, line) } else { call!(ad, write, file, line) }).await;
        });
    }

    /// Everything Obsidian is told about: the editor extension and the Reading-view post-processor
    /// (<https://docs.obsidian.md/Reference/TypeScript+API/Plugin/registerEditorExtension>, <https://docs.obsidian.md/Reference/TypeScript+API/Plugin/registerMarkdownPostProcessor)>, vault events
    /// (<https://docs.obsidian.md/Reference/TypeScript+API/Vault>, `on`), the settings tab (<https://docs.obsidian.md/Plugins/User+interface/Settings>), the
    /// status bar item (<https://docs.obsidian.md/Plugins/User+interface/Status+bar>), commands
    /// (<https://docs.obsidian.md/Plugins/User+interface/Commands>), and the start-up check once the layout is
    /// ready (<https://docs.obsidian.md/Reference/TypeScript+API/Workspace/onLayoutReady>).
    fn register(&mut self) {
        let p = self.plugin.clone();
        call!(p, registerEditorExtension, crate::editor::extension(&self.cmv));
        call!(p, registerMarkdownPostProcessor, f2(|el, ctx| {
            with(|a| a.process(el, ctx));
            JsValue::UNDEFINED
        }));
        let on = |event: &str, f: JsValue| _ = call!(p, registerEvent, call!(self.vault, on, event, f));
        on("modify", f1(|file| {
            if with(|a| a.docs.contains(&str(&get(&file, "path")))) == Some(true) {
                js::spawn(read_doc(file));
            }
            JsValue::UNDEFINED
        }));
        on("rename", f2(|file, old| {
            with(|a| {
                let (from, to) = (str(&old), str(&get(&file, "path")));
                if a.docs.remove(&from) {
                    a.docs.insert(to.clone());
                }
                let before = a.settings.links.clone();
                crate::colours::rename(&mut a.settings.links, &from, &to);
                if a.settings.links != before {
                    a.persist_soon();
                }
                a.post(&ToWorker::Rename { from, to });
            });
            JsValue::UNDEFINED
        }));
        on("delete", f1(|file| {
            let path = str(&get(&file, "path"));
            if let Some((ad, full)) = with(|a| (a.adapter.clone(), a.cache_path(&crate::cache::file_name(&path)))) {
                js::spawn(async move { if exists(&ad, &full).await { _ = js::wait(call!(ad, remove, full)).await; } });
            }
            JsValue::UNDEFINED
        }));
        let tab = new(&get(&self.obs, "PluginSettingTab"), &[get(&p, "app"), p.clone()]);
        self.tab = tab.clone();
        let t = tab.clone();
        set(&tab, "display", f0(move || with(|a| a.display(&t)).unwrap_or_default()));
        call!(p, addSettingTab, tab);
        self.status = call!(p, addStatusBarItem);
        call!(get(&self.status, "classList"), add, "av-status", "mod-clickable");
        call!(p, registerDomEvent, self.status.clone(), "click", f1(|e| {
            with(|a| a.menu(&e));
            JsValue::UNDEFINED
        }));
        let command = |id: &str, name: &str, f: JsValue| _ = call!(p, addCommand, obj! { id: id, name: name, callback: f });
        command("toggle-server", "Start or stop the server", f0(toggle_server));
        command("toggle-emphasis", "Show or hide emphasis", f0(|| _ = with(|a| a.flip("enabled"))));
        command("more", "More emphasis", f0(|| _ = with(|a| a.nudge(0.1))));
        command("less", "Less emphasis", f0(|| _ = with(|a| a.nudge(-0.1))));
        call!(p, registerEvent, call!(get(&get(&p, "app"), "workspace"), on, "editor-menu", f3(editor_menu)));
        let workspace = get(&get(&p, "app"), "workspace");
        for event in ["active-leaf-change", "file-open"] {
            call!(p, registerEvent, call!(workspace, on, event, f0(|| _ = with(|a| a.refresh_key()))));
        }
        // a note with no colour code, opened while emphasis is on: ask which code (user, 2026-10-27)
        call!(p, registerEvent, call!(workspace, on, "file-open", f0(|| js::spawn(ask_code()))));
        call!(get(&get(&p, "app"), "workspace"), onLayoutReady, f0(|| {
            with(|a| a.refresh_key());
            js::spawn(async {
                check().await;
                if with(|a| a.state != State::Running && a.settings.auto_start) == Some(true) {
                    start_server().await;
                }
            });
            js::timeout(10_000.0, || js::spawn(remove_orphans()));
        }));
    }

    // ----- settings

    /// A setting by its key; `local.` keys are this device's (msg::Local).
    fn field(&self, key: &str) -> JsValue {
        match key.strip_prefix("local.") {
            Some(k) => get(&serde_wasm_bindgen::to_value(&self.local).unwrap_or_default(), k),
            None => get(&serde_wasm_bindgen::to_value(&self.settings).unwrap_or_default(), key),
        }
    }

    pub fn set_field(&mut self, key: &str, v: JsValue) {
        if let Some(k) = key.strip_prefix("local.") {
            let o = serde_wasm_bindgen::to_value(&self.local).unwrap_or_default();
            set(&o, k, v);
            if let Ok(l) = serde_wasm_bindgen::from_value::<Local>(o) {
                self.local = l;
            }
            self.save_local();
            self.server_changed();
            js::spawn(check()); // a new address: is a server there?
            return;
        }
        let o = serde_wasm_bindgen::to_value(&self.settings).unwrap_or_default();
        set(&o, key, v);
        if let Ok(s) = serde_wasm_bindgen::from_value::<Settings>(o) {
            self.settings = fixed(s);
        }
        self.post(&ToWorker::Settings(self.settings.clone())); // the worker redraws only what this changes
        self.server_changed();
        self.persist_soon();
    }

    /// This device's settings, in its own storage (<https://docs.obsidian.md/Reference/TypeScript+API/App/saveLocalStorage>).
    pub(crate) fn save_local(&self) {
        call!(get(&self.plugin, "app"), saveLocalStorage, LOCAL_KEY, serde_wasm_bindgen::to_value(&self.local).unwrap_or_default());
    }

    /// The code that applies to a note (colours::assigned).
    pub(crate) fn assigned(&self, path: &str) -> crate::colours::Assigned {
        crate::colours::assigned(&self.settings.colour_codes, &self.settings.links, path)
    }

    /// The id of the code that colours a note, if one does.
    pub(crate) fn code_id(&self, path: &str) -> Option<String> {
        match self.assigned(path) {
            crate::colours::Assigned::Code(i) => Some(self.settings.colour_codes[i].id.clone()),
            _ => None,
        }
    }

    /// Link a note (or its folder) to a code, or to none (""): saved, and every open view of the notes it covers
    /// redrawn in it; a note that now has a code is sent to the server, one that has none is no longer.
    pub(crate) fn set_link(&mut self, path: &str, code: &str, folder: bool) {
        crate::colours::link(&mut self.settings.links, path, code, folder);
        self.post(&ToWorker::Settings(self.settings.clone()));
        self.persist_soon();
        self.retag();
        self.refresh_key();
        js::timeout(0.0, || _ = with(|a| a.display(&a.tab.clone())));
    }

    /// After a link changes: every editor and Reading view takes its note's code (or none), starts or stops sending.
    fn retag(&mut self) {
        for id in self.editors.keys().copied().collect::<Vec<_>>() {
            self.tag_editor(id);
        }
        let uncoloured: Vec<String> = self.reading.paths().into_iter().filter(|p| self.code_id(p).is_none()).collect();
        self.reading.tag(|p| self.code_id(p));
        for p in uncoloured {
            self.post(&ToWorker::DropView(crate::view::view_id(&p)));
        }
        // Reading views of notes that had no code were never registered: render them again, after this call returns
        // (rendering runs the post-processor, which needs the App).
        let leaves = js_sys::Array::from(&call!(get(&get(&self.plugin, "app"), "workspace"), getLeavesOfType, "markdown"));
        let stale: Vec<JsValue> = leaves
            .iter()
            .map(|leaf| get(&leaf, "view"))
            .filter(|view| {
                let path = str(&get(&get(view, "file"), "path"));
                str(&call!(view, getMode)) == "preview" && self.code_id(&path).is_some() && !self.reading.shows(&path)
            })
            .collect();
        if !stale.is_empty() {
            js::timeout(0.0, move || stale.iter().for_each(|view| _ = call!(get(view, "previewMode"), rerender, true)));
        }
    }

    /// The colours' stylesheet in the document's head, written again on every change (colours::stylesheet).
    fn apply_colours(&self) {
        let doc = get(&js::global(), "document");
        let mut style = call!(doc, getElementById, STYLE_ID);
        if style.is_null() {
            style = call!(doc, createElement, "style");
            set(&style, "id", STYLE_ID);
            call!(get(&doc, "head"), appendChild, style.clone());
        }
        let css = crate::colours::stylesheet_all(&self.settings.colour_codes);
        set(&style, "textContent", css);
    }

    /// A change to the colour codes: drawn at once and saved; `layout` (a colour or code added or removed) redraws
    /// the settings tab too, after this call returns (it reads the App).
    pub(crate) fn edit_colours(&mut self, f: impl FnOnce(&mut Vec<crate::colours::ColourCode>), layout: bool) {
        f(&mut self.settings.colour_codes);
        crate::colours::ensure_ids(&mut self.settings.colour_codes);
        self.apply_colours();
        self.retag();
        self.refresh_key();
        self.post(&ToWorker::Settings(self.settings.clone()));
        self.persist_soon();
        if layout {
            js::timeout(0.0, || _ = with(|a| a.display(&a.tab.clone())));
        }
    }

    /// The editor's "Always mark as a key term": the term added to *Always mark*, or taken out if it is there.
    fn toggle_bold(&mut self, term: &str) {
        let (list, added) = crate::bold::toggle(&self.settings.bold_terms, term);
        self.set_field("boldTerms", list.into());
        let off = if self.settings.enabled && self.settings.terms { "" } else { " (shown while emphasis and key terms are on)" };
        let said = if added { format!("\u{201c}{term}\u{201d} is now always marked as a key term{off}.") } else { format!("\u{201c}{term}\u{201d} is no longer always marked.") };
        self.notice(&said, 4000.0);
    }

    fn flip(&mut self, key: &str) {
        let v = !self.field(key).is_truthy();
        self.set_field(key, v.into());
    }

    fn nudge(&mut self, by: f32) {
        let v = ((self.settings.sensitivity + by).clamp(0.0, 1.0) * 20.0).round() / 20.0;
        self.set_field("sensitivity", v.into());
    }

    /// Write data.json after a pause, so a dragged slider writes once.
    fn persist_soon(&mut self) {
        if let Some(h) = self.persist.take() {
            js::clear_timeout(h);
        }
        self.persist = Some(js::timeout(500.0, || {
            with(|a| {
                a.persist = None;
                call!(a.plugin, saveData, serde_wasm_bindgen::to_value(&a.settings).unwrap_or_default());
            });
        }));
    }

    /// The settings tab, from the rows above (obsidian_kit::settings), the Models section after its heading, then the
    /// colour codes.
    fn display(&self, tab: &JsValue) {
        obsidian_kit::settings::render(tab, &self.obs, UI, |k| self.field(k), |k, v| _ = with(|a| a.set_field(k, v))); // called inside with(): read self, not the App again
        crate::models::section(self);
        self.colour_settings(tab);
    }

    /// Whether this device may start the server (desktop Obsidian outside a sandbox).
    pub(crate) fn can_spawn(&self) -> bool {
        self.can_spawn
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state == State::Running
    }

    /// Whether a colour code has phrase colours, so the server needs GLiNER2 too.
    pub(crate) fn phrases(&self) -> bool {
        self.settings.colour_codes.iter().any(|c| !crate::colours::model_phrases(c).is_empty())
    }

    /// Each colour code: its name, a row per colour (name, colour, how it marks, opacity, dark-theme colour, what it
    /// marks), and buttons to add and remove colours and codes. Obsidian's Setting components:
    /// <https://docs.obsidian.md/Reference/TypeScript+API/Setting> (addText, addTextArea, addColorPicker, addDropdown,
    /// addSlider, addExtraButton, addButton).
    fn colour_settings(&self, tab: &JsValue) {
        use crate::colours::{dark, rgb, Colour, ColourCode, Level};
        let el = get(tab, "containerEl");
        let setting = get(&self.obs, "Setting");
        let row = || new(&setting, std::slice::from_ref(&el));
        type Edit = Box<dyn FnOnce(&mut Vec<ColourCode>)>;
        fn edit(f: Edit, layout: bool) {
            with(|a| a.edit_colours(f, layout));
        }
        let text = |s: &JsValue, value: String, placeholder: &str, area: bool, f: fn(&mut Colour, String), (ci, k): (usize, usize)| {
            let placeholder = placeholder.to_string();
            let add = if area { "addTextArea" } else { "addText" };
            js::invoke(s, add, &[f1(move |t| {
                call!(call!(t, setValue, value.clone()), setPlaceholder, placeholder.clone());
                call!(t, onChange, f1(move |v| {
                    edit(Box::new(move |codes| f(&mut codes[ci].colours[k], str(&v))), false);
                    JsValue::UNDEFINED
                }))
            })]);
        };
        let s = row();
        call!(call!(s, setName, "Colour codes"), setHeading);
        call!(row(), setDesc, "Each colour is a category drawn as a tint behind the text: a clause colour as a pale background, a phrase colour over it, each at its own opacity. A code applies to the notes and folders linked to it below (link them from the floating key). Light themes use the colour as given; dark themes a deeper version of it, unless a dark-theme colour is set.");
        for (ci, code) in self.settings.colour_codes.iter().enumerate() {
            let s = row();
            call!(s, setHeading);
            let name = code.name.clone();
            call!(s, addText, f1(move |t| {
                call!(t, setValue, name.clone());
                call!(t, onChange, f1(move |v| {
                    edit(Box::new(move |codes| codes[ci].name = str(&v)), false);
                    JsValue::UNDEFINED
                }))
            }));
            call!(s, addExtraButton, f1(move |b| {
                call!(call!(b, setIcon, "trash"), setTooltip, "Remove this colour code");
                call!(b, onClick, f0(move || edit(Box::new(move |codes| _ = codes.remove(ci)), true)))
            }));
            for (k, c) in code.colours.iter().enumerate() {
                let s = row();
                let mut shows: Vec<&str> = KIND_NAMES.iter().filter(|(kind, _)| c.kinds.iter().any(|x| x == kind)).map(|(_, n)| *n).collect();
                // the models find a clause colour by its description, and a phrase colour that no kind finds
                if c.level == crate::colours::Level::Clause {
                    shows.insert(0, "sentences its description fits (language model)");
                } else if shows.is_empty() {
                    shows.push("phrases its description fits (GLiNER2)");
                }
                call!(s, setDesc, format!("Shows: {}.", shows.join(", ")));
                call!(get(&s, "settingEl"), addClass, "av-colour-row");
                text(&s, c.name.clone(), "Name", false, |c, v| c.name = v, (ci, k));
                text(&s, c.description.clone(), "What it marks", true, |c, v| c.description = v, (ci, k));
                let hex = c.hex.clone();
                call!(s, addColorPicker, f1(move |p| {
                    call!(p, setValue, hex.clone());
                    call!(p, onChange, f1(move |v| {
                        edit(Box::new(move |codes| codes[ci].colours[k].hex = str(&v)), false);
                        JsValue::UNDEFINED
                    }))
                }));
                let level = if c.level == Level::Phrase { "phrase" } else { "clause" };
                call!(s, addDropdown, f1(move |d| {
                    call!(call!(d, addOption, "clause", "Clause"), addOption, "phrase", "Phrase");
                    call!(call!(d, setValue, level), onChange, f1(move |v| {
                        let level = if str(&v) == "phrase" { Level::Phrase } else { Level::Clause };
                        edit(Box::new(move |codes| codes[ci].colours[k].level = level), false);
                        JsValue::UNDEFINED
                    }))
                }));
                let opacity = c.opacity;
                call!(s, addSlider, f1(move |sl| {
                    call!(call!(sl, setLimits, 0.0, 1.0, 0.05), setDynamicTooltip);
                    call!(call!(sl, setValue, opacity), onChange, f1(move |v| {
                        let o = v.as_f64().unwrap_or(0.6) as f32;
                        edit(Box::new(move |codes| codes[ci].colours[k].opacity = o), false);
                        JsValue::UNDEFINED
                    }))
                }));
                let shade = rgb(&c.dark).or_else(|| rgb(&c.hex).map(dark)).map_or(String::new(), |(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}"));
                call!(s, addColorPicker, f1(move |p| {
                    call!(p, setValue, shade.clone());
                    call!(p, onChange, f1(move |v| {
                        edit(Box::new(move |codes| codes[ci].colours[k].dark = str(&v)), false);
                        JsValue::UNDEFINED
                    }))
                }));
                call!(s, addExtraButton, f1(move |b| {
                    call!(call!(b, setIcon, "rotate-ccw"), setTooltip, "Dark theme: back to the automatic colour");
                    call!(b, onClick, f0(move || edit(Box::new(move |codes| codes[ci].colours[k].dark.clear()), true)))
                }));
                call!(s, addExtraButton, f1(move |b| {
                    call!(call!(b, setIcon, "trash"), setTooltip, "Remove this colour");
                    call!(b, onClick, f0(move || edit(Box::new(move |codes| _ = codes[ci].colours.remove(k)), true)))
                }));
            }
            call!(row(), addButton, f1(move |b| {
                call!(b, setButtonText, "Add a colour");
                call!(b, onClick, f0(move || edit(Box::new(move |codes| codes[ci].colours.push(Colour::default())), true)))
            }));
        }
        call!(row(), addButton, f1(|b| {
            call!(b, setButtonText, "Add a colour code");
            call!(b, onClick, f0(|| edit(Box::new(|codes| codes.push(ColourCode::default())), true)))
        }));
        // The links: a note's own wins over its folder's, a nearer folder over a farther one; "/" is the whole vault.
        call!(call!(row(), setName, "Linked notes and folders"), setHeading);
        call!(row(), setDesc, "Which colour code each note or folder uses (a folder ends in /). A note's own link wins over its folder's. A note with none shows a picker in the floating key, and nothing is sent to the server until a code is chosen.");
        let codes: Vec<(String, String)> = self.settings.colour_codes.iter().map(|c| (c.id.clone(), c.name.clone())).collect();
        let mut links = self.settings.links.clone();
        links.sort_by(|a, b| a.path.cmp(&b.path));
        for l in links {
            let s = row();
            call!(s, setName, l.path.clone());
            let (path, code, codes) = (l.path.clone(), l.code.clone(), codes.clone());
            call!(s, addDropdown, f1(move |d| {
                for (id, name) in &codes {
                    call!(d, addOption, id.clone(), name.clone());
                }
                call!(d, addOption, "", "None (not marked)");
                let path = path.clone();
                call!(call!(d, setValue, code.clone()), onChange, f1(move |v| {
                    let (path, code) = (path.clone(), str(&v));
                    with(|a| {
                        if let Some(l) = a.settings.links.iter_mut().find(|l| l.path == path) {
                            l.code = code;
                        }
                        a.post(&ToWorker::Settings(a.settings.clone()));
                        a.persist_soon();
                        a.retag();
                        a.refresh_key();
                    });
                    JsValue::UNDEFINED
                }))
            }));
            let path = l.path.clone();
            call!(s, addExtraButton, f1(move |b| {
                let path = path.clone();
                call!(call!(b, setIcon, "trash"), setTooltip, "Remove this link");
                call!(b, onClick, f0(move || {
                    let path = path.clone();
                    with(|a| {
                        a.settings.links.retain(|l| l.path != path);
                        a.post(&ToWorker::Settings(a.settings.clone()));
                        a.persist_soon();
                        a.retag();
                        a.refresh_key();
                    });
                    js::timeout(0.0, || _ = with(|a| a.display(&a.tab.clone())));
                }))
            }));
        }
    }

    /// The status bar's menu: <https://docs.obsidian.md/Reference/TypeScript+API/Menu>.
    fn menu(&self, e: &JsValue) {
        let m = new(&get(&self.obs, "Menu"), &[]);
        let item = |title: String, checked: Option<bool>, f: Box<dyn FnOnce()>| {
            let mut f = Some(f);
            call!(m, addItem, f1(move |i| {
                call!(i, setTitle, title.clone());
                if let Some(c) = checked {
                    call!(i, setChecked, c);
                }
                let mut g = f.take();
                call!(i, onClick, f0(move || if let Some(g) = g.take() { g() }))
            }));
        };
        let running = matches!(self.state, State::Running | State::Starting);
        item(if running { "Stop the server" } else { "Start the server" }.into(), None, Box::new(toggle_server));
        if self.state == State::Running {
            // Where the models run and the latest readings, as disabled items (MenuItem.setDisabled).
            for line in std::iter::once(format!("On {}{}", self.on(), if self.cuda { "" } else { " (no CUDA)" })).chain(self.stats.lines().map(String::from)) {
                call!(m, addItem, f1(move |i| call!(call!(i, setTitle, line.clone()), setDisabled, true)));
            }
        }
        item(if self.settings.enabled { "Hide emphasis" } else { "Show emphasis" }.into(), None, Box::new(|| _ = with(|a| a.flip("enabled"))));
        call!(m, addSeparator);
        for level in [0.2f32, 0.4, 0.6, 0.8, 1.0] {
            let on = (self.settings.sensitivity - level).abs() < 0.01;
            item(format!("Sensitivity {:.0}%", level * 100.0), Some(on), Box::new(move || _ = with(|a| a.set_field("sensitivity", level.into()))));
        }
        call!(m, showAtMouseEvent, e.clone());
    }

    // ----- the server

    fn running(&mut self, i: Info) {
        if crate::models::was_missing() {
            crate::models::clear(); // the models are in now: the Models section says so
            redraw_settings();
        }
        (self.model, self.claims_model, self.has_claims) = (i.model, i.claims_model, i.claims);
        (self.device, self.cuda, self.notes) = (i.devices, i.cuda, i.notes);
        let detail = if i.claims { format!("{} + {}", self.model, self.claims_model) } else { format!("{}, no sentence colours", self.model) };
        self.set_state(State::Running, &detail);
        // the hardware settings, made to suit this computer once per load of the plugin (user, 2026-10-27)
        js::spawn(optimize());
        // the note open now, if it has no colour code: asked now that the server runs (user, 2026-10-27)
        js::spawn(ask_code());
        // Say once per server why it is not (all) on the GPU: CUDA missing or unusable, too little GPU memory, a CPU build.
        let why: Vec<String> = self.fallback.iter().cloned().chain(self.notes.iter().cloned()).collect();
        if i.pid != self.noticed && !why.is_empty() {
            self.noticed = i.pid;
            let how = if self.cuda { "partly on the CPU" } else { "without CUDA" };
            self.notice(&format!("running {how}: {}", why.join("; ")), 15_000.0);
        }
        if !self.polling {
            self.polling = true;
            js::timeout(0.0, poll_stats);
        }
    }

    /// A notice: <https://docs.obsidian.md/Reference/TypeScript+API/Notice>.
    pub(crate) fn notice(&self, text: &str, ms: f64) {
        new(&get(&self.obs, "Notice"), &[format!("Augmented View: {text}").into(), ms.into()]);
    }

    fn set_state(&mut self, state: State, detail: &str) {
        let (was, said) = (self.state, std::mem::replace(&mut self.detail, detail.to_string()));
        self.state = state;
        self.server_changed();
        if state == State::Stopped && !detail.is_empty() && detail != said {
            self.notice(&format!("the server is not running: {detail}"), 15_000.0); // why a start did nothing
        }
        if state != was || detail != said {
            self.log(&format!("server {state:?} {detail}"));
        }
        if state != was {
            if state != State::Running {
                self.aborts.values().for_each(|c| _ = call!(c, abort));
                (self.stats, self.hot) = (String::new(), 0);
                self.warned.clear();
            }
            if state == State::Failed {
                self.notice(&format!("the server did not start. {detail}"), 15_000.0);
            }
        }
    }

    /// Where the models run, in a word or two: "CUDA", "CPU", "CUDA + CPU" (one model each), "Metal".
    fn on(&self) -> String {
        let mut kinds: Vec<&str> = self.device.split("; ").map(|d| d.rsplit(": ").next().unwrap_or(d)).map(|d| d.split(',').next().unwrap_or(d)).collect();
        kinds.dedup();
        kinds.join(" + ")
    }

    /// The status bar, and the worker's view of the server: the models (the last ones reported while it is down, so
    /// cached results still show), told only when something changed.
    fn server_changed(&mut self) {
        let s = &self.settings;
        let warn = if self.warned.is_empty() { "" } else { " \u{26a0}" };
        let label = match self.state {
            State::Running if s.enabled => format!("{:.0}% \u{b7} {}{warn}", s.sensitivity * 100.0, self.on()),
            State::Running => format!("hidden{warn}"),
            st => format!("{st:?}").to_lowercase(),
        };
        call!(self.status, setText, format!("AugView {label}"));
        let cls = get(&self.status, "classList");
        call!(cls, toggle, "is-running", self.state == State::Running);
        call!(cls, toggle, "is-busy", matches!(self.state, State::Starting | State::Stopping));
        call!(cls, toggle, "is-warning", !self.warned.is_empty());
        let mut tip = vec![format!("Augmented View server: {:?}", self.state)];
        if self.state == State::Running {
            tip.push(format!("{} ({})", self.device, if self.cuda { "CUDA in use" } else { "no CUDA" }));
            tip.extend(self.fallback.iter().chain(&self.notes).cloned());
            if !self.stats.is_empty() {
                tip.push(self.stats.clone());
            }
        } else if !self.detail.is_empty() {
            tip.push(self.detail.clone());
        }
        call!(self.status, setAttr, "aria-label", tip.join("\n"));
        let running = self.state == State::Running;
        if running && (s.last_model != self.model || s.last_claims_model != self.claims_model || s.last_has_claims != self.has_claims) {
            self.settings.last_model = self.model.clone();
            self.settings.last_claims_model = self.claims_model.clone();
            self.settings.last_has_claims = self.has_claims;
            self.persist_soon();
        }
        let s = &self.settings;
        let m = ToWorker::Server {
            running,
            model: s.last_model.clone(),
            claims_model: s.last_claims_model.clone(),
            has_claims: s.last_has_claims,
            max_paras: self.local.max_paras,
        };
        let told = format!("{m:?}");
        if told != self.told {
            self.told = told;
            self.post(&m);
        }
    }

    /// The readings from /stats, shown in the status bar's tooltip, with a notice the first time one reaches the warning
    /// level (again only after it has been 5 points below it): the GPU's memory or the computer's (all programs), or the
    /// GPU busy for three readings running. Readings are null where they do not apply (no NVIDIA GPU in use).
    fn show_stats(&mut self, j: Option<&JsValue>) {
        let n = |o: &JsValue, k: &str| get(o, k).as_f64();
        let gb = |mb: f64| format!("{:.1}", mb / 1024.0);
        let pct = |u: f64, t: f64| if t > 0.0 { u * 100.0 / t } else { 0.0 };
        let warn = self.settings.warn_percent as f64;
        let (mut parts, mut levels) = (Vec::new(), Vec::new()); // levels: (key, percent, message)
        if let Some(j) = j {
            let g = get(j, "gpu");
            if g.is_object() {
                let (used, total, util) = (n(&g, "mem_used_mb").unwrap_or(0.0), n(&g, "mem_total_mb").unwrap_or(0.0), n(&g, "util").unwrap_or(0.0));
                let mut s = format!("GPU {util:.0}% busy, memory {} of {} GB", gb(used), gb(total));
                if let (Some(w), Some(l)) = (n(&g, "power_w"), n(&g, "power_limit_w")) {
                    s += &format!(", {w:.0} of {l:.0} W");
                }
                if let Some(t) = n(&g, "temp_c") {
                    s += &format!(", {t:.0} \u{b0}C");
                }
                parts.push(s + " (all programs)");
                levels.push(("vram", pct(used, total), format!("the GPU's memory is {:.0}% full ({} of {} GB, all programs). Close other programs that use the GPU, or turn off critical points.", pct(used, total), gb(used), gb(total))));
                self.hot = if util >= warn { self.hot + 1 } else { 0 };
                let busy = if self.hot >= 3 { 100.0 } else { util.min(warn - 0.1) };
                levels.push(("gpu", busy, format!("the GPU has been {util:.0}% busy for {} s. Lower GPU load limit in Augmented View's settings for less heat, fan and coil noise (results come more slowly).", self.hot as f64 * STATS_MS / 1000.0)));
            }
            if let Some(u) = n(&get(j, "cpu"), "util") {
                parts.push(format!("CPU {u:.0}% busy (all programs)"));
            }
            let r = get(j, "ram");
            if let (Some(used), Some(total)) = (n(&r, "used_mb"), n(&r, "total_mb")) {
                parts.push(format!("memory {} of {} GB", gb(used), gb(total)));
                levels.push(("ram", pct(used, total), format!("the computer's memory is {:.0}% full ({} of {} GB, all programs).", pct(used, total), gb(used), gb(total))));
            }
            let sv = get(j, "server");
            if let Some(m) = n(&sv, "ram_mb") {
                parts.push(format!("server: {} GB memory, {:.0}% CPU", gb(m), n(&sv, "cpu").unwrap_or(0.0)));
            }
        }
        self.stats = parts.join("\n");
        for (key, level, message) in levels {
            if level >= warn && self.warned.insert(key) {
                self.notice(&message, 15_000.0);
                self.log(&format!("warning: {message}"));
            } else if level < warn - 5.0 {
                self.warned.remove(key);
            }
        }
        self.server_changed();
    }

    /// This computer's server executable (`cpu`: the CPU-only build): the setting, else the file in the plugin's folder.
    pub(crate) fn server_path(&self, cpu: bool) -> String {
        let set = if cpu { &self.local.cpu_server } else { &self.local.server };
        if !set.is_empty() {
            return set.clone();
        }
        let windows = str(&get(&get(&js::global(), "process"), "platform")) == "win32";
        let base = str(&call!(self.adapter, getBasePath)); // FileSystemAdapter, desktop only
        format!("{base}/{}/augmented-view-server{}{}", self.dir, if cpu { "-cpu" } else { "" }, if windows { ".exe" } else { "" })
    }

    fn exists_on_disk(&self, path: &str) -> bool {
        call!(self.require.call1(&JsValue::NULL, &"fs".into()).unwrap_or_default(), existsSync, path).is_truthy()
    }

    /// After the server could not start at all (`why`): the CPU build if there is one, else failed.
    fn fall_back(&mut self, attempt: u8, why: String) {
        let cpu = self.server_path(true);
        if attempt == 0 && self.exists_on_disk(&cpu) {
            self.fallback = Some(format!("{why}; using the CPU build"));
            self.spawn_server(1);
        } else {
            self.set_state(State::Failed, &why);
        }
    }

    /// Start the model server as a child process (Node's `child_process.spawn`,
    /// <https://nodejs.org/api/child_process.html#child_processspawncommand-args-options>), reading its status lines
    /// from stdout. Attempt 0 runs the server, attempt 1 the CPU build when the first could not start at all.
    fn spawn_server(&mut self, attempt: u8) {
        let path = self.server_path(attempt == 1);
        if !self.exists_on_disk(&path) {
            let why = format!("no server at {path}: download the server for this computer from {RELEASES} and put it there, or set its path (Settings, Server)");
            return self.fall_back(attempt, why);
        }
        let gpu_mode = std::mem::replace(&mut self.gpu_mode, "ask".into());
        let args = server_args(&self.settings, gpu_mode);
        self.shortage = None;
        self.missing = None;
        let cp = self.require.call1(&JsValue::NULL, &"child_process".into()).unwrap_or_default();
        let stdio: Array = ["ignore", "pipe", "pipe"].iter().map(|s| JsValue::from(*s)).collect();
        let child = call!(cp, spawn, path.clone(), args.into_iter().map(JsValue::from).collect::<Array>(), obj! { windowsHide: true, stdio: stdio });
        self.child = Some(child.clone());
        self.set_state(State::Starting, "loading the models");
        let (out, err) = (Rc::new(RefCell::new(String::new())), Rc::new(RefCell::new(String::new())));
        let stdout = get(&child, "stdout");
        call!(stdout, setEncoding, "utf8");
        call!(stdout, on, "data", f1(move |d| {
            out.borrow_mut().push_str(&str(&d));
            loop {
                // not `while let`: its condition's borrow would last through the body, and borrow_mut would panic
                let Some(i) = out.borrow().find('\n') else { break };
                let line: String = out.borrow_mut().drain(..=i).collect();
                if let Ok(m) = JSON::parse(&line) {
                    if get(&m, "gpu_shortage").is_truthy() {
                        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                        with(|a| a.shortage = Some(v["gpu_shortage"].clone()));
                    } else if get(&m, "missing").is_truthy() {
                        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                        with(|a| a.missing = Some(v));
                    } else if get(&m, "ready").is_truthy() {
                        with(|a| a.running(info(&m)));
                    } else if get(&m, "stopping").is_truthy() {
                        with(|a| a.set_state(State::Stopping, &str(&get(&m, "stopping"))));
                    }
                }
            }
            JsValue::UNDEFINED
        }));
        let tail = err.clone();
        call!(get(&child, "stderr"), on, "data", f1(move |d| {
            let mut e = tail.borrow_mut();
            e.push_str(&str(&d));
            let cut = e.ceil_char_boundary(e.len().saturating_sub(4000));
            e.drain(..cut);
            JsValue::UNDEFINED
        }));
        let me = child.clone();
        call!(child, on, "exit", f1(move |code| {
            let last = err.borrow().lines().filter(|l| !l.contains("arn") && !l.contains("Loading weights")).last().unwrap_or("").to_string();
            with(|a| {
                if a.child.as_ref() != Some(&me) {
                    return; // a server this plugin has since replaced or let go
                }
                a.child = None;
                let code = code.as_f64();
                if let (Some(3.0), Some(short)) = (code, a.shortage.take()) {
                    a.set_state(State::Stopped, "not enough GPU memory");
                    js::spawn(gpu_choice(short));
                    return;
                }
                if let (Some(4.0), Some(missing)) = (code, a.missing.take()) {
                    // the models are not downloaded yet: the server did not start. The plugin says which and shows the
                    // Models section, with the command the user runs; it never downloads them itself
                    a.state = State::Stopped;
                    a.detail = "the models are not downloaded yet (Settings \u{2192} Models)".into();
                    a.server_changed();
                    a.log("server Stopped: models not downloaded yet (exit 4)");
                    js::timeout(0.0, move || crate::models::on_exit_missing(missing));
                    return;
                }
                match a.state {
                    State::Starting if code.is_some_and(|c| CANNOT_RUN.contains(&c)) => {
                        a.fall_back(attempt, format!("{} could not start (exit {}; for a CUDA build: CUDA's libraries are missing)", a.server_path(attempt == 1), code.unwrap_or(0.0)))
                    }
                    State::Starting => a.set_state(State::Failed, &if last.is_empty() { format!("exited with code {code:?}") } else { last }),
                    State::Failed => {}
                    _ => a.set_state(State::Stopped, ""),
                }
            });
            JsValue::UNDEFINED
        }));
        let me = child.clone();
        call!(child, on, "error", f1(move |e| {
            with(|a| {
                if a.child.as_ref() == Some(&me) {
                    a.child = None;
                    a.fall_back(attempt, str(&get(&e, "message")));
                }
            });
            JsValue::UNDEFINED
        }));
    }
}

/// The server's readings every couple of seconds while it runs (`GET /stats`); where there are none (an older server, or
/// a relay that does not pass them on) the tooltip just shows none.
fn poll_stats() {
    js::spawn(async {
        let Some(Some(base)) = with(|a| (a.state == State::Running).then(|| a.base())) else {
            with(|a| a.polling = false);
            return;
        };
        let signal = call!(get(&js::global(), "AbortSignal"), timeout, 1500);
        let j = async {
            let res = js::wait(call!(js::global(), fetch, format!("{base}/stats"), obj! { signal: signal })).await.ok()?;
            if !get(&res, "ok").is_truthy() {
                return None;
            }
            js::wait(call!(res, json)).await.ok()
        }
        .await;
        with(|a| a.show_stats(j.as_ref()));
        js::timeout(STATS_MS, poll_stats);
    });
}

/// A note's text from the vault: <https://docs.obsidian.md/Reference/TypeScript+API/Vault/cachedRead>.
async fn read_doc(file: JsValue) {
    let Some(vault) = with(|a| a.vault.clone()) else { return };
    let path = str(&get(&file, "path"));
    if let Ok(text) = js::wait(call!(vault, cachedRead, file)).await {
        with(|a| a.send_doc(&path, str(&text)));
    }
}

/// Cache files whose note no longer exists (deleted while Obsidian was closed): found by name, nothing is read
/// (<https://docs.obsidian.md/Reference/TypeScript+API/Vault/getMarkdownFiles>, <https://docs.obsidian.md/Reference/TypeScript+API/DataAdapter/list)>.
async fn remove_orphans() {
    let Some((ad, vault, dir)) = with(|a| (a.adapter.clone(), a.vault.clone(), a.cache_dir())) else { return };
    if !exists(&ad, &dir).await {
        return;
    }
    let notes: HashSet<String> = Array::from(&call!(vault, getMarkdownFiles)).iter().map(|f| crate::cache::file_name(&str(&get(&f, "path")))).collect();
    let Ok(list) = js::wait(call!(ad, list, dir)).await else { return };
    for f in Array::from(&get(&list, "files")).iter().map(|f| str(&f)) {
        if !notes.contains(f.rsplit('/').next().unwrap_or("")) {
            _ = js::wait(call!(ad, remove, f)).await;
        }
    }
}

/// The service's shortage answer as obsidian_kit::gpu reads it.
fn shortage_of(v: &serde_json::Value) -> obsidian_kit::gpu::Shortage {
    obsidian_kit::gpu::Shortage {
        need_mb: v["need_mb"].as_u64().unwrap_or(0),
        free_mb: v["free_mb"].as_u64().unwrap_or(0),
        others_mb: v["others_mb"].as_u64().unwrap_or(0),
        holders: v["holders"].as_array().into_iter().flatten().filter_map(|h| h.as_str().map(str::to_string)).collect(),
    }
}

/// The settings tab drawn again (after this call returns: it reads the App).
pub(crate) fn redraw_settings() {
    js::timeout(0.0, || _ = with(|a| a.display(&a.tab.clone())));
}

/// The server's arguments for a start to serve: never `--download`. The plugin starts no other program, so it never
/// starts a download (models.rs; Obsidian's developer policies). `gpu_mode`: what to do when the GPU has no room.
pub(crate) fn server_args(s: &Settings, gpu_mode: String) -> Vec<String> {
    let mut args = vec!["--port".to_string(), s.port.to_string(), "--model".into(), s.model.clone(), "--idle-minutes".into(), s.idle_minutes.to_string()];
    args.extend(["--gpu-limit".into(), s.gpu_limit.to_string()]);
    if !s.use_gpu {
        args.extend(["--device".into(), "cpu".into()]);
    } else {
        args.extend(["--gpu".into(), gpu_mode]);
    }
    if !s.points && !s.contrast {
        args.push("--no-claims".into()); // spares the instruct model's memory
    }
    // GLiNER2 is checked for only when a code has phrase colours
    if s.colour_codes.iter().any(|c| !crate::colours::model_phrases(c).is_empty()) {
        args.push("--phrases".into());
    }
    args
}

/// The server stopped before loading: the GPU has too little free memory. The user chooses, asked as every plugin of the
/// workspace asks (obsidian_kit::gpu): free the memory the user's other GPU services hold, use the CPU, or leave it
/// stopped.
async fn gpu_choice(short: serde_json::Value) {
    let Some((obs, app)) = with(|a| (a.obs.clone(), get(&a.plugin, "app"))) else { return };
    let asking = obsidian_kit::gpu::Asking { plugin: "Augmented View", models: "its emphasis models", task: "emphasis", on_cpu: "Use the CPU", decline: "Leave it stopped" };
    let mode = obsidian_kit::gpu::choose(&obs, &app, &shortage_of(&short), &asking).await;
    if let Some(m) = mode {
        with(|a| {
            a.gpu_mode = m.into();
            a.spawn_server(0);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The server's arguments never ask it to download, whatever the settings and the GPU answer.
    #[test]
    fn the_server_is_never_started_with_download() {
        let mut s = Settings::default();
        for (gpu, points, phrases, mode) in [(true, true, false, "ask"), (false, false, true, "cpu"), (true, true, true, "free")] {
            s.use_gpu = gpu;
            (s.points, s.contrast) = (points, points);
            if phrases {
                let mut code = crate::colours::ColourCode::default();
                code.colours.push(crate::colours::Colour { level: crate::colours::Level::Phrase, kinds: Vec::new(), ..Default::default() });
                s.colour_codes.push(code);
            }
            let args = server_args(&s, mode.into());
            assert!(!args.iter().any(|a| a.contains("download")), "{args:?}");
            assert_eq!(args.contains(&"--phrases".to_string()), s.colour_codes.iter().any(|c| !crate::colours::model_phrases(c).is_empty()));
        }
    }

    /// The plugin starts programs only in app.rs (Node's child_process, here alone), and nowhere names the server's
    /// download option except the command it shows to copy (models.rs, which starts nothing).
    #[test]
    fn only_the_shown_command_names_download() {
        let flag = concat!("\"--", "download");
        let spawns = concat!("child_", "process");
        for (name, src) in [("app.rs", include_str!("app.rs")), ("models.rs", include_str!("models.rs")), ("lib.rs", include_str!("lib.rs")), ("engine.rs", include_str!("engine.rs")), ("editor.rs", include_str!("editor.rs")), ("view.rs", include_str!("view.rs")), ("key.rs", include_str!("key.rs")), ("reading.rs", include_str!("reading.rs")), ("cache.rs", include_str!("cache.rs"))] {
            assert!(!src.contains(flag) || name == "models.rs", "{name} names --download");
            assert!(!src.contains(spawns) || name == "app.rs", "{name} starts a program");
        }
        assert!(!include_str!("models.rs").contains(spawns), "the Models section starts nothing");
    }
}
