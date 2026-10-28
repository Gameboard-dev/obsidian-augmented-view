//! The models (the server's parser, key-term, sentence-colour and phrase-colour models). Obsidian's developer policies
//! forbid plugins that "install or update themselves or their dependencies" (<https://docs.obsidian.md/Developer+policies>),
//! so **the plugin never downloads anything and never starts a download**, not even from a button (user, 2026-10-28):
//!
//! - the server will not start while a model it needs is missing: it prints a `{"missing": [...]}` line (each model's
//!   purpose, page, licence, folder, files and size) and exits with code 4. The plugin then says which models are
//!   missing, in a notice whose button opens the Models section ([`on_exit_missing`]);
//! - the Models section (Settings → Augmented View → Models) lists each model with its purpose, size, licence, a link to
//!   its page (opened in the browser) and the folder and files it needs, and shows the command the user runs in a
//!   terminal to have the server program download them (`"<server>" --download all`), with a Copy button. The plugin
//!   never runs it: the only place it starts a program is app.rs's spawn_server, whose arguments (app::server_args)
//!   never include `--download` (tested there). *Check again* asks the running server (GET /models) or starts the server
//!   again, which is its start check.

use std::cell::RefCell;

use serde_json::Value;
use wasm_bindgen::prelude::*;

use crate::app::{with, App};
use crate::js::{self, call, f0, f1, get, new, set, str};

/// The default key-term model (msg::Settings' default), which the command need not name.
const DEFAULT_MODEL: &str = "BAAI/bge-base-en-v1.5";

thread_local! {
    /// The models the server last said were missing (its exit-4 line), shown in the Models section.
    static MISSING: RefCell<Option<Value>> = const { RefCell::new(None) };
    /// The Models section's first row (a Ctl::Custom of the settings table), after which the section is drawn.
    static ANCHOR: RefCell<Option<JsValue>> = const { RefCell::new(None) };
}

/// Bytes as the plugin shows them.
pub fn gb(bytes: u64) -> String {
    match bytes {
        b if b >= 100_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        b => format!("{} MB", b.div_ceil(1_000_000)),
    }
}

/// The models a missing line names: (names, total bytes).
fn summary(v: &Value) -> (Vec<String>, u64) {
    let list = v["missing"].as_array().cloned().unwrap_or_default();
    let names = list.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect();
    let total = v["total_bytes"].as_u64().unwrap_or_else(|| list.iter().filter_map(|m| m["missing_bytes"].as_u64()).sum());
    (names, total)
}

/// The notice's text for a missing line.
pub fn notice_text(v: &Value) -> String {
    let (names, total) = summary(v);
    let n = names.len();
    format!(
        "Augmented View's server did not start: {n} model{} {} not downloaded yet (about {}): {}.",
        if n == 1 { "" } else { "s" },
        if n == 1 { "is" } else { "are" },
        gb(total),
        names.join(", ")
    )
}

/// Whether a model was missing at the last start (cleared when the server runs).
pub fn was_missing() -> bool {
    MISSING.with(|m| m.borrow().is_some())
}

pub fn clear() {
    MISSING.with(|m| *m.borrow_mut() = None);
}

/// The server exited with code 4 after its missing line: a notice says which models, with a button that opens the
/// Models section. Nothing is downloaded.
pub fn on_exit_missing(v: Value) {
    let text = notice_text(&v);
    MISSING.with(|m| *m.borrow_mut() = Some(v));
    with(|a| {
        let n = new(&get(&a.obs, "Notice"), &[text.into(), 0.into()]); // stays until closed
        let doc = get(&js::global(), "document");
        let button = call!(doc, createElement, "button");
        set(&button, "textContent", "Show the models");
        call!(get(&button, "classList"), add, "av-notice-button");
        let nn = n.clone();
        call!(button, addEventListener, "click", f1(move |_| {
            call!(nn, hide);
            open_settings();
            JsValue::UNDEFINED
        }));
        call!(get(&n, "noticeEl"), appendChild, button);
    });
    crate::app::redraw_settings();
}

/// Augmented View's settings tab, scrolled to Models.
fn open_settings() {
    let Some((app, id)) = with(|a| (get(&a.plugin, "app"), str(&get(&get(&a.plugin, "manifest"), "id")))) else { return };
    let setting = get(&app, "setting");
    call!(setting, open);
    call!(setting, openTabById, id);
    js::timeout(100.0, || {
        if let Some(el) = ANCHOR.with(|s| s.borrow().clone()) {
            // the Models heading, just above its (hidden) anchor row
            let heading = get(&get(&el, "settingEl"), "previousElementSibling");
            call!(if heading.is_object() { heading } else { get(&el, "settingEl") }, scrollIntoView, js::obj! { block: "start" });
        }
    });
}

/// The settings table's Ctl::Custom row under the Models heading: kept, and filled by [`section`] after the table.
pub fn mark(s: &JsValue) {
    call!(get(s, "settingEl"), addClass, "av-models-anchor"); // an empty row: hidden (styles.css)
    ANCHOR.with(|a| *a.borrow_mut() = Some(s.clone()));
}

fn element(parent: &JsValue, tag: &str, text: &str) -> JsValue {
    let el = call!(get(&js::global(), "document"), createElement, tag);
    set(&el, "textContent", text);
    call!(parent, appendChild, el.clone());
    el
}

/// A link to a web page, opened in the browser (Obsidian hands external links to the system); the plugin never
/// fetches it.
fn link(parent: &JsValue, url: &str, text: &str) {
    let a = element(parent, "a", text);
    set(&a, "href", url);
    set(&a, "className", "external-link");
    set(&a, "target", "_blank");
    set(&a, "rel", "noopener noreferrer");
}

/// The command the user runs in a terminal to have the server download the models: the server program's path,
/// quoted, then `--download all`, and what the server needs to know to list the same models as the plugin's start
/// does (a key-term model set by hand, phrase colours). PowerShell (Windows' default terminal) runs a quoted path only
/// after `&`. Shown and copied, never run.
pub fn command(server: &str, windows: bool, model: &str, phrases: bool) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('"', ""));
    let mut c = format!("{}{} --download all", if windows { "& " } else { "" }, quote(server));
    if !model.trim().is_empty() && model.trim() != DEFAULT_MODEL {
        c.push_str(&format!(" --model {}", quote(model.trim())));
    }
    if phrases {
        c.push_str(" --phrases");
    }
    c
}

/// Settings → Models, drawn after its anchor row: each model with its purpose, size, licence, page, folder and files;
/// the command to download the missing ones, with Copy; and Check again.
pub fn section(a: &App) {
    let Some(anchor) = ANCHOR.with(|s| s.borrow().clone()) else { return };
    let setting = get(&a.obs, "Setting");
    let body = call!(get(&js::global(), "document"), createElement, "div");
    call!(get(&anchor, "settingEl"), after, body.clone());
    if a.remote() || !a.can_spawn() {
        element(&body, "p", "The models are downloaded on the computer that runs the server.");
        return;
    }
    let windows = str(&get(&get(&js::global(), "process"), "platform")) == "win32";
    let cmd = command(&a.server_path(false), windows, &a.settings.model, a.phrases());
    if a.is_running() {
        let (b, s, base) = (body.clone(), setting.clone(), a.base());
        js::spawn(async move {
            match get_models(&base).await {
                Some(v) => fill(&s, &b, v["models"].as_array().cloned().unwrap_or_default(), true, &cmd),
                None => {
                    element(&b, "p", "The server is running, so every model it needs is on this computer (this server is too old to list them).");
                }
            }
        });
    } else if let Some(v) = MISSING.with(|m| m.borrow().clone()) {
        fill(&setting, &body, v["missing"].as_array().cloned().unwrap_or_default(), false, &cmd);
    } else {
        element(&body, "p", "Start the server (status bar) to see whether its models are on this computer. If any are missing it does not start, and this section lists them with the command that downloads them.");
        check_again(&setting, &body);
    }
}

/// GET /models from the running server.
async fn get_models(base: &str) -> Option<Value> {
    let signal = call!(get(&js::global(), "AbortSignal"), timeout, 5000);
    let res = js::wait(call!(js::global(), fetch, format!("{base}/models"), js::obj! { signal: signal })).await.ok()?;
    if !get(&res, "ok").is_truthy() {
        return None;
    }
    let text = js::wait(call!(res, text)).await.ok()?;
    serde_json::from_str(&str(&text)).ok()
}

/// The section's rows: from GET /models (`all`: every model, with its status) or from the server's missing line.
fn fill(setting: &JsValue, body: &JsValue, models: Vec<Value>, all: bool, cmd: &str) {
    set(body, "textContent", "");
    element(body, "p", "The models the server runs on this computer. Augmented View never downloads them itself: you download them with the command below, which the server program runs in a terminal (it fetches each file and checks it), or from each model's page into the folder shown. A model already in your Hugging Face cache is used from there.");
    let mut missing = 0u64;
    for m in &models {
        let ready = all && m["ready"] == true;
        if !ready {
            missing += m["missing_bytes"].as_u64().unwrap_or(0);
        }
        let s = new(setting, std::slice::from_ref(body));
        call!(s, setName, m["name"].as_str().unwrap_or_default());
        let desc = get(&s, "descEl");
        element(&desc, "div", m["purpose"].as_str().unwrap_or_default());
        let size = m["bytes"].as_u64().filter(|b| *b > 0).map_or("size listed by Hugging Face".to_string(), gb);
        let line = element(&desc, "div", &format!("{size} · "));
        let licence = m["licence"].as_str().unwrap_or_default();
        match m["licence_url"].as_str() {
            Some(u) => link(&line, u, licence),
            None => _ = element(&line, "span", licence),
        }
        element(&line, "span", " · ");
        let page = m["page"].as_str().unwrap_or_default();
        link(&line, page, if page.contains("huggingface.co") { "Hugging Face page" } else { "Model page" });
        if !ready {
            if let Some(folder) = m["folder"].as_str() {
                element(&desc, "div", &format!("Goes in: {folder}"));
            }
            let files: Vec<&str> = m["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()).collect();
            if !files.is_empty() {
                element(&desc, "div", &format!("Files: {}", files.join(", ")));
            }
        }
        let status = if ready { "On this computer".to_string() } else if m["status"] == "partial" { format!("Partly downloaded: {} to go", gb(m["missing_bytes"].as_u64().unwrap_or(0))) } else { "Not downloaded".to_string() };
        let st = element(&desc, "div", &status);
        call!(get(&st, "classList"), add, if ready { "av-model-ready" } else { "av-model-missing" });
        if let Some(note) = m["note"].as_str().filter(|n| !n.is_empty()) {
            element(&desc, "div", note);
        }
    }
    if models.iter().any(|m| !(all && m["ready"] == true)) {
        let s = new(setting, std::slice::from_ref(body));
        call!(s, setName, "Download command");
        let desc = get(&s, "descEl");
        call!(get(&s, "settingEl"), addClass, "av-models-command");
        element(&desc, "div", &format!("To download the missing models (about {}), run this in a terminal (PowerShell on Windows). The server program fetches them, checks each file against its published size and checksum, and prints its progress; Ctrl+C stops it, and running it again resumes. To download only some, put their ids after --download, separated by commas: parser, terms, colours, phrases. Augmented View does not run it for you.", gb(missing)));
        element(&desc, "code", cmd);
        let text = cmd.to_string();
        call!(s, addButton, f1(move |b| {
            let text = text.clone();
            call!(call!(b, setButtonText, "Copy"), onClick, f0(move || {
                call!(get(&get(&js::global(), "navigator"), "clipboard"), writeText, text.as_str());
                with(|a| a.notice("the download command is on the clipboard.", 4000.0));
            }))
        }));
    }
    check_again(setting, body);
}

/// Check again: the running server is asked (GET /models), or the server is started again (its start check, which
/// never downloads).
fn check_again(setting: &JsValue, body: &JsValue) {
    let s = new(setting, std::slice::from_ref(body));
    call!(call!(s, setName, "Check again"), setDesc, "After downloading: looks again for the models (starting the server if it is not running).");
    call!(s, addButton, f1(|b| call!(call!(b, setButtonText, "Check again"), onClick, f0(|| {
        if with(|a| a.is_running()) != Some(true) {
            clear();
            js::spawn(crate::app::start_server());
        }
        crate::app::redraw_settings();
    }))));
}

#[cfg(test)]
mod tests {
    /// The server's exit-4 line read as the notice says it.
    #[test]
    fn the_notice_names_the_models_and_their_size() {
        let v = serde_json::json!({ "missing": [{ "id": "parser", "name": "UDPipe English EWT (UD 2.5)", "missing_bytes": 16309608u64 }, { "id": "phrases", "name": "fastino/gliner2-base-v1", "missing_bytes": 842589868u64 }], "total_bytes": 858899476u64 });
        assert_eq!(super::notice_text(&v), "Augmented View's server did not start: 2 models are not downloaded yet (about 0.9 GB): UDPipe English EWT (UD 2.5), fastino/gliner2-base-v1.");
        let one = serde_json::json!({ "missing": [{ "name": "BAAI/bge-base-en-v1.5", "missing_bytes": 437955512u64 }] });
        assert!(super::notice_text(&one).contains("1 model is not downloaded yet (about 0.4 GB): BAAI/bge-base-en-v1.5"));
        assert_eq!(super::gb(16_309_608), "17 MB");
    }

    /// The command shown to copy: the path quoted (PowerShell's & on Windows), a model set by hand and phrase colours
    /// passed on; never run by the plugin.
    #[test]
    fn the_download_command() {
        assert_eq!(super::command(r"C:\Vault\.obsidian\plugins\augmented-view\augmented-view-server.exe", true, "BAAI/bge-base-en-v1.5", false), r#"& "C:\Vault\.obsidian\plugins\augmented-view\augmented-view-server.exe" --download all"#);
        assert_eq!(super::command("/home/u/augmented-view-server", false, "someone/embedder", true), r#""/home/u/augmented-view-server" --download all --model "someone/embedder" --phrases"#);
    }
}
