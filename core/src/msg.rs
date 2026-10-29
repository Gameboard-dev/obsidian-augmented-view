//! The plugin's settings, and the messages between the main thread and the worker (postcard, sent as transferred
//! bytes: one shared definition, nothing to keep in step by hand). Encoding: <https://docs.rs/postcard>.

use serde::{Deserialize, Serialize};

/// The plugin's settings, as Obsidian stores them in `data.json` (<https://docs.obsidian.md/Reference/TypeScript+API/Plugin/loadData>; camelCase keys,
/// <https://serde.rs/container-attrs.html#rename_all>; missing ones take the defaults).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Not used: the server's executables are this device's (`Local`), never the vault's (data.json is shared with
    /// every device and Ignis). Read once to move an older data.json's values to the device, then kept empty.
    pub server: String,
    pub cpu_server: String,
    pub port: u16,
    /// Let the server use the GPU (CUDA or Metal) when it has one with room; off: the CPU only.
    pub use_gpu: bool,
    /// The most of the time the GPU may work for the server, in percent (it rests in between).
    pub gpu_limit: u32,
    /// Warn when the GPU's load or memory, or the computer's memory, is at or above this percent.
    pub warn_percent: u32,
    pub model: String,
    pub idle_minutes: f64,
    pub auto_start: bool,
    pub enabled: bool,
    pub sensitivity: f32,
    pub diversity: f32,
    pub terms: bool,
    pub contrast: bool,
    /// Sentence colours (the model's; the main and critical points before 2026-10-25).
    pub points: bool,
    pub quotes: bool,
    pub definitions: bool,
    pub every_occurrence: bool,
    /// The user's own key terms, drawn bold wherever they appear (bold.rs): separated by commas (or new lines).
    pub bold_terms: String,
    pub context_paragraphs: u32,
    pub debug_log: bool,
    /// The models the server last reported: cached results show (and are checked against them) while it is down.
    pub last_model: String,
    pub last_claims_model: String,
    pub last_has_claims: bool,
    /// The keys of colours (colours.rs).
    pub colour_codes: Vec<crate::colours::ColourCode>,
    /// Which code applies to which notes and folders (colours::assigned).
    pub links: Vec<crate::colours::Link>,
    /// The command "Export this note with its colours" (export.rs; docs/colour-codes.md): off by default.
    pub export_colours: bool,
    /// The markers: "tilde" (`~={colour, category}text=~`) or "html" (`<mark data-colour data-category>`).
    pub export_format: String,
    /// Phrase colours too, not only sentences'.
    pub export_phrases: bool,
    /// Where exports go: a vault-relative folder; empty: next to the note.
    pub export_folder: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            server: String::new(),
            cpu_server: String::new(),
            port: 8790,
            use_gpu: true,
            gpu_limit: 100,
            warn_percent: 90,
            model: "BAAI/bge-base-en-v1.5".into(),
            idle_minutes: 30.0,
            auto_start: false,
            enabled: true,
            sensitivity: 0.5,
            diversity: 0.4,
            terms: true,
            contrast: true,
            points: true,
            quotes: true,
            definitions: true,
            every_occurrence: false,
            bold_terms: String::new(),
            context_paragraphs: 1,
            debug_log: false,
            last_model: String::new(),
            last_claims_model: String::new(),
            last_has_claims: false,
            colour_codes: crate::colours::defaults(),
            links: Vec::new(),
            export_colours: false,
            export_format: "tilde".into(),
            export_phrases: false,
            export_folder: String::new(),
        }
    }
}

/// Settings of this device only, kept in its own storage rather than data.json (which a synced vault shares between
/// devices): `App.loadLocalStorage` (<https://docs.obsidian.md/Reference/TypeScript+API/App/loadLocalStorage>).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Local {
    /// A server elsewhere (for a device that cannot start one, or through a relay); empty: this computer's, on `port`.
    pub server_url: String,
    /// The model server's executable; empty: `augmented-view-server` in the plugin's folder.
    pub server: String,
    /// A CPU-only build, started when the first cannot start at all (a CUDA build on Linux without CUDA's libraries);
    /// empty: `augmented-view-server-cpu` in the plugin's folder, if there.
    pub cpu_server: String,
    /// Paragraphs per request: fewer show sooner where a proxy passes answers on only when complete.
    pub max_paras: u32,
    /// The floating key's place (pixels from the window's top left; negative: the default corner) and whether it is folded.
    pub key_x: f64,
    pub key_y: f64,
    pub key_folded: bool,
    /// The key's size as the user left it (pixels; 0: the stylesheet's).
    pub key_w: f64,
    pub key_h: f64,
}

impl Default for Local {
    fn default() -> Self {
        Local { server_url: String::new(), server: String::new(), cpu_server: String::new(), max_paras: 64, key_x: -1.0, key_y: -1.0, key_folded: false, key_w: 0.0, key_h: 0.0 }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ToWorker {
    Settings(Settings),
    Server { running: bool, model: String, claims_model: String, has_claims: bool, max_paras: u32 },
    /// A note's text, from an editor or the file; `version` comes back with its marks.
    Doc { path: String, version: u32, text: String },
    /// An editor (visible and rendered ranges in UTF-16 offsets) or a Reading view (visible lines).
    View { id: u32, path: String, editor: bool, vis: (u32, u32), render: (u32, u32) },
    DropView(u32),
    /// A rendered Reading-view block: its section's source lines, its place among the section's blocks, its text.
    Block { id: u32, path: String, lines: Option<(u32, u32)>, idx: u32, n: u32, text: String },
    DropBlock(u32),
    Loaded { path: String, bytes: Option<Vec<u8>> },
    Chunk { req: u32, text: String },
    End { req: u32, ok: bool },
    Rename { from: String, to: String },
    /// Export a note (sent after its `Doc`) with its colours, computing those it lacks first; `header` opens it.
    Export { req: u32, path: String, header: String },
    /// Finish an export now with the colours there are (the server cannot answer).
    ExportNow(u32),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FromWorker {
    Fetch { req: u32, endpoint: String, body: String, paras: u32 },
    Load { path: String, file: String },
    Save { file: String, bytes: Vec<u8> },
    RenameFile { from: String, to: String },
    /// [from, to, kind] in the note's UTF-16 offsets, for the editor's rendered range.
    Marks { view: u32, version: u32, marks: Vec<(u32, u32, u8)> },
    /// [start, end, mask of kinds] in the block's text.
    Wrap { block: u32, segs: Vec<(u32, u32, u32)> },
    /// An export waits for the colours of `missing` paragraphs.
    ExportWaiting { req: u32, missing: u32 },
    /// An export's text, the markers in it, and the paragraphs still without colours; `ok` false: no such note.
    Exported { req: u32, text: String, markers: u32, missing: u32, ok: bool },
}

pub fn encode<T: Serialize>(m: &T) -> Vec<u8> {
    postcard::to_allocvec(m).unwrap_or_default()
}

pub fn decode<'a, T: Deserialize<'a>>(b: &'a [u8]) -> Option<T> {
    postcard::from_bytes(b).ok()
}
