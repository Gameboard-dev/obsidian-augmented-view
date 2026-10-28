//! The engine, run in the worker: the open notes (text, paragraphs, each layer's results by paragraph hash), the
//! views showing them, the Reading-view blocks to draw, and the plan of requests.
//!
//! - A paragraph is sent once: its results are kept by the hash of its text, in memory and in its note's cache file,
//!   which is read only when the note is opened. Editing one paragraph changes only its hash, so only it is sent
//!   again, and the note's file is rewritten with the current paragraphs' results only (nothing accumulates).
//! - A paragraph that has just changed waits `QUIET_MS` before it is sent, so typing does not send every draft.
//! - Only the visible paragraphs and `context_paragraphs` either side are asked for, nearest the view first. One request per layer at a time; a failing server is retried with back-off.
//! - What is on screen goes first, once the view has rested `SETTLE_MS` (a fast scroll sends nothing). The context
//!   paragraphs wait until the view has rested `PREFETCH_MS`, nothing on screen is left to ask for and no request is
//!   running, and go `PREFETCH_BATCH` at a time, so the GPU works ahead only when idle and a scroll never waits long.
//! - Marks are computed for an editor's rendered range only, Reading-view segments for blocks near the screen only,
//!   and only for what changed (dirty views and blocks).
//! - Every result is also kept by paragraph hash in one store for the whole vault (`Shared`, the cache file
//!   paragraphs.bin): a paragraph whose text is unchanged is never sent again, whichever note it is in, after an edit is
//!   undone, after the note was closed, or after Obsidian restarts. The store keeps the latest `SHARED_MAX` results of
//!   each layer, and only those made by the current models.
//! - The user's own terms (*Always bold*) and citations are found here in the paragraphs' text (bold.rs, rules.rs),
//!   never by the server: a change to the list redraws at once and sends nothing.
//! - The second layer is each sentence's colour (/colours), asked with the note's colour code: its clause colours'
//!   names and descriptions. A paragraph's results in this layer are kept under its hash mixed with a hash of that list
//!   ([`Note::lid`]), so notes with different codes never share them, and changing a description asks again only for the
//!   notes of that code.
//!
//! The server's protocol (JSON Lines, <https://jsonlines.org/>) is described in server/src/main.rs.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use xxhash_rust::xxh3::xxh3_64;

use crate::bold;
use crate::cache::{self, CacheFile, FORMAT};
use crate::colours::{self, ColourCode};
use crate::msg::{FromWorker, Settings, ToWorker};
use crate::pick::{self, Span};
use crate::reading;
use crate::rules;
use crate::split::{split, Para};

const FAST: usize = 0; // terms, quotations, definitions: /emphasis
const COLOURS: usize = 1; // each sentence's colour, and contrast words: /colours
const ENDPOINT: [&str; 2] = ["/emphasis", "/colours"];
const COLOURS_METHOD: &str = "colours1"; // part of the layer's key: results of an earlier way are not reused
const MAX_BODY: usize = 256 * 1024; // bytes of paragraph text per request
const RECENT_NOTES: usize = 8; // notes kept in memory after their last view closes
const QUIET_MS: f64 = 1500.0;
const SETTLE_MS: f64 = 150.0;
const PREFETCH_MS: f64 = 1000.0;
const PREFETCH_BATCH: usize = 2;
/// A cache file's layer as read, kept until the models are known: the key it was made with, and its entries.
type Carry = Option<(String, Vec<(u64, Vec<Span>)>)>;

/// The vault-wide store's load request (never a note's path: a note's path has no NUL) and its cache file.
const SHARED: &str = "\u{0}paragraphs";
const SHARED_FILE: &str = "paragraphs.bin";
const SHARED_MAX: usize = 20_000; // results kept per layer: a few MB of spans

/// The latest results of every paragraph seen, by hash, whichever note it was in.
#[derive(Default)]
struct Shared {
    layers: [HashMap<u64, Vec<Span>>; 2],
    /// Oldest first, for dropping the oldest beyond SHARED_MAX.
    order: [VecDeque<u64>; 2],
    /// paragraphs.bin as read, until the models are known: a layer is taken in only when its key matches.
    carry: [Carry; 2],
    asked: bool,
    loaded: bool,
    dirty: bool,
}

#[derive(Default)]
struct Note {
    text: String,
    version: u32,
    paras: Vec<Para>,
    ids: HashSet<u64>,
    born: HashMap<u64, f64>, // paragraphs that appeared since the note was opened: when
    /// What the colours layer is asked with for this note (Engine::code_key; 0: nothing to ask), and the ids its
    /// paragraphs' results are kept under in that layer.
    code: u64,
    lids: HashSet<u64>,
    layers: [HashMap<u64, Vec<Span>>; 2],
    /// A cache-file layer whose models are not known yet: kept, and written back as it was.
    carry: [Carry; 2],
    loaded: bool,
    dirty: bool,
    touched: u64,
}

impl Note {
    fn set_text(&mut self, text: String, now: f64, fresh: bool) {
        self.paras = split(&text);
        let ids: HashSet<u64> = self.paras.iter().map(|p| p.id).collect();
        if fresh {
            for id in ids.difference(&self.ids) {
                self.born.insert(*id, now);
            }
        }
        self.born.retain(|id, t| ids.contains(id) && now - *t < QUIET_MS);
        self.ids = ids;
        self.text = text;
        self.keep();
    }

    /// The id a paragraph's results are kept under in a layer: its hash, mixed in the colours layer with the code's.
    fn lid(&self, layer: usize, id: u64) -> u64 {
        if layer == COLOURS { xxh3_64(&[id.to_le_bytes(), self.code.to_le_bytes()].concat()) } else { id }
    }

    fn has(&self, layer: usize, id: u64) -> bool {
        if layer == COLOURS { self.lids.contains(&id) } else { self.ids.contains(&id) }
    }

    /// After the paragraphs or the code changed: the colours layer's ids, and only the current paragraphs' results.
    fn keep(&mut self) {
        self.lids = self.ids.iter().map(|&id| self.lid(COLOURS, id)).collect();
        let (ids, lids) = (&self.ids, &self.lids);
        self.layers[FAST].retain(|id, _| ids.contains(id)); // results of paragraphs edited away
        self.layers[COLOURS].retain(|id, _| lids.contains(id));
    }

    /// Indices of the paragraphs overlapping [from, to] in UTF-16 offsets.
    fn paras_in(&self, from: u32, to: u32) -> std::ops::Range<usize> {
        let a = self.paras.partition_point(|p| p.to16 < from);
        a..self.paras.partition_point(|p| p.from16 <= to).max(a)
    }

    /// The lines a view's visible offsets cover (the nearest paragraph's when they fall between paragraphs).
    fn lines_of(&self, (from, to): (u32, u32)) -> (u32, u32) {
        let r = self.paras_in(from, to);
        let (a, b) = if r.is_empty() { (r.start.min(self.paras.len().saturating_sub(1)), r.start) } else { (r.start, r.end - 1) };
        match (self.paras.get(a), self.paras.get(b.min(self.paras.len().saturating_sub(1)))) {
            (Some(p), Some(q)) => (p.line_from, q.line_to),
            _ => (0, 0),
        }
    }

    fn spans(&self, id: u64) -> Vec<&Span> {
        self.layers.iter().enumerate().filter_map(|(layer, l)| l.get(&self.lid(layer, id))).flatten().collect()
    }

    fn para_text(&self, p: &Para) -> &str {
        &self.text[p.start..p.end]
    }
}

struct View {
    path: String,
    editor: bool,
    vis: (u32, u32),
    render: (u32, u32),
    /// When `vis` last changed.
    moved: f64,
}

struct Block {
    path: String,
    lines: Option<(u32, u32)>,
    idx: u32,
    n: u32,
    text: String,
    para: Option<u64>,
}

struct Request {
    layer: usize,
    ids: Vec<u64>,
    buf: String,
}

#[derive(Serialize)]
struct BodyPara<'a> {
    id: String,
    text: &'a str,
}

#[derive(Serialize)]
struct Body<'a> {
    paras: Vec<BodyPara<'a>>,
    top_k: u32,
    diversity: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    colours: Vec<Category<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    phrases: Vec<Category<'a>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stress: bool,
}

/// The colours as the server is asked for them.
fn cats(list: Vec<(usize, &colours::Colour)>) -> Vec<Category<'_>> {
    list.into_iter().map(|(i, c)| Category { i, name: &c.name, description: &c.description }).collect()
}

/// A colour the model looks for (the server's claims::Category).
#[derive(Serialize)]
struct Category<'a> {
    i: usize,
    name: &'a str,
    description: &'a str,
}

#[derive(Deserialize)]
struct InSpan {
    k: String,
    #[serde(default)]
    c: u8,
    w: f32,
    at: Vec<(u32, u32)>,
    #[serde(default)]
    v: String,
}

#[derive(Deserialize)]
struct InLine {
    id: Option<String>,
    spans: Option<Vec<InSpan>>,
}

#[derive(Default)]
pub struct Engine {
    settings: Settings,
    /// The user's own terms, from the settings (bold::terms).
    bold: Vec<Vec<char>>,
    /// Each colour code's own rules, compiled once per settings (rules::CodeRules), by the code's id.
    rules: HashMap<String, rules::CodeRules>,
    notes: HashMap<String, Note>,
    views: HashMap<u32, View>,
    blocks: HashMap<u32, Block>,
    running: bool,
    keys: [String; 2],
    has_claims: bool,
    max_paras: usize,
    inflight: [HashSet<u64>; 2],
    busy: [bool; 2],
    requests: HashMap<u32, Request>,
    next_request: u32,
    pause_until: [f64; 2],
    failures: [u32; 2],
    loads: VecDeque<String>,
    dirty_views: HashSet<u32>,
    dirty_blocks: HashSet<u32>,
    tick: u64,
    shared: Shared,
}

impl Engine {
    pub fn handle(&mut self, m: ToWorker, now: f64) {
        if !self.shared.asked {
            self.shared.asked = true;
            self.loads.push_back(SHARED.to_string());
        }
        match m {
            ToWorker::Settings(s) => {
                let diversity = (s.diversity - self.settings.diversity).abs() > 1e-6;
                self.bold = bold::terms(&s.bold_terms);
                self.rules = s.colour_codes.iter().filter_map(|c| Some((c.id.clone(), rules::CodeRules::new(c)?))).collect();
                self.settings = s;
                if diversity && !self.keys[FAST].is_empty() {
                    let model = self.keys[FAST].split('|').next().unwrap_or("").to_string();
                    self.set_key(FAST, self.fast_key(&model));
                }
                for p in self.notes.keys().cloned().collect::<Vec<_>>() {
                    self.recode(&p);
                }
                self.all_dirty();
            }
            ToWorker::Server { running, model, claims_model, has_claims, max_paras } => {
                self.running = running;
                self.has_claims = has_claims;
                self.max_paras = max_paras.max(1) as usize;
                if !model.is_empty() {
                    self.set_key(FAST, self.fast_key(&model));
                }
                if has_claims && !claims_model.is_empty() {
                    self.set_key(COLOURS, format!("{claims_model}|{COLOURS_METHOD}"));
                }
                if !running {
                    self.requests.clear();
                    self.inflight = Default::default();
                    self.busy = [false; 2];
                }
                for p in self.notes.keys().cloned().collect::<Vec<_>>() {
                    self.merge_carry(&p);
                }
                self.all_dirty();
            }
            ToWorker::Doc { path, version, text } => self.set_doc(path, version, text, now),
            ToWorker::View { id, path, editor, vis, render } => {
                self.tick += 1;
                if let Some(n) = self.notes.get_mut(&path) {
                    n.touched = self.tick;
                }
                if editor {
                    self.dirty_views.insert(id);
                }
                let moved = self.views.get(&id).filter(|v| v.path == path && v.vis == vis).map_or(now, |v| v.moved);
                self.views.insert(id, View { path, editor, vis, render, moved });
            }
            ToWorker::DropView(id) => {
                self.views.remove(&id);
                self.dirty_views.remove(&id);
            }
            ToWorker::Block { id, path, lines, idx, n, text } => {
                self.blocks.insert(id, Block { path, lines, idx, n, text, para: None });
                self.dirty_blocks.insert(id);
            }
            ToWorker::DropBlock(id) => {
                self.blocks.remove(&id);
                self.dirty_blocks.remove(&id);
            }
            ToWorker::Loaded { path, bytes } => self.loaded(&path, bytes),
            ToWorker::Chunk { req, text } => self.feed(req, &text),
            ToWorker::End { req, ok } => self.end(req, ok, now),
            ToWorker::Rename { from, to } => {
                if let Some(n) = self.notes.remove(&from) {
                    self.notes.insert(to.clone(), n);
                    self.recode(&to); // a link may give it another code
                }
                self.views.values_mut().filter(|v| v.path == from).for_each(|v| v.path = to.clone());
                self.blocks.values_mut().filter(|b| b.path == from).for_each(|b| b.path = to.clone());
            }
        }
    }

    /// What to send now: cache files to read, requests, marks and segments for what changed.
    pub fn outbox(&mut self, now: f64) -> Vec<FromWorker> {
        let file = |path: &str| if path == SHARED { SHARED_FILE.to_string() } else { cache::file_name(path) };
        let mut out: Vec<FromWorker> = self.loads.drain(..).map(|path| FromWorker::Load { file: file(&path), path }).collect();
        while let Some(r) = self.request(now) {
            out.push(r);
        }
        for view in self.dirty_views.drain().collect::<Vec<_>>() {
            if let Some((version, marks)) = self.marks(view) {
                out.push(FromWorker::Marks { view, version, marks });
            }
        }
        for block in self.dirty_blocks.drain().collect::<Vec<_>>() {
            let segs = self.wrap(block);
            out.push(FromWorker::Wrap { block, segs });
        }
        out
    }

    /// Cache files of the notes whose results changed, and the vault-wide store if it did.
    pub fn saves(&mut self) -> Vec<FromWorker> {
        let keys = self.keys.clone();
        let mut out = Vec::new();
        if self.shared.dirty && self.shared.loaded {
            self.shared.dirty = false;
            let sh = &self.shared;
            let mut file = CacheFile { format: FORMAT, ..Default::default() };
            for layer in 0..2 {
                // a layer whose models are not known yet is written back as it was read
                (file.keys[layer], file.layers[layer]) = match &sh.carry[layer] {
                    Some((k, e)) => (k.clone(), e.clone()),
                    None => (keys[layer].clone(), sh.order[layer].iter().filter_map(|id| Some((*id, sh.layers[layer].get(id)?.clone()))).collect()),
                };
            }
            out.push(FromWorker::Save { file: SHARED_FILE.into(), bytes: file.encode() });
        }
        for (path, note) in self.notes.iter_mut().filter(|(_, n)| n.dirty && n.loaded) {
            note.dirty = false;
            let mut file = CacheFile { format: FORMAT, ..Default::default() };
            for layer in 0..2 {
                let (key, mut entries): (String, Vec<(u64, Vec<Span>)>) = match &note.carry[layer] {
                    Some((k, e)) => (k.clone(), e.iter().filter(|(id, _)| note.has(layer, *id)).cloned().collect()),
                    None => (keys[layer].clone(), note.layers[layer].iter().map(|(id, s)| (*id, s.clone())).collect()),
                };
                entries.sort_unstable_by_key(|e| e.0);
                file.keys[layer] = key;
                file.layers[layer] = entries;
            }
            out.push(FromWorker::Save { file: cache::file_name(path), bytes: file.encode() });
        }
        out
    }

    /// Milliseconds until the next paused layer, quiet paragraph or resting view may be sent (0 if nothing waits).
    pub fn wait_ms(&self, now: f64) -> f64 {
        let paused = (0..2).filter(|&l| !self.busy[l]).map(|l| self.pause_until[l] - now);
        let quiet = self.notes.values().flat_map(|n| n.born.values()).map(|t| t + QUIET_MS - now);
        let rest = self.views.values().flat_map(|v| [v.moved + SETTLE_MS - now, v.moved + PREFETCH_MS - now]);
        let next = paused.chain(quiet).chain(rest).filter(|w| *w > 0.0).fold(f64::INFINITY, f64::min);
        if next.is_finite() { next } else { 0.0 }
    }

    fn set_doc(&mut self, path: String, version: u32, text: String, now: f64) {
        self.tick += 1;
        let text = if text.contains('\r') { text.replace("\r\n", "\n") } else { text };
        let note = self.notes.entry(path.clone()).or_insert_with(|| {
            self.loads.push_back(path.clone());
            Note::default()
        });
        let fresh = !note.paras.is_empty(); // a note being edited, not one just opened
        note.version = version;
        note.touched = self.tick;
        if note.text != text || note.paras.is_empty() {
            note.set_text(text, now, fresh);
        }
        self.recode(&path);
        self.path_dirty(&path);
        self.gc();
    }

    fn loaded(&mut self, path: &str, bytes: Option<Vec<u8>>) {
        if path == SHARED {
            self.shared.loaded = true;
            if let Some(file) = bytes.as_deref().and_then(CacheFile::decode) {
                for (layer, (key, entries)) in file.keys.into_iter().zip(file.layers).enumerate() {
                    self.shared.carry[layer] = (!entries.is_empty()).then_some((key, entries));
                }
            }
            self.adopt_shared();
            self.all_dirty();
            return;
        }
        let Some(note) = self.notes.get_mut(path) else { return };
        note.loaded = true;
        if let Some(file) = bytes.as_deref().and_then(CacheFile::decode) {
            for (layer, (key, entries)) in file.keys.into_iter().zip(file.layers).enumerate() {
                if note.carry[layer].is_none() && !entries.is_empty() {
                    note.carry[layer] = Some((key, entries));
                }
            }
            self.merge_carry(path);
        }
        self.path_dirty(path);
    }

    fn feed(&mut self, req: u32, chunk: &str) {
        let Some(r) = self.requests.get_mut(&req) else { return };
        r.buf.push_str(chunk);
        let layer = r.layer;
        let Some(cut) = r.buf.rfind('\n') else { return };
        let complete: String = r.buf.drain(..=cut).collect();
        for line in complete.lines() {
            let Ok(InLine { id: Some(id), spans: Some(spans) }) = serde_json::from_str::<InLine>(line) else { continue };
            let Ok(id) = u64::from_str_radix(&id, 16) else { continue };
            let spans = spans.into_iter().filter_map(|s| Some(Span { k: kind(&s.k, s.c)?, w: s.w, at: s.at, v: s.v })).collect();
            self.inflight[layer].remove(&id);
            self.store(layer, id, spans);
        }
    }

    fn end(&mut self, req: u32, ok: bool, now: f64) {
        let Some(r) = self.requests.remove(&req) else { return };
        r.ids.iter().for_each(|id| _ = self.inflight[r.layer].remove(id));
        self.busy[r.layer] = false;
        if ok {
            self.failures[r.layer] = 0;
        } else {
            self.failures[r.layer] += 1;
            self.pause_until[r.layer] = now + (1000.0 * 2f64.powi(self.failures[r.layer] as i32 - 1)).min(30_000.0);
        }
    }

    fn request(&mut self, now: f64) -> Option<FromWorker> {
        self.reuse();
        if !self.running || !self.settings.enabled {
            return None;
        }
        let wanted: Vec<_> = [FAST, COLOURS].into_iter().filter(|&l| self.layer_on(l)).map(|l| (l, self.wanted(l, now))).collect();
        // the context paragraphs only when no request runs and nothing on screen is left to ask for
        let ahead = !self.busy.contains(&true) && wanted.iter().all(|(_, w)| w.first().is_none_or(|(dist, ..)| *dist > 0));
        let limit = if ahead { PREFETCH_BATCH } else { self.max_paras };
        for (layer, wanted) in wanted {
            if self.busy[layer] || now < self.pause_until[layer] {
                continue;
            }
            let (mut ids, mut paras, mut bytes) = (Vec::new(), Vec::new(), 0);
            // one request asks with one code: the nearest paragraph's, and only paragraphs of notes with that code
            let code = wanted.first().map(|(_, path, _)| self.notes[path].code);
            let mut path_of_code = None;
            for (dist, path, i) in wanted {
                let note = &self.notes[&path];
                let (p, text) = (&note.paras[i], note.para_text(&note.paras[i]));
                if Some(note.code) != code && layer == COLOURS {
                    continue;
                }
                if (dist > 0 && !ahead) || (!paras.is_empty() && (paras.len() >= limit || bytes + text.len() > MAX_BODY)) {
                    break;
                }
                bytes += text.len();
                let lid = note.lid(layer, p.id);
                ids.push(lid);
                paras.push(BodyPara { id: format!("{lid:016x}"), text });
                path_of_code.get_or_insert(path);
            }
            if paras.is_empty() {
                continue;
            }
            let n = paras.len() as u32;
            let code = path_of_code.and_then(|p| self.code_of(&p)).filter(|_| layer == COLOURS);
            let colours = code.map_or(Vec::new(), |c| cats(colours::model_colours(c)));
            let phrases = code.map_or(Vec::new(), |c| cats(colours::model_phrases(c)));
            let stress = code.is_some_and(|c| self.stress_on(c));
            let body = serde_json::to_string(&Body { paras, top_k: 12, diversity: self.settings.diversity, colours, phrases, stress }).ok()?;
            self.next_request += 1;
            let req = self.next_request;
            self.inflight[layer].extend(&ids);
            self.busy[layer] = true;
            self.requests.insert(req, Request { layer, ids, buf: String::new() });
            return Some(FromWorker::Fetch { req, endpoint: ENDPOINT[layer].into(), body, paras: n });
        }
        None
    }

    /// Paragraphs to ask for in this layer, nearest the views first: (paragraphs from the screen, note, paragraph index).
    /// Those on screen once their view has rested `SETTLE_MS`, the others once it has rested `PREFETCH_MS`.
    fn wanted(&self, layer: usize, now: f64) -> Vec<(u32, String, usize)> {
        let mut best: HashMap<u64, (u32, &String, usize)> = HashMap::new();
        for v in self.views.values() {
            let rested = now - v.moved;
            if rested < SETTLE_MS {
                continue;
            }
            let reach = if rested < PREFETCH_MS { 0 } else { self.settings.context_paragraphs };
            let Some(note) = self.notes.get(&v.path).filter(|n| n.loaded) else { continue }; // its cache file first
            let (lf, lt) = if v.editor { note.lines_of(v.vis) } else { v.vis };
            // the paragraphs on screen, by index: those before the first that reaches the view, after the last that starts in it
            let first = note.paras.iter().position(|p| p.line_to >= lf).unwrap_or(note.paras.len());
            let last = note.paras.iter().rposition(|p| p.line_from <= lt).unwrap_or(0);
            for (i, p) in note.paras.iter().enumerate() {
                let dist = (first.saturating_sub(i)).max(i.saturating_sub(last)) as u32;
                let lid = note.lid(layer, p.id);
                if dist > reach
                    || (layer == COLOURS && note.code == 0)
                    || note.layers[layer].contains_key(&lid)
                    || self.inflight[layer].contains(&lid)
                    || note.born.get(&p.id).is_some_and(|t| now - t < QUIET_MS)
                {
                    continue;
                }
                let e = best.entry(p.id).or_insert((u32::MAX, &v.path, i));
                if dist < e.0 {
                    *e = (dist, &v.path, i);
                }
            }
        }
        let mut out: Vec<_> = best.into_values().collect();
        out.sort_unstable();
        out.into_iter().map(|(d, p, i)| (d, p.clone(), i)).collect()
    }

    fn marks(&self, view: u32) -> Option<(u32, Vec<(u32, u32, u8)>)> {
        let v = self.views.get(&view)?;
        let note = self.notes.get(&v.path)?;
        let out = note.paras[note.paras_in(v.render.0, v.render.1)].iter().flat_map(|p| self.para_ranges(&v.path, note, p)).collect();
        Some((note.version, out))
    }

    /// Whether the user's own terms are drawn: whenever emphasis and key terms are on, whatever the sensitivity.
    fn own_on(&self) -> bool {
        self.settings.enabled && self.settings.terms && !self.bold.is_empty()
    }

    /// The ranges to mark in a paragraph, in the note's UTF-16 offsets: the chosen spans, the user's own terms and the
    /// citations, with each sentence's colour cut where a rule of its code wins (pick::cut).
    fn para_ranges(&self, path: &str, note: &Note, p: &Para) -> Vec<(u32, u32, u8)> {
        let ruled = self.ruled(path, note, p);
        let spans: Vec<&Span> = ruled.as_ref().map_or_else(|| note.spans(p.id), |r| r.iter().collect());
        let mut out = pick::source_ranges(&spans, p.words, p.to16 - p.from16, &self.settings);
        if self.own_on() {
            out.extend(bold::source_ranges(note.para_text(p), &self.bold).into_iter().map(|(a, e)| (a, e, pick::TERM)));
        }
        if self.settings.enabled && self.settings.sensitivity > 0.0 {
            out.extend(rules::citations(note.para_text(p)).into_iter().map(|(a, e)| (a, e, pick::CITATION)));
        }
        pick::tidy(&mut out);
        pick::cut(&mut out, self.code_of(path).map_or(0, colours::rule_mask));
        out.into_iter().map(|(a, e, k)| (p.from16 + a, p.from16 + e, k)).collect()
    }

    /// A paragraph's spans with its code's own rules applied (rules::CodeRules::apply), or None when the code has none.
    fn ruled(&self, path: &str, note: &Note, p: &Para) -> Option<Vec<Span>> {
        let code = self.code_of(path)?;
        Some(self.rules.get(&code.id)?.apply(note.para_text(p), &note.spans(p.id)))
    }

    /// The colour code that applies to a note (colours::assigned).
    fn code_of(&self, path: &str) -> Option<&ColourCode> {
        match colours::assigned(&self.settings.colour_codes, &self.settings.links, path) {
            colours::Assigned::Code(i) => self.settings.colour_codes.get(i),
            _ => None,
        }
    }

    /// Whether contrast words are asked for with a code: when they are on and one of its colours shows them.
    fn stress_on(&self, code: &ColourCode) -> bool {
        self.settings.contrast && code.colours.iter().any(|c| c.kinds.iter().any(|k| k == "stress"))
    }

    /// What the colours layer is asked with for a note, as a hash: its code's clause and phrase colours (names and descriptions)
    /// and whether contrast words are wanted; 0 when there is nothing to ask.
    fn code_key(&self, path: &str) -> u64 {
        let Some(code) = self.code_of(path) else { return 0 };
        let colours: Vec<_> = colours::model_colours(code).into_iter().chain(colours::model_phrases(code)).collect();
        if colours.is_empty() && !self.stress_on(code) {
            return 0;
        }
        let list: Vec<(usize, &str, &str)> = colours.iter().map(|(i, c)| (*i, c.name.as_str(), c.description.as_str())).collect();
        xxh3_64(serde_json::to_string(&(list, self.stress_on(code))).unwrap_or_default().as_bytes()).max(1)
    }

    /// A note takes its code's key; when it changed, the colours layer's results under the old one are dropped and the
    /// store is looked in again.
    fn recode(&mut self, path: &str) {
        let key = self.code_key(path);
        let Some(note) = self.notes.get_mut(path).filter(|n| n.code != key) else { return };
        note.code = key;
        note.keep();
        self.path_dirty(path);
    }

    fn wrap(&mut self, block: u32) -> Vec<(u32, u32, u32)> {
        let Some(id) = self.block_para(block) else { return Vec::new() };
        let b = &self.blocks[&block];
        let note = &self.notes[&b.path];
        let Some(p) = note.paras.iter().find(|p| p.id == id) else { return Vec::new() };
        let ruled = self.ruled(&b.path, note, p);
        let spans: Vec<&Span> = ruled.as_ref().map_or_else(|| note.spans(id), |r| r.iter().collect());
        let mut found: Vec<(u32, u32, u8)> = Vec::new();
        if self.own_on() {
            found.extend(bold::block_ranges(note.para_text(p), &b.text, &self.bold).into_iter().map(|(a, e)| (a, e, pick::TERM)));
        }
        if self.settings.enabled && self.settings.sensitivity > 0.0 {
            // Reading view keeps a citation's text ("[@key, p. 3]"; Zotero Cite Links only wraps the key)
            found.extend(rules::citations(&b.text).into_iter().map(|(a, e)| (a, e, pick::CITATION)));
        }
        let rules = self.code_of(&b.path).map_or(0, colours::rule_mask);
        reading::block_segments(&b.text, &spans, &pick::choose(&spans, p.words, &self.settings), &found, rules)
    }

    fn fast_key(&self, model: &str) -> String {
        format!("{model}|{:.3}", self.settings.diversity)
    }

    fn set_key(&mut self, layer: usize, key: String) {
        if self.keys[layer] != key {
            self.keys[layer] = key;
            self.notes.values_mut().for_each(|n| n.layers[layer].clear()); // made by other models, or another diversity
            self.inflight[layer].clear();
            self.shared.layers[layer].clear();
            self.shared.order[layer].clear();
            self.adopt_shared();
        }
    }

    /// paragraphs.bin's layers made by the current models, taken into the store once those models are known.
    fn adopt_shared(&mut self) {
        let known: Vec<usize> = (0..2).filter(|&l| !self.keys[l].is_empty()).collect();
        for layer in known {
            if let Some((key, entries)) = self.shared.carry[layer].take() {
                if key == self.keys[layer] {
                    for (id, spans) in entries {
                        self.remember(layer, id, spans);
                    }
                    self.shared.dirty = false; // as read: nothing new to write
                } else {
                    self.shared.dirty = true; // another model's results: written over with the current ones
                }
            }
        }
    }

    /// A result into the vault-wide store, the oldest dropped beyond SHARED_MAX.
    fn remember(&mut self, layer: usize, id: u64, spans: Vec<Span>) {
        if self.keys[layer].is_empty() {
            return;
        }
        let sh = &mut self.shared;
        if sh.layers[layer].insert(id, spans).is_none() {
            sh.order[layer].push_back(id);
        }
        while sh.order[layer].len() > SHARED_MAX {
            if let Some(old) = sh.order[layer].pop_front() {
                sh.layers[layer].remove(&old);
            }
        }
        sh.dirty = true;
    }

    /// Paragraphs of the open notes that the store already has results for take them, so they are never sent.
    fn reuse(&mut self) {
        let mut touched = Vec::new();
        for (path, note) in self.notes.iter_mut().filter(|(_, n)| n.loaded) {
            let mut got = false;
            for layer in 0..2 {
                for p in &note.paras {
                    let lid = note.lid(layer, p.id);
                    if !note.layers[layer].contains_key(&lid) {
                        if let Some(spans) = self.shared.layers[layer].get(&lid) {
                            note.layers[layer].insert(lid, spans.clone());
                            got = true;
                        }
                    }
                }
            }
            if got {
                note.dirty = true;
                touched.push(path.clone());
            }
        }
        for path in touched {
            self.path_dirty(&path);
        }
    }

    fn layer_on(&self, layer: usize) -> bool {
        let s = &self.settings;
        !self.keys[layer].is_empty() && if layer == FAST { s.terms || s.quotes || s.definitions } else { self.has_claims && (s.points || s.contrast) }
    }

    /// Move a carried layer into memory once the models are known: kept if they match, dropped if not.
    fn merge_carry(&mut self, path: &str) {
        let Some(note) = self.notes.get_mut(path) else { return };
        for layer in (0..2).filter(|&l| !self.keys[l].is_empty()) {
            if let Some((_, entries)) = note.carry[layer].take().filter(|(k, _)| *k == self.keys[layer]) {
                let entries: Vec<_> = entries.into_iter().filter(|(id, _)| note.has(layer, *id)).collect();
                for (id, spans) in entries {
                    if !self.shared.layers[layer].contains_key(&id) {
                        let sh = &mut self.shared;
                        sh.layers[layer].insert(id, spans.clone());
                        sh.order[layer].push_back(id);
                        sh.dirty = true;
                    }
                    note.layers[layer].entry(id).or_insert(spans);
                }
            }
        }
    }

    fn store(&mut self, layer: usize, id: u64, spans: Vec<Span>) {
        self.remember(layer, id, spans.clone());
        let mut touched = Vec::new();
        for (path, note) in self.notes.iter_mut().filter(|(_, n)| n.has(layer, id)) {
            note.layers[layer].insert(id, spans.clone());
            note.dirty = true;
            touched.push((path.clone(), note.code));
        }
        for (path, code) in touched {
            let mix = |p: u64| if layer == COLOURS { Note { code, ..Default::default() }.lid(COLOURS, p) } else { p };
            self.dirty_views.extend(self.views.iter().filter(|(_, v)| v.editor && v.path == path).map(|(i, _)| *i));
            self.dirty_blocks.extend(self.blocks.iter().filter(|(_, b)| b.path == path && b.para.is_none_or(|p| mix(p) == id)).map(|(i, _)| *i));
        }
    }

    fn path_dirty(&mut self, path: &str) {
        self.dirty_views.extend(self.views.iter().filter(|(_, v)| v.editor && v.path == path).map(|(i, _)| *i));
        for (i, b) in self.blocks.iter_mut().filter(|(_, b)| b.path == path) {
            b.para = None; // the text may have changed: map again
            self.dirty_blocks.insert(*i);
        }
    }

    fn all_dirty(&mut self) {
        self.dirty_views.extend(self.views.iter().filter(|(_, v)| v.editor).map(|(i, _)| *i));
        self.dirty_blocks.extend(self.blocks.keys());
    }

    /// Which source paragraph a Reading-view block shows: the section's paragraphs by position when the counts
    /// agree, else the one sharing most of the block's first words.
    fn block_para(&mut self, block: u32) -> Option<u64> {
        let b = self.blocks.get(&block)?;
        let note = self.notes.get(&b.path)?;
        if let Some(id) = b.para.filter(|id| note.ids.contains(id)) {
            return Some(id);
        }
        let section: Vec<&Para> = b.lines.map_or(Vec::new(), |(ls, le)| note.paras.iter().filter(|p| p.line_from <= le && p.line_to >= ls).collect());
        let found = if section.len() == b.n as usize && (b.idx as usize) < section.len() {
            Some(section[b.idx as usize].id)
        } else {
            let words = reading::plain_words(&b.text, 12);
            let pool: Vec<&Para> = if section.is_empty() { note.paras.iter().collect() } else { section };
            pool.iter()
                .map(|p| (words.iter().filter(|w| reading::plain_words(note.para_text(p), 40).contains(w)).count(), p.id))
                .filter(|&(score, _)| score > 0 && score * 2 >= words.len())
                .max_by_key(|&(score, _)| score)
                .map(|(_, id)| id)
        };
        self.blocks.get_mut(&block)?.para = found;
        found
    }

    /// Drop notes no view or block shows, beyond the most recent few, once saved.
    fn gc(&mut self) {
        let shown: HashSet<&String> = self.views.values().map(|v| &v.path).chain(self.blocks.values().map(|b| &b.path)).collect();
        let mut idle: Vec<(u64, String)> = self.notes.iter().filter(|(p, n)| !shown.contains(p) && !n.dirty).map(|(p, n)| (n.touched, p.clone())).collect();
        idle.sort_unstable();
        let excess = idle.len().saturating_sub(RECENT_NOTES);
        idle.into_iter().take(excess).for_each(|(_, p)| _ = self.notes.remove(&p));
    }
}

/// A server's kind as pick's: a sentence's colour as `COLOUR` plus its place in the code.
fn kind(k: &str, c: u8) -> Option<u8> {
    if k == "colour" {
        return ((c as usize) < pick::MAX_COLOURS).then_some(pick::COLOUR + c);
    }
    ["term", "stress", "point", "main", "quote", "definition"].iter().position(|&x| x == k).map(|i| i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(ctx: u32) -> Engine {
        let mut e = Engine::default();
        let s = Settings { context_paragraphs: ctx, points: false, ..Default::default() };
        e.handle(ToWorker::Settings(s), 0.0);
        e.handle(ToWorker::Server { running: true, model: "m".into(), claims_model: String::new(), has_claims: false, max_paras: 2 }, 0.0);
        e
    }

    fn note(n: usize) -> String {
        (0..n).map(|i| format!("Paragraph number {i} has a few words.")).collect::<Vec<_>>().join("\n\n")
    }

    fn open(e: &mut Engine, path: &str, text: &str, bytes: Option<Vec<u8>>) {
        e.handle(ToWorker::Doc { path: path.into(), version: 1, text: text.into() }, 0.0);
        assert!(matches!(e.outbox(0.0).first(), Some(FromWorker::Load { .. })));
        e.handle(ToWorker::Loaded { path: path.into(), bytes }, 0.0);
    }

    /// A view that has rested long enough for everything near it to be sent.
    fn view(e: &mut Engine, editor: bool, vis: (u32, u32)) {
        e.handle(ToWorker::View { id: 1, path: "a.md".into(), editor, vis, render: (0, 100_000) }, -PREFETCH_MS);
    }

    /// Run the outbox until it is empty, answering every request: the indices of the paragraphs asked for (in order)
    /// and the number of marks the view last received.
    fn run(e: &mut Engine, now: f64) -> (Vec<usize>, usize) {
        let (mut asked, mut marks) = (Vec::new(), 0);
        loop {
            let out = e.outbox(now);
            if out.is_empty() {
                return (asked, marks);
            }
            for m in out {
                match m {
                    FromWorker::Marks { marks: m, .. } => marks = m.len(),
                    FromWorker::Fetch { req, body, .. } => {
                        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                        let mut reply = String::new();
                        for p in v["paras"].as_array().unwrap() {
                            let id = p["id"].as_str().unwrap();
                            asked.push(e.notes["a.md"].paras.iter().position(|q| format!("{:016x}", q.id) == id).unwrap());
                            reply += &format!(r#"{{"id":"{id}","spans":[{{"k":"term","t":"x","w":0.9,"at":[[0,9]],"v":"Paragraph"}}]}}"#);
                            reply += "\n";
                        }
                        let cut = reply.len() / 2; // a chunk boundary inside a line
                        e.handle(ToWorker::Chunk { req, text: reply[..cut].into() }, now);
                        e.handle(ToWorker::Chunk { req, text: reply[cut..].into() }, now);
                        e.handle(ToWorker::End { req, ok: true }, now);
                    }
                    _ => {}
                }
            }
        }
    }

    fn serve(e: &mut Engine, now: f64) -> Vec<usize> {
        run(e, now).0
    }

    #[test]
    fn nearest_first_then_cached_and_reopened() {
        let mut e = engine(100);
        open(&mut e, "a.md", &note(6), None);
        let p4 = e.notes["a.md"].paras[4].clone();
        view(&mut e, true, (p4.from16, p4.to16));
        let (asked, marks) = run(&mut e, 0.0);
        assert_eq!(asked[0], 4);
        assert_eq!(asked.len(), 6);
        assert_eq!(marks, 6);
        let Some(FromWorker::Save { bytes, .. }) = e.saves().pop() else { panic!() };
        let mut f = engine(100);
        open(&mut f, "a.md", &note(6), Some(bytes));
        view(&mut f, true, (0, 10));
        assert_eq!(run(&mut f, 0.0), (vec![], 6), "a reopened note is not sent again, and shows its marks");
    }

    #[test]
    fn an_edit_sends_only_that_paragraph_after_a_pause_and_the_file_holds_only_current_ones() {
        let mut e = engine(100);
        open(&mut e, "a.md", &note(5), None);
        view(&mut e, true, (0, 10));
        serve(&mut e, 0.0);
        let edited = note(5).replace("number 2 has", "number 2 now has");
        e.handle(ToWorker::Doc { path: "a.md".into(), version: 2, text: edited }, 100.0);
        assert!(serve(&mut e, 200.0).is_empty(), "quiet while typing");
        assert!(e.wait_ms(200.0) > 0.0);
        assert_eq!(serve(&mut e, 100.0 + QUIET_MS), vec![2]);
        let Some(FromWorker::Save { bytes, .. }) = e.saves().pop() else { panic!() };
        let file = CacheFile::decode(&bytes).unwrap();
        assert_eq!(file.layers[FAST].len(), 5, "the old paragraph's results are gone from the file");
    }

    /// The store's load is the first thing asked for; a test answers it (with `bytes`) so notes load after it.
    fn answer_shared(e: &mut Engine, bytes: Option<Vec<u8>>) {
        e.handle(ToWorker::Loaded { path: SHARED.into(), bytes }, 0.0);
    }

    #[test]
    fn an_unchanged_paragraph_is_never_sent_again_in_any_note_after_an_undo_or_a_restart() {
        let mut e = engine(100);
        answer_shared(&mut e, None);
        open(&mut e, "a.md", &note(3), None);
        view(&mut e, true, (0, 10));
        assert_eq!(serve(&mut e, 0.0).len(), 3);
        // the same paragraphs in another note: taken from the store, nothing sent
        open(&mut e, "b.md", &note(3), None);
        e.handle(ToWorker::View { id: 2, path: "b.md".into(), editor: true, vis: (0, 10), render: (0, 100_000) }, -PREFETCH_MS);
        assert!(serve(&mut e, 0.0).is_empty(), "another note with the same paragraphs");
        // an edit, then undone: the edited paragraph is sent, the restored one is not
        let edited = note(3).replace("number 1 has", "number 1 now has");
        e.handle(ToWorker::Doc { path: "a.md".into(), version: 2, text: edited }, 100.0);
        assert_eq!(serve(&mut e, 100.0 + QUIET_MS), vec![1]);
        e.handle(ToWorker::Doc { path: "a.md".into(), version: 3, text: note(3) }, 5000.0);
        assert!(serve(&mut e, 5000.0 + QUIET_MS).is_empty(), "undone: its earlier results come back");
        // a restart: the store's file alone, without the note's cache file, means nothing is sent
        let shared = e.saves().into_iter().find_map(|m| match m {
            FromWorker::Save { file, bytes } if file == SHARED_FILE => Some(bytes),
            _ => None,
        });
        let shared_copy = shared.clone();
        assert!(shared.is_some(), "the store is written");
        let mut f = engine(100);
        answer_shared(&mut f, shared);
        open(&mut f, "a.md", &note(3), None); // its own cache file missing
        view(&mut f, true, (0, 10));
        let (asked, marks) = run(&mut f, 0.0);
        assert!(asked.is_empty() && marks == 3, "after a restart, with no cache file of the note's own: {asked:?}, {marks} marks");
        // results made by other models are not used
        let mut g = engine(100);
        g.handle(ToWorker::Server { running: true, model: "other".into(), claims_model: String::new(), has_claims: false, max_paras: 2 }, 0.0);
        let bytes = e.saves().into_iter().chain(f.saves()).find_map(|m| match m {
            FromWorker::Save { file, bytes } if file == SHARED_FILE => Some(bytes),
            _ => None,
        });
        answer_shared(&mut g, bytes.or(shared_copy));
        open(&mut g, "a.md", &note(3), None);
        view(&mut g, true, (0, 10));
        assert_eq!(serve(&mut g, 0.0).len(), 3, "another model: sent again");
    }

    #[test]
    fn asks_only_around_the_view() {
        let mut e = engine(1);
        open(&mut e, "a.md", &note(10), None);
        view(&mut e, false, (10, 10)); // Reading view: line 10 = paragraph 5, and one paragraph either side
        let mut asked = serve(&mut e, 0.0);
        asked.sort();
        assert_eq!(asked, vec![4, 5, 6]);
    }

    /// The paragraphs asked for in one request, if one goes now (the request left unanswered).
    fn ask(e: &mut Engine, now: f64) -> Option<Vec<usize>> {
        e.outbox(now).into_iter().find_map(|m| match m {
            FromWorker::Fetch { req, body, .. } => {
                let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                let ids: Vec<&str> = v["paras"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
                Some((req, ids.iter().map(|id| e.notes["a.md"].paras.iter().position(|q| format!("{:016x}", q.id) == *id).unwrap()).collect()))
            }
            _ => None,
        }).map(|(req, asked): (u32, Vec<usize>)| {
            e.handle(ToWorker::End { req, ok: true }, now); // answered empty: the paragraphs stay wanted, the layer is free
            asked
        })
    }

    /// Every request in the outbox, answered by `reply(endpoint, body)` (the lines for its paragraphs): the requests.
    fn answer(e: &mut Engine, reply: impl Fn(&str, &serde_json::Value) -> String) -> Vec<(String, serde_json::Value)> {
        let mut seen = Vec::new();
        loop {
            let fetches: Vec<_> = e.outbox(0.0).into_iter().filter_map(|m| match m {
                FromWorker::Fetch { req, endpoint, body, .. } => Some((req, endpoint, serde_json::from_str::<serde_json::Value>(&body).unwrap())),
                _ => None,
            }).collect();
            if fetches.is_empty() {
                return seen;
            }
            for (req, endpoint, body) in fetches {
                e.handle(ToWorker::Chunk { req, text: reply(&endpoint, &body) }, 0.0);
                e.handle(ToWorker::End { req, ok: true }, 0.0);
                seen.push((endpoint, body));
            }
        }
    }

    #[test]
    fn sentence_colours_are_asked_with_each_note_s_code_and_rules_win_over_them() {
        let mut s = Settings { context_paragraphs: 0, terms: false, sensitivity: 1.0, ..Default::default() };
        colours::link(&mut s.links, "a.md", "reading", false);
        colours::link(&mut s.links, "b.md", "methods", false);
        let mut e = Engine::default();
        e.handle(ToWorker::Settings(s.clone()), 0.0);
        e.handle(ToWorker::Server { running: true, model: "m".into(), claims_model: "q".into(), has_claims: true, max_paras: 8 }, 0.0);
        let text = "Detectors fail on new data [@a2020]. As @b2021 put it, \"a quoted bit\" follows.";
        open(&mut e, "a.md", text, None);
        open(&mut e, "b.md", text, None);
        for (id, path) in [(1, "a.md"), (2, "b.md")] {
            e.handle(ToWorker::View { id, path: path.into(), editor: true, vis: (0, 10), render: (0, 100_000) }, -PREFETCH_MS);
        }
        let quote = text.find('"').unwrap() as u32;
        let reply = |endpoint: &str, body: &serde_json::Value| -> String {
            body["paras"].as_array().unwrap().iter().map(|p| {
                let id = p["id"].as_str().unwrap();
                let spans = if endpoint == "/colours" {
                    format!(r#"[{{"k":"colour","c":0,"w":0.95,"at":[[0,36]],"v":""}},{{"k":"colour","c":3,"w":0.3,"at":[[37,{}]],"v":""}}]"#, text.len())
                } else {
                    format!(r#"[{{"k":"quote","w":1,"at":[[{quote},{}]],"v":""}}]"#, quote + 13)
                };
                format!("{{\"id\":\"{id}\",\"spans\":{spans}}}\n")
            }).collect()
        };
        let asked = answer(&mut e, reply);
        let colours: Vec<&serde_json::Value> = asked.iter().filter(|(ep, _)| ep == "/colours").map(|(_, b)| b).collect();
        assert_eq!(colours.len(), 2, "one request per code");
        let names = |b: &serde_json::Value| b["colours"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(colours[0]).len() + names(colours[1]).len(), 6 + 4, "each code's clause colours");
        let phrases = |b: &serde_json::Value| b.get("phrases").and_then(|p| p.as_array()).map_or(0, |p| p.len());
        assert_eq!(phrases(colours[0]) + phrases(colours[1]), 4, "the Methodology code's phrase colours, which no rule finds");
        assert!(colours.iter().all(|b| b.get("stress").is_none()));
        assert_ne!(colours[0]["paras"][0]["id"], colours[1]["paras"][0]["id"], "the same paragraph is kept apart per code");
        let marks = |e: &mut Engine| e.outbox(0.0).into_iter().find_map(|m| match m {
            FromWorker::Marks { view: 1, marks, .. } => Some(marks),
            _ => None,
        }).unwrap();
        e.dirty_views.insert(1);
        let m = marks(&mut e);
        let at = |k: u8| m.iter().filter(|r| r.2 == k).map(|r| (r.0, r.1)).collect::<Vec<_>>();
        assert_eq!(at(pick::COLOUR), [(0, 36)]);
        let (cite, quoted) = (text.find("[@").unwrap() as u32, (quote, quote + 13));
        assert_eq!(at(pick::CITATION), [(cite, cite + 8), (40, 46)], "citations by rule");
        assert_eq!(at(pick::QUOTE), [quoted]);
        assert_eq!(at(pick::COLOUR + 3), [(37, quote), (quote + 13, text.len() as u32)], "the quotation wins over the sentence's colour");
        // the slider: at 50% only the surer colour
        e.handle(ToWorker::Settings(Settings { sensitivity: 0.5, ..s.clone() }), 0.0);
        let m = marks(&mut e);
        assert!(m.iter().any(|r| r.2 == pick::COLOUR) && !m.iter().any(|r| r.2 == pick::COLOUR + 3));
        // a description changed: only that code's notes are asked again
        let mut t = s.clone();
        t.colour_codes[0].colours[0].description = "Something else.".into();
        e.handle(ToWorker::Settings(t), 0.0);
        let again = answer(&mut e, reply);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].1["colours"][0]["description"], "Something else.");
    }

    #[test]
    fn a_fast_scroll_sends_nothing_until_the_view_rests() {
        let mut e = engine(1);
        open(&mut e, "a.md", &note(10), None);
        for (t, line) in [(0.0, 2), (50.0, 8), (100.0, 14)] {
            e.handle(ToWorker::View { id: 1, path: "a.md".into(), editor: false, vis: (line, line), render: (0, 100_000) }, t);
            assert_eq!(ask(&mut e, t), None, "scrolling");
        }
        assert_eq!(e.wait_ms(120.0), 100.0 + SETTLE_MS - 120.0, "woken when the view has rested");
        assert_eq!(ask(&mut e, 100.0 + SETTLE_MS), Some(vec![7]), "only where it stopped, and only what is on screen");
    }

    #[test]
    fn the_screen_first_then_the_context_when_idle_two_at_a_time() {
        let mut e = engine(3);
        e.handle(ToWorker::Server { running: true, model: "m".into(), claims_model: String::new(), has_claims: false, max_paras: 64 }, 0.0);
        open(&mut e, "a.md", &note(12), None);
        e.handle(ToWorker::View { id: 1, path: "a.md".into(), editor: false, vis: (10, 12), render: (0, 100_000) }, 0.0);
        let t = SETTLE_MS;
        let first = e.outbox(t).into_iter().find_map(|m| match m {
            FromWorker::Fetch { req, paras, .. } => Some((req, paras)),
            _ => None,
        });
        let Some((req, 2)) = first else { panic!("the two paragraphs on screen, alone: {first:?}") };
        assert_eq!(ask(&mut e, t), None, "nothing more while that request runs");
        e.handle(ToWorker::End { req, ok: true }, t);
        // the screen's paragraphs came back without results, so they are still wanted: nothing ahead yet
        assert_eq!(ask(&mut e, PREFETCH_MS - 1.0), Some(vec![5, 6]));
        let id = |i: usize| e.notes["a.md"].paras[i].id;
        let (five, six) = (id(5), id(6));
        e.store(FAST, five, vec![]);
        e.store(FAST, six, vec![]);
        assert_eq!(ask(&mut e, PREFETCH_MS - 1.0), None, "the context waits until the view has rested");
        assert_eq!(ask(&mut e, PREFETCH_MS), Some(vec![4, 7]), "then the nearest, two at a time");
    }

    /// The marks the view gets, as text, and whether anything was asked of the server.
    fn drawn(e: &mut Engine, text: &str) -> (Vec<String>, bool) {
        let utf: Vec<u16> = text.encode_utf16().collect();
        let out = e.outbox(0.0);
        let asked = out.iter().any(|m| matches!(m, FromWorker::Fetch { .. }));
        let marks = out.into_iter().filter_map(|m| match m {
            FromWorker::Marks { marks, .. } => Some(marks),
            _ => None,
        });
        let words = marks.last().unwrap_or_default().iter().map(|&(a, b, k)| format!("{k}:{}", String::from_utf16_lossy(&utf[a as usize..b as usize]))).collect();
        (words, asked)
    }

    #[test]
    fn own_terms_drawn_at_once_everywhere_and_never_sent() {
        let mut e = engine(100);
        let text = "Paragraph session ends.\n\nParagraph [[session]] link, then Session two and `session` code.";
        open(&mut e, "a.md", text, None);
        view(&mut e, true, (0, 10));
        serve(&mut e, 0.0); // every paragraph's results are in
        let set = |e: &mut Engine, bold: &str, sensitivity: f32, terms: bool| {
            let s = Settings { bold_terms: bold.into(), sensitivity, terms, points: false, context_paragraphs: 100, ..Default::default() };
            e.handle(ToWorker::Settings(s), 0.0);
        };
        set(&mut e, "session", 0.0, true); // whatever the sensitivity
        assert_eq!(drawn(&mut e, text), (vec!["0:session".into(), "0:Session".into()], false), "every occurrence outside links and code; nothing sent");
        set(&mut e, "session", 0.0, false);
        assert_eq!(drawn(&mut e, text).0, Vec::<String>::new(), "not while key terms are off");
        set(&mut e, "", 0.0, true);
        assert_eq!(drawn(&mut e, text), (vec![], false), "taken out: gone at once");
        // with the server's own terms (each paragraph's first word): both
        set(&mut e, "session", 0.5, true);
        assert_eq!(drawn(&mut e, text).0, ["0:Paragraph", "0:session", "0:Paragraph", "0:Session"]);
    }

    #[test]
    fn a_colour_s_own_rules_show_at_once_in_both_views_and_never_ask_the_server() {
        use crate::colours::{Action, Rule, RuleKind};
        let mut s = Settings { context_paragraphs: 0, sensitivity: 1.0, ..Default::default() };
        colours::link(&mut s.links, "a.md", "reading", false);
        let mut e = Engine::default();
        e.handle(ToWorker::Settings(s.clone()), 0.0);
        e.handle(ToWorker::Server { running: false, model: "m".into(), claims_model: "q".into(), has_claims: true, max_paras: 8 }, 0.0);
        let text = "Detectors fail. Second, no study has tried.";
        open(&mut e, "a.md", text, None);
        e.handle(ToWorker::View { id: 1, path: "a.md".into(), editor: true, vis: (0, 10), render: (0, 100_000) }, -PREFETCH_MS);
        assert_eq!(drawn(&mut e, text).0, Vec::<String>::new());
        // a Force rule for Limitations (the code's sixth colour)
        let mut t = s.clone();
        t.colour_codes[0].colours[5].rules.push(Rule { kind: RuleKind::Pattern, action: Action::Force, text: "^(First|Second), no\\b".into(), on: true });
        e.handle(ToWorker::Settings(t.clone()), 0.0);
        let limitations = pick::COLOUR + 5;
        assert_eq!(drawn(&mut e, text), (vec![format!("{limitations}:Second, no study has tried.")], false), "drawn at once, nothing sent");
        e.handle(ToWorker::Block { id: 7, path: "a.md".into(), lines: Some((0, 0)), idx: 0, n: 1, text: text.into() }, 0.0);
        assert_eq!(e.wrap(7), vec![(16, 43, 1 << limitations)], "Reading view too");
        // switched off: gone at once
        t.colour_codes[0].colours[5].rules[0].on = false;
        e.handle(ToWorker::Settings(t), 0.0);
        assert_eq!(drawn(&mut e, text).0, Vec::<String>::new());
    }

    #[test]
    fn own_terms_in_reading_view_and_before_any_result() {
        let mut e = engine(100);
        e.handle(ToWorker::Server { running: false, model: "m".into(), claims_model: String::new(), has_claims: false, max_paras: 2 }, 0.0);
        e.handle(ToWorker::Settings(Settings { bold_terms: "network intrusion".into(), ..Default::default() }), 0.0);
        open(&mut e, "a.md", "A network intrusion, a [[x|network intrusion]], and a **network** intrusion.", None);
        let block = "A network intrusion, a network intrusion, and a network intrusion.";
        e.handle(ToWorker::Block { id: 7, path: "a.md".into(), lines: Some((0, 0)), idx: 0, n: 1, text: block.into() }, 0.0);
        let got: Vec<&str> = e.wrap(7).iter().map(|&(a, b, _)| &block[a as usize..b as usize]).collect();
        assert_eq!(got, ["network intrusion", "network intrusion"], "the first and the last; not the link's words");
    }

    #[test]
    fn failure_backs_off() {
        let mut e = engine(100);
        open(&mut e, "a.md", &note(1), None);
        view(&mut e, true, (0, 5));
        let Some(FromWorker::Fetch { req, .. }) = e.outbox(0.0).into_iter().find(|m| matches!(m, FromWorker::Fetch { .. })) else { panic!() };
        e.handle(ToWorker::End { req, ok: false }, 0.0);
        assert!(serve(&mut e, 500.0).is_empty());
        assert!(e.wait_ms(500.0) > 0.0);
        assert_eq!(serve(&mut e, 1000.0), vec![0]);
    }

    #[test]
    fn other_models_results_are_ignored() {
        let mut e = engine(100);
        open(&mut e, "a.md", &note(2), None);
        view(&mut e, true, (0, 5));
        serve(&mut e, 0.0);
        let Some(FromWorker::Save { bytes, .. }) = e.saves().pop() else { panic!() };
        let mut f = Engine::default();
        f.handle(ToWorker::Server { running: true, model: "other".into(), claims_model: String::new(), has_claims: false, max_paras: 2 }, 0.0);
        open(&mut f, "a.md", &note(2), Some(bytes));
        view(&mut f, true, (0, 5));
        assert_eq!(run(&mut f, 0.0).0.len(), 2, "another model's results are not shown: both paragraphs are asked for");
    }

    #[test]
    fn reading_block_maps_to_its_paragraph() {
        let mut e = engine(100);
        open(&mut e, "a.md", "Intro words here.\n\nThe **second** paragraph, with [[x|a link]].", None);
        let id = e.notes["a.md"].paras[1].id;
        e.store(FAST, id, vec![Span { k: pick::TERM, w: 0.9, at: vec![(4, 14)], v: "second".into() }]);
        let text = "The second paragraph, with a link.";
        e.handle(ToWorker::Block { id: 7, path: "a.md".into(), lines: Some((2, 2)), idx: 0, n: 1, text: text.into() }, 0.0);
        e.handle(ToWorker::Block { id: 8, path: "a.md".into(), lines: None, idx: 0, n: 1, text: text.into() }, 0.0); // by its words
        assert_eq!(e.wrap(7), vec![(4, 10, 1)]);
        assert_eq!(e.wrap(8), vec![(4, 10, 1)]);
        // a citation keeps its text in Reading view, so it is found in the block
        let cited = "The second paragraph, with a link [@a2020, p. 3].";
        e.handle(ToWorker::Block { id: 9, path: "a.md".into(), lines: Some((2, 2)), idx: 0, n: 1, text: cited.into() }, 0.0);
        assert_eq!(e.wrap(9), vec![(4, 10, 1), (34, 48, 1 << pick::CITATION)]);
    }
}
