//! Augmented View: an Obsidian plugin in Rust. main.js (the only JavaScript, a few lines) loads this module and calls
//! `start` on Obsidian's main thread, which drives Obsidian and CodeMirror (app, editor, view); the same module runs a
//! second time in a Web Worker (`worker_start`), where the engine does everything that costs time, so nothing
//! competes with Obsidian's rendering. The two exchange postcard messages (msg).
//!
//! References: Web Workers (<https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Using_web_workers>), transferable objects
//! (<https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Transferable_objects>).

pub use obsidian_kit::js; // shared with the other plugins (../../shared)

mod app;
mod bold;
mod cache;
mod colours;
mod editor;
mod engine;
mod export;
mod key;
mod models;
mod msg;
mod pick;
mod reading;
mod rules;
mod rules_tab;
mod split;
mod view;

use std::cell::RefCell;

use js_sys::{Array, Uint8Array};
use wasm_bindgen::prelude::*;

use js::{call, f1, get, set};
use msg::{FromWorker, ToWorker};

thread_local! {
    static ENGINE: RefCell<engine::Engine> = RefCell::default();
    static TIMER: RefCell<Option<f64>> = const { RefCell::new(None) };
}

const SAVE_EVERY: f64 = 2000.0; // ms: cache files of the notes whose results changed
const STREAM_BATCH: f64 = 30.0; // ms: a streamed answer's results are drawn at most this often

/// In the worker: take messages, run the engine, send what it produces.
#[wasm_bindgen]
pub fn worker_start() {
    set(&js::global(), "onmessage", f1(|e| {
        if let Some(m) = msg::decode::<ToWorker>(&Uint8Array::new(&get(&e, "data")).to_vec()) {
            let delay = if matches!(m, ToWorker::Chunk { .. }) { STREAM_BATCH } else { 0.0 };
            ENGINE.with(|en| en.borrow_mut().handle(m, js::now()));
            schedule(delay);
        }
        JsValue::UNDEFINED
    }));
    call!(js::global(), setInterval, js::f0(|| ENGINE.with(|en| en.borrow_mut().saves()).iter().for_each(post)), SAVE_EVERY);
}

fn post(m: &FromWorker) {
    let arr = Uint8Array::from(&msg::encode(m)[..]);
    call!(js::global(), postMessage, arr.clone(), Array::of1(&arr.buffer()));
}

/// Run the outbox soon; a burst of messages (a streamed answer, a scroll) is handled in one pass.
fn schedule(delay: f64) {
    if TIMER.with(|t| t.borrow().is_some()) {
        return;
    }
    let handle = js::timeout(delay, || {
        TIMER.with(|t| t.borrow_mut().take());
        let now = js::now();
        let (out, wait) = ENGINE.with(|en| {
            let mut en = en.borrow_mut();
            (en.outbox(now), en.wait_ms(now))
        });
        out.iter().for_each(post);
        if wait > 0.0 {
            schedule(wait); // a layer backing off, or a paragraph that has just changed
        }
    });
    TIMER.with(|t| *t.borrow_mut() = Some(handle));
}
