# Augmented View

Colours the paragraphs of a note for skim reading: each sentence in the colour of the role it plays (a claim, a finding, a limitation, a signpost …) and key phrases in theirs (terms, definitions, citations, methods), by local models running on your own computer. Works in Live Preview, Source mode and Reading view. The note is never changed: the colours are drawn over the text.

The colours come from a **colour code**, a list of colours you can edit, each with a name, a hex colour and a description of what it marks: the models read the descriptions. Two codes come with the plugin (an academic taxonomy for literature notes and one for methodology notes); a note, or a folder, is linked to a code from the floating key, and a note with no code is never sent anywhere.

## Install

1. Install the plugin from Obsidian's community plugins (or copy `main.js`, `manifest.json` and `styles.css` from a [release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases) into `<vault>/.obsidian/plugins/augmented-view/`).
2. Download the **model server** for your computer from the same [release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases) and put it in the plugin's folder (or anywhere, and set its path in the settings):
   - Windows with an NVIDIA GPU: `augmented-view-server.exe` (CUDA; falls back to the CPU by itself when CUDA is missing);
   - Windows without one: `augmented-view-server-cpu.exe`, as `augmented-view-server.exe` or as the *CPU server* setting;
   - macOS and Linux: build it from `server/` (Build and deploy, below); these builds are written for but not yet tested.
   Save it as `augmented-view-server.exe` (the release names it `augmented-view-server-windows-x64.exe`; its `.sha256` file lets you check it), or keep the name and set the *Server* setting to it. Keep the two licence files of the release (`augmented-view-server-LICENSE.txt`, `augmented-view-server-THIRD-PARTY-LICENSES.txt`) with it.
   The plugin does not download, install or update the server itself.
3. **Download the models.** The plugin never downloads anything: you run the server's download command yourself, in a terminal. Click the status bar's "AugView" item and choose *Start the server*. While a model is missing the server does not start; a notice says which models are missing and how large they are, and its button **Show the models** opens Settings → Augmented View → **Models**. That section lists each model with its purpose, size, licence, a link to its page (opened in your browser), the folder it goes in and its files, and below them the download command for your computer, with **Copy**:

   ```
   & "<vault>\.obsidian\plugins\augmented-view\augmented-view-server.exe" --download all      # Windows, in PowerShell
   "<vault>/.obsidian/plugins/augmented-view/augmented-view-server" --download all            # macOS and Linux
   ```

   Run it in a terminal (the section shows it with your server's real path; it adds `--phrases` when a colour code has phrase colours, and `--model` for a key-term model set by hand). The server program downloads the missing files (about 8.5 GB once, about 2.9 GB kept on disk; 0.84 GB more for phrase colours), checks each against its published size and checksum, and prints its progress. Ctrl+C stops it; running it again resumes where it stopped. To download only some models, name them: `--download terms,phrases` (the ids are `parser`, `terms`, `colours`, `phrases`). Then press **Check again** in the section (or start the server from the status bar). You can also download the files by hand from each model's page into the folder the section shows.
4. Open a note, choose a colour code in the floating key, and read.

Desktop only (the server is a separate program). About 4 GB of GPU memory (or 5 GB of the computer's memory on the CPU, where it is much slower) while the server runs; it stops by itself after 30 idle minutes.

## Disclosures and compliance

Augmented View is written to meet Obsidian's [Developer policies](https://docs.obsidian.md/Developer+policies), the [Submission requirements for plugins](https://docs.obsidian.md/Plugins/Releasing/Submission+requirements+for+plugins) and the [Plugin guidelines](https://docs.obsidian.md/Plugins/Releasing/Plugin+guidelines). The disclosures the policies ask for, in their order:

- **Payment:** none. The plugin, the server and the models are free.
- **Account:** none. No sign-in, no API key.
- **Network use:**
  - *The plugin* connects only to its model server on this computer, `http://127.0.0.1:8790` (the port is a setting), to send the paragraphs on screen and receive their colours. On a device where you set *Server address*, it connects to that address instead, and to nothing else. It makes no other network requests.
  - *The plugin never downloads anything; the server downloads its models only when you run its download command* (`augmented-view-server --download all`, in a terminal: Install, step 3). The plugin shows that command, with Copy, and never runs it; started by the plugin, the server never downloads, and when a model is missing it says which and exits. The sources: Hugging Face (`huggingface.co`) for BAAI/bge-base-en-v1.5 (key terms, 0.44 GB), Qwen/Qwen3-4B-Instruct-2507 (sentence colours, 8.06 GB, converted once to a 2.4 GB 4-bit file) and, if a colour code has phrase colours, fastino/gliner2-base-v1 (0.84 GB); LINDAT (`lindat.mff.cuni.cz`) for the UDPipe English parser model (16 MB). These are needed because the colours come from these models, which are too large to ship with the plugin. Once they are on disk the server needs no network.
  - *On this computer only*, when the GPU has too little free memory and you choose to free it, the server asks your other local model servers that use the same mechanism (on `127.0.0.1`) to release their GPU memory.
  - Your notes are never sent off your computer (unless you point *Server address* at another machine yourself).
- **Files outside the vault:**
  - *The server program*, which you download and place yourself (by default in the plugin's folder, inside the vault).
  - *Model files*, in the server's cache folder: Windows `%LOCALAPPDATA%\augmented-view`, macOS `~/Library/Caches/augmented-view`, Linux `$XDG_CACHE_HOME/augmented-view` (by default `~/.cache/augmented-view`). Delete the folder to remove them.
  - *The Hugging Face cache* (`$HF_HUB_CACHE`, `$HF_HOME/hub` or `~/.cache/huggingface/hub`): read only, so that models you already have are not downloaded again.
  - *GPU claims*: one small JSON file while the server holds GPU memory (its port, process id and the memory it holds), removed when it stops: Windows `%LOCALAPPDATA%\obsidian-kit\gpu`, Linux and macOS `$XDG_DATA_HOME/obsidian-kit/gpu` (by default `~/.local/share/obsidian-kit/gpu`). It lets the user's GPU services see which of them holds memory.
  - The plugin itself writes only inside the vault's plugin folder (its settings, its per-note cache, an optional debug log) and Obsidian's local storage (device-only settings). It starts the server as a child process.
- **Ads:** none.
- **Telemetry:** none, client-side or server-side. Nothing is collected or sent anywhere; the readings in the status bar (GPU and memory load) stay on your computer.
- **Source code:** open source, under the MIT licence (`LICENSE`). The plugin and the server are built from the Rust source in this repository and the shared crates and build tool in [obsidian-cargo-lib](https://github.com/Gameboard-dev/obsidian-cargo-lib) (Build and deploy, below).

| Rule | How Augmented View meets it |
|---|---|
| No self-install or self-update | The plugin never downloads, installs or updates anything, and never starts a download. You download the server from the [releases page](https://github.com/Gameboard-dev/obsidian-augmented-view/releases) and put it in place yourself. The server downloads its models only when you run its download command in a terminal (`augmented-view-server --download all`); the plugin shows that command with a Copy button and never runs it (the arguments it starts the server with never include `--download`, which a test checks). |
| No obfuscation | `main.js` is a short loader; the plugin's code is a WebAssembly module (inlined in `main.js` in a release) compiled from the Rust source here, with its JavaScript bindings generated by wasm-bindgen and kept readable. Build instructions are below. |
| No client-side telemetry | None. The plugin contacts only its own server. |
| No ads | None. |
| Disclosures | Network use and files outside the vault, above. |
| Licence and attribution | `LICENSE` (MIT). Each server release carries `augmented-view-server-THIRD-PARTY-LICENSES.txt` (the crates' licences and the native code compiled in, generated by `licences/generate.py`); `pack --release` refuses to release the server without it. The models keep their own licences (below). |
| Desktop only | `isDesktopOnly` is `true`: the plugin starts the server with Node's `child_process`, available only on desktop, and checks for desktop Obsidian before doing so. |
| Command ids | Without the plugin id (`toggle-server`, `toggle-emphasis`, `more`, `less`); no default hotkeys. |
| Security | No `innerHTML`, `outerHTML` or `insertAdjacentHTML`; text is set with `textContent` and the DOM API. No `eval`; in a release the bindings are part of `main.js`. The worker runs the same bundled bindings from a Blob URL, never code from elsewhere. |
| Settings | Headings with `setHeading`, in sentence case, without the word "settings". |
| Resources | On unload the plugin stops its worker, its requests and a server it started, and removes its stylesheet and floating key; it never detaches leaves. |
| `this.app` | The plugin uses the plugin's own `app`, never the global one. |

**Licences of the models** (not distributed with the plugin or the server; downloaded only by the download command you run): BAAI/bge-base-en-v1.5 MIT; Qwen/Qwen3-4B-Instruct-2507 Apache-2.0; fastino/gliner2-base-v1 Apache-2.0; the UDPipe 2.5 English EWT model CC BY-NC-SA 4.0 (personal and non-commercial use only).

## What it marks

What is found, and by what (each colour of the code says which it shows; COLOUR-CODES.md has the design):

| Kind | Default colour | Found by (server) |
|---|---|---|
| Key terms | Key terms and definitions (yellow, phrase) | noun phrases from UDPipe's parse, ranked by their similarity to the paragraph (an embedding model, KeyBERT's method) plus how specific their words are (how rarely English uses them: wordfreq's word frequencies, CC BY-SA 4.0, in `server/data/`) and how often the paragraph repeats them; also a word or two quoted to be talked about ("positivism"); never a cited author's name |
| Contrast words | none | the instruct model below, asked which single words the paragraph's meaning turns on ("only", "never", "external") |
| Sentence colours | every clause colour (Claims, Signposts, Findings, Evidence and context, Limitations, Connections) | a local instruct model (Qwen3-4B-Instruct, 4-bit) choosing each sentence's colour from the colours' descriptions (`/colours`) |
| Phrase colours | the Methods code's phrase colours (frameworks, analysis, data, design type) | GLiNER2 (fastino/gliner2-base-v1) finding the phrases each colour's description fits, on the CPU (server/src/phrases.rs) |
| Citations | Peripheral citations (grey, phrase) | rule, in the worker (core/src/rules.rs): `[@key, p. 3]` and `@key`, outside code; in Reading view too, where a citation keeps its text |
| Quotations | Raw evidence and context (blue, clause) | rule: words between quotation marks, outside code, links and citations |
| Definitions | Key terms and definitions (yellow, phrase) | rules on the parse (server/src/meaning.rs): a quotation presented as a definition ("in ISO's definition", "defined as", after the term and a comma, after "is a"), a gloss ("an adversarial attack, a change to the pixels …"), "X is a Y that …", what follows "means", "denotes" or "refers to", an abbreviation's clause ("FAR counts attacks accepted"), and a naming ("Choosing the range … is called calibration": the clause underlined, the name bold) |

A clause colour is a pale background; a phrase colour is drawn over it, so a key term inside a claim still shows. Each colour has its own opacity. Light themes use the colour as given, dark themes a deeper version of the same hue (or the colour's own dark-theme colour), so light text stays readable. The colours are one stylesheet generated from the settings (`core/src/colours.rs`), so changing one never asks the worker or the server.

The note is never changed: the emphasis is drawn as CodeMirror decorations (editor) or wrapper spans (Reading view). Everything runs on your computer; nothing leaves it.

## Layout

Everything is Rust except `main.js` (18 lines, the only JavaScript), which loads `augmented_view.wasm` and hands it Obsidian; the .wasm carries its own wasm-bindgen bindings in a custom section named `glue`, so nothing else is shipped or compiled from JavaScript.

- `core/src/`: the plugin crate. On Obsidian's main thread: `app.rs` (settings, status bar, menu, commands, settings tab, the server's life cycle and readings, fetch and file I/O), `editor.rs` (the CodeMirror view plugin), `view.rs` (Reading view), `models.rs` (the notice when models are missing, and the Models section of the settings, with the download command to copy; it starts nothing). In a Web Worker started from the same module: `engine.rs` (notes, cache, request planning), with `split.rs` (paragraphs), `pick.rs` (the sensitivity rule), `bold.rs` (the user's own terms, *Always mark*), `reading.rs` (finding spans in rendered text), `cache.rs` (per-note cache files). `msg.rs` holds the settings and the messages between the two (postcard). The helpers through which Obsidian's and CodeMirror's JavaScript APIs are called by name (`call!`, `obj!`) and the settings-tab builder are in the shared crate, `obsidian-plugins/shared` (obsidian-kit), which Dictionary and Synonyms uses too.
- `obsidian-plugins/pack/`: the build, shared by the plugins in `obsidian-plugins/`: compiles to WebAssembly, runs wasm-bindgen, stores the bindings in the .wasm, builds the server, copies the plugin into a vault.
- `server/`: the model server, a native Rust program (its own cargo workspace; `src/main.rs` describes the protocol). `text.rs` blanks markup and aligns UDPipe's parse to the text; `rules.rs` finds terms and clauses and ranks them; `meaning.rs` finds quotations and definitions; `embed.rs` runs the embedding model; `claims.rs` the instruct model; `phrases.rs` GLiNER2 for the phrase colours; `models.rs` the models' manifest, the start check and the download command (`--download`); the hardware (each model's device and fallbacks, the readings, GPU pacing) is the shared crate `obsidian-plugins/native-kit`, which Quarry's service uses too; `models.rs` finds or downloads model files; `build.rs` makes the Windows CUDA build start without CUDA.

Dependencies are the official rustwasm crates (wasm-bindgen, wasm-bindgen-futures, js-sys) and serde, serde_json, serde-wasm-bindgen, postcard, xxhash-rust; no web-sys (the DOM is reached by name too) and no JavaScript tooling. No maintained Obsidian binding crate exists (magnetite-lib was archived in 2024; trashhalo/obsidian-rust-plugin is a template that still needs yarn and rollup).

## Build and deploy

This repository is one plugin of a Rust workspace: clone [obsidian-cargo-lib](https://github.com/Gameboard-dev/obsidian-cargo-lib) (the shared crate and the build tool, pack), then clone this repository into it as `augmented-view`, and build from the obsidian-kit folder. The release assets are the four files to copy into `<vault>/.obsidian/plugins/augmented-view/`.

```bash
git clone https://github.com/Gameboard-dev/obsidian-cargo-lib
git clone https://github.com/Gameboard-dev/obsidian-augmented-view obsidian-cargo-lib/augmented-view
```

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129      # must match the wasm-bindgen crate (core/Cargo.toml)
cd obsidian-plugins                                   # the workspace of all the plugins
cargo run -p pack --release -- augmented-view --server --deploy <vault>/.obsidian/plugins/augmented-view   # plugin + server
cargo run -p pack --release -- augmented-view --fast --deploy <dir>   # the plugin only, built quickly while working
cargo run -p pack --release -- augmented-view --release <dir>       # the release's main.js (the module inlined), manifest.json, styles.css
cargo run -p pack --release -- augmented-view --cpu-server            # the CPU-only server, into server/target/cpu
cargo test --release -p augmented-view-core              # the plugin's tests (native)
(cd augmented-view/server && cargo test --release)      # the server's tests (masking, clauses, terms; uses the UDPipe model if present)
cargo doc -p augmented-view-core --no-deps --open         # the code's documentation, with links to the APIs it relies on
```

`--deploy` also reads its folder from the environment variable `AUGMENTED_VIEW_DEPLOY`. It copies the plugin only; point the plugin's *Server* setting at the server, or copy it into the plugin's folder as `augmented-view-server` (`.exe` on Windows), and the CPU build as `augmented-view-server-cpu`.

Two build profiles for the plugins (`obsidian-plugins/Cargo.toml`): `release`, the one shipped: `opt-level = 3`, fat LTO, one codegen unit, WebAssembly SIMD (passed by pack); about 10 s after a change. `fast`: the same optimisation level with thin LTO, 16 codegen units and incremental compilation; about 1 s after a change, a 12% larger module. Dependencies are compiled once and kept in `obsidian-plugins/target`, and pack regenerates nothing when the compiled module is unchanged (0.3 s). Measured and left out (2026-10-24): Binaryen's wasm-opt made the module 6% smaller but the engine 5–10% slower.

Then turn the plugin off and on in Obsidian (Settings → Community plugins).

### Building the server

One build per kind of computer (server/Cargo.toml features):

| Computer | Build | GPU readings |
|---|---|---|
| Windows or Linux with an NVIDIA GPU | `cargo build --release` (feature `cuda`, the default) | load, memory, power, temperature (NVML, from the driver) |
| macOS (Apple silicon) | `cargo build --release --no-default-features --features metal` | none: Apple's GPU shares the computer's memory, which is reported |
| Any, CPU only | `cargo build --release --no-default-features` | none |

Every build reports CPU load and memory (the computer's and the server's), and every build falls back to the CPU by itself: a CUDA build without a usable NVIDIA GPU, or with too little GPU memory free, runs that model on the CPU and says why.

- **CUDA's libraries.** candle links cuBLAS and cuRAND when the program loads. On Windows the server delay-loads them (`build.rs`), so it starts without CUDA installed, finds the libraries missing and uses the CPU. On Linux a CUDA build cannot start without them; the plugin then starts the CPU build (*CPU server*).
- **Machine-specific settings** go in a cargo config outside this folder (cargo merges config files from parent folders): this machine's is `obsidian-plugins/.cargo/config.toml`, with `target-cpu=native`, `NVCC_CCBIN` (nvcc needs Microsoft's C++ compiler on Windows), `CUDA_COMPUTE_CAP` (the GPU's compute capability; a build for others should use the lowest it supports, as newer GPUs compile its kernels when they load) and `NVCC_APPEND_FLAGS = "-Xcompiler /Zc:preprocessor"` (CUDA 13 with MSVC). The checked-in `server/.cargo/config.toml` holds only what every build needs (Windows: the C runtime linked statically, as candle's CUDA kernels are). The first CUDA build compiles candle's kernels (several minutes); later ones take about 20 s.

## Performance

- **Nothing that costs time runs on Obsidian's main thread.** Splitting, hashing, planning, the protocol, choosing spans and Reading-view mapping run in the worker (on a 1.6 MB, 5,000-paragraph note: split and hash 10 ms, plan 0.4 ms, marks 0.1 ms). The main thread forwards events, makes the requests, reads and writes cache files, and draws.
- **The editor never scans the note.** An edit maps the existing marks through the change at once; the text goes to the worker only after 400 ms without typing; the visible range at most once a frame; marks come back for the editor's rendered range only.
- **Reading view touches only what is near the screen** (an IntersectionObserver, one screen either side), in animation frames with a 6 ms budget; elements are held weakly and forgotten when Obsidian discards them.
- **What is on screen goes first, and the rest only when the server is idle:**
  - The paragraphs on screen are sent once the view has rested 150 ms, so a fast scroll sends nothing. Up to *Paragraphs per request* (64) go in a request.
  - *Context paragraphs* (default 1) either side follow once the view has rested 1 s, nothing on screen is left to ask for and no request is running. They go nearest first, two to a request, so a scroll never waits behind a long one.
  - One request per layer runs at a time, streamed back paragraph by paragraph. A failing server is retried after 1, 2, 4 … 30 s.
  - Before 2026-10-25 the context (then 5 either side) went in the same request as the screen. Opening a page then sent about 12 paragraphs through the language model at once, keeping the GPU at full power for about 10 s.
- **The server parses in parallel and runs the models one paragraph at a time.** A request's paragraphs are parsed on all CPU cores (one UDPipe model per thread, loaded at start-up) while the models work through them in order, each result sent as soon as it is done. The two layers have their own threads but take turns on the GPU, never loading it together.
- **The language model reads each paragraph's clause listing once per order, not once per question** (the attention cache is reused for both questions on the GPU): two forward passes over the listing instead of four.
- **On the CPU** the embedding model encodes the sentences and the candidate phrases as separate batches, so short phrases are not padded to a sentence's length: 4 paragraphs' key terms in 10.9 s instead of 54 s, the same results as the GPU's (2026-10-24).
- Measured on 8 paragraphs in one request (RTX 4060 Ti, before the changes above): terms and phrases 97 ms a paragraph, points and main point 300 ms (the Python server it replaced: 66 and 323 ms). Start-up about 2.5 s once the models are on disk. Batching the models across paragraphs was measured slower and changed the 4-bit model's results.

## GPU load, memory and warnings

- **GPU load limit** (default 100%): the server rests after each piece of GPU work so that the GPU works for it at most that share of the time (at 50%, as long resting as working). Measured on 4 paragraphs at 100%: the GPU 100% busy for 3.6 s, 111 W on average and 148 W at most of its 160 W limit (idle: 10 W). That full-power burst is what makes some cards' coils whine; a lower limit lowers the average power and heat but makes results slower, and cannot change the card's own sound while it works.
- **Readings**: every 2 s while the server runs, the status bar's tooltip and menu show the GPU's load, memory, power and temperature (NVIDIA; all programs together, as the card reports them), the CPU's load and the computer's memory (all programs), and the server's own memory and CPU share. Where they come from: NVML for the GPU (<https://docs.rs/nvml-wrapper>), sysinfo for the rest (<https://docs.rs/sysinfo>).
- **Warnings**: when the GPU's memory or the computer's memory reaches *Warn at* (default 90%), or the GPU has been that busy for three readings running, a notice says so and what to do, and the status dot turns red. Each warns once until it has fallen 5 points below the level.
- **Memory needed**: about 0.55 GB of GPU memory for the embedding model (measured, with CUDA's own) and about 3.5 GB for the language model (4.1 GB for both, measured); on the CPU about 1.2 GB and 4 GB of the computer's. Before loading each model the server checks the free GPU memory (and the computer's, for the CPU): a model that does not fit on the GPU goes on the CPU, one that fits nowhere is left out (critical points and main point) or stops the server with the reason (key terms). Turning off critical points starts the server without the language model.

## Where it runs, and its fallbacks

The status bar says where the models run: "AugView 50% · CUDA", "· CPU", "· CUDA + CPU" (one model each) or "· Metal". When anything is not on the GPU, a notice says why once per server start, and the tooltip keeps the reasons:

| Situation | What happens |
|---|---|
| CUDA build, NVIDIA GPU with room | both models on CUDA |
| *Use the GPU* off | both on the CPU ("the GPU is turned off in the settings") |
| CUDA's libraries missing (Windows) | the same server runs on the CPU ("CUDA's libraries are not installed (cublas64_13.dll, …)") |
| CUDA's libraries missing (Linux) | the CUDA build cannot start; the plugin starts the CPU build ("… could not start; using the CPU build") |
| No NVIDIA GPU, or the driver fails | the CPU ("CUDA could not start: …") |
| Too little free GPU memory for a model | that model on the CPU ("critical points on the CPU: the GPU has 1200 MB free and needs about 3600 MB") |
| Too little memory anywhere for the language model | the server runs without critical points, and says so |
| Too little memory for the embedding model | the server stops; the notice says how much memory it needs and how much is available |
| No server where the settings point | the plugin says where it looked |

## Cache

One small binary file per note in the plugin's `cache/` folder (named by a hash of the note's path), read only when that note is opened. Results are kept by the hash of each paragraph's text, so:

- **an unchanged paragraph is never sent again**, also after Obsidian restarts;
- **editing one paragraph re-sends only that one**, once it has been unchanged for 1.5 s (typing does not send every draft): the editor reports each change, the worker splits the note into paragraphs and hashes each one, and only a paragraph whose hash it has no result for is sent; editing N paragraphs sends those N;
- **nothing accumulates**: the note's file is rewritten with its current paragraphs' results only, a deleted note's file is removed (and files of notes deleted while Obsidian was closed are removed by name at start-up), and a renamed note's file is renamed;
- results made by other models (or another *Diversity*) are ignored, not shown.

The dictionary that was here (synonyms on select, definitions on Ctrl+select) is now its own plugin, Dictionary and Synonyms (`obsidian-plugins/dictionary-and-synonyms`).

## Use

- **Status bar** ("AugView …"): where the models run, and a menu: Start/Stop the server, the readings, Show/Hide emphasis, the sensitivity levels. Grey dot: stopped; orange: starting or stopping; green: running; red: a reading at or above *Warn at*.
- **Commands**: *Start or stop the server*, *Show or hide emphasis*, *More emphasis*, *Less emphasis*.
- **The floating key**: a small panel over the workspace showing the open note's colour code, each colour as a chip (a thin swatch for a phrase colour) with a check box that shows or hides that colour in the notes (since 2026-10-28 this replaces the switches for key terms, contrast words, quotations, definitions and sentence colours: the colour code decides what is drawn); hover a chip for what it marks. Drag it by its title bar, fold it with the arrow; its place and fold are kept for this device. Its picker links the note (or, with the box ticked, its folder; for a note at the vault's root, the whole vault) to a code or to none. A note with no code is not marked and nothing of it is sent to the server until one is chosen. **+** adds a colour to the code (its colour and name; the rest in the settings). The links are listed in the settings, where they can be changed or removed; a note's own link wins over its folder's, and links follow renamed notes and folders.
- **Always mark as a key term** (the editor's right-click menu, when up to six words on one line are selected): adds them to *Always mark*, so they are marked as a key term in every paragraph, each time they appear, whatever the sensitivity (while emphasis and key terms are on): whole words, in any case, never inside links, code, maths, comments or citations. A term the server sometimes misses ("session") is then never missed. For a listed term the item reads *Stop always marking as a key term* and takes it out. The list is the setting *Always mark* (comma-separated, in data.json, so shared by the devices that share the vault); it is applied in the worker to the text, so a change shows at once, also on cached paragraphs, and never asks the server.
- **Settings**: sensitivity, the five kinds, always mark, every occurrence, diversity; the colour codes (each colour's name, colour, clause or phrase, opacity, dark-theme colour and description; colours and codes added and removed); context lines; debug log; the server's start with Obsidian, stop when idle (30 min), use the GPU, GPU load limit, warn at, model, port, the server's and the CPU server's paths; Models (each model's purpose, size, licence, page, folder and files, the download command with Copy, and Check again); and for this device only (kept in its own storage, not in the vault's data.json, so devices sharing a vault can differ): *Server address* and *Paragraphs per request*.

The slider is applied in the worker to the ranked results it holds, so moving it never asks the server: at sensitivity *s*, each sentence's colour when its probability is at least 1 − *s*, every quotation, definition and citation (whenever *s* > 0), up to `round(s × words / 20)` terms scoring within `0.04 + 0.16 s` of the paragraph's best, `round(s × n)` of the model's *n* contrast words (at least one), and `floor(3 s + 0.25)` more critical points.

## Server

A native Rust program: candle for the models (<https://github.com/huggingface/candle>), Hugging Face's tokenizers, udpipe-rs for the parse, axum for HTTP. The plugin starts it, stops it and adopts one already running on its port.

- **Models**: BAAI/bge-base-en-v1.5, Qwen/Qwen3-4B-Instruct-2507 and, for a code with phrase colours, fastino/gliner2-base-v1 (Apache-2.0; 0.8 GB, on the CPU, loaded on first use), read from the local Hugging Face cache when there, else from the server's cache folder (Disclosures and compliance, above). Started to serve, the server never downloads: when a file is missing it prints a `missing` line (each model's purpose, page, licence, folder, files and size) and exits with code 4, and the plugin says which models are missing and shows its Models section. `--download <ids|all>` is the download-only run you start in a terminal (`server/src/models.rs`): each default model is pinned to a Hugging Face commit, and each file goes through a `.part` file, is resumed with an HTTP range request after an interruption, and is checked against the size and checksum the Hugging Face API publishes (SHA-256, or the Git blob SHA-1 for small files) before it is renamed into place (native-kit's `download`). A model set by hand is listed from the Hugging Face API when it is downloaded. Qwen publishes no GGUF for this model and candle reads quantized weights only from GGUF, so the server quantizes the official weights once, at its first start after the download (Q4K, the tied embedding Q6K), into a 2.4 GB file there (about two minutes) and deletes the 8 GB of official weights it downloaded for it. `GET /models` lists the models a running server uses, with their status.
- **Parser**: UDPipe 1 with the Universal Dependencies 2.5 English EWT model (`english-ewt-ud-2.5-191206.udpipe`, 16 MB, MD5 4284eb5c16348e716d739b8ecec7de6d), downloaded by the same command into the same folder from LINDAT (<https://hdl.handle.net/11234/1-3131>) and checked against the SHA-256 of the file with that MD5. The model's licence is CC BY-NC-SA 4.0: personal and non-commercial use.
- **Offline**: once the models are on disk, the server needs no network at all, and the plugin talks only to the server.
- **Precision**: the embedding model runs in float32 (TF32 matrix products on CUDA). candle's BERT builds its attention mask with `f32::MIN`, which is −∞ in fp16 and bf16 and gives NaN.
- **Origins**: the server answers Obsidian's own origins (desktop, iOS, Android); `--origin` adds others.
- **Against the Python server it replaced** (8 paragraphs): the same main point in 5 of 8, overlap of the top terms 0.61, phrases 0.48, points 0.42; most differences come from UDPipe's parse, which is less accurate than spaCy's.

A device that cannot start the server (a browser, a phone, or a sandbox such as Ignis) uses one running elsewhere: set *Server address* on that device (for Ignis: the address of a relay to the computer's server, and *Paragraphs per request* 16). With an address set the plugin never starts or stops a server there.

## Colours

`POST /colours` gives each sentence one clause colour of a code (the worker asks it with the note's code, so a paragraph's colours are kept per code, and changing a description asks again for that code's notes only): Qwen3-4B reads the colours (lettered, with their descriptions), the paragraph, then each sentence in turn, and the letters' logits pick the colour (or "none of these"). Measured 2026-10-25 on 20 long paragraphs (245 sentences) of the two notes linked to the Literature code, against one independent reader's labels (strict: the reader's label; lenient: or the reader's second choice):

| | strict | lenient | clause colours drawn | time |
|---|---|---|---|---|
| Today's kinds mapped onto the colours (points, quotations; user's settings) | 10% | 13% | 2 of 6; 179 sentences unmarked | 1.4 s a paragraph (/claims) |
| /colours, Gemini's descriptions, two orders | 44% | 55% | 6 of 6 | 2.4 s |
| /colours, Gemini's descriptions, one order | 40% | 47% | 6 of 6 | 1.4 s |
| /colours, sharper descriptions, two orders | 51% | 64% | 6 of 6 | 2.4 s |
| /colours, sharper descriptions, one order (kept) | 53% | 67% | 6 of 6 | 1.4 s |

Calibrating against a content-free sentence (Zhao et al. 2021) lowered agreement (49%/61%). Below a probability ("w") of about 0.9 a colour agreed with the reader about half the time, above it 72%, so the sensitivity slider sets the floor: at sensitivity *s* a sentence is coloured when its colour's probability is at least 1 − *s* (every sentence at 100%). The descriptions matter most: Gemini's one-line descriptions let "macro conclusions" (Findings) swallow the writer's own verdicts.

What else was tried (2026-10-25), on the same sample with the sharper descriptions (one order: 53% strict, 66% lenient, 1.4 s a paragraph; Transitions F1 0.33, Thematic connections 0.17):

| Variant | strict | lenient | time | |
|---|---|---|---|---|
| the sentence's place in the paragraph ("sentence 3 of 9") | 52% | 66% | same | Transitions and connections found more often, as often wrongly |
| a hint to prefer a sentence's job over its topic | 50% | 63% | same | the same, worse overall |
| the sentence marked inside the paragraph instead of repeated after it | 39% | 49% | 3 × | connections everywhere |
| the colours' names scored instead of letters | 23% | 31% | same | |
| a short reason generated before the answer | 54% | 68% | 18 × | |
| a zero-shot NLI classifier (MoritzLaurer/deberta-v3-large-zeroshot-v2.0, MIT) | 32% | 44% | | never chose Transitions or connections |

A second independent reader agreed with the first on 92% of sentences (Transitions F1 0.90, Thematic connections 0.84), so the categories are clear to a careful reader: what limits the colours is the model's judgement, not the task. None of the variants is kept.

**Other models (2026-10-25)**, only those Rust can run (candle or a Rust crate), on the same samples:

| Model | Literature sentences (strict / lenient) | Methodology sentences | Methodology phrases (recall / precision) | time a paragraph | memory |
|---|---|---|---|---|---|
| Qwen3-4B-Instruct-2507, 4-bit (kept for sentences) | 53% / 66% | 37% | 60% / 15% | 0.9–1.4 s (GPU) | 3.5 GB GPU |
| Qwen3-8B, 4-bit (no thinking) | 47% / 58% | 32% | not run | 2–3 × | about 5 GB GPU |
| GLiNER2 base (fastino/gliner2-base-v1; kept for phrases) | 31% / 42% | | 63% / 41% | 2.3 s (CPU) | about 1 GB of the computer's |
| GLiNER2 large (fastino/gliner2-large-v1) | 50% / 61% | | 61% / 40% at 0.3 (Python) | | 2 GB |
| GLiNER2.5 base and small | | | 49% / 40% and 24% / 47% at 0.3 (Python) | | 0.9 and 0.4 GB |

Left out as Rust cannot run them yet: Qwen3.5-4B and 9B (February 2026; their linear-attention layers are not in candle) and the GLiNER2.5 checkpoints (a third-party ONNX port only). The NLI classifier above runs in candle (DeBERTa-v2) but was worse. So the instruct model keeps the sentence colours, and GLiNER2 base finds the phrase colours that no kind or rule finds (server/src/phrases.rs, the gliner2 crate): with the methods code's descriptions of the time it found 63% of the 164 phrases a reader marked in 20 methodology paragraphs, and 41% of what it marks is the reader's (47% at the slider's 50%, 51% at 30%), against 60% and 15% for the instruct model choosing a colour for every noun phrase. It reads a paragraph a few sentences at a time (its encoder takes 512 tokens) and runs on the CPU, about 2.3 s a paragraph, only for codes with such colours; the crate makes its inputs on the CPU, so the GPU would need a change to it.

## Known limits

- Links, citations, comments, code and maths are never emphasised; a phrase that would run across one is dropped.
- UDPipe sometimes mis-tags a word ("NER uses" as a noun phrase; "counts" as a noun), and coordinated phrases ("noisy and clean images") count as one; the definition rules that matter most (abbreviations, namings) therefore read the words, not the parse.
- The marks are for English only (the parser model, the definition cues and the prompts).
- Sentence colours are only as good as a 4B model's judgement (README, Colours: about half the sentences agree with a careful reader's labels, two thirds counting the reader's second choice); on the CPU they take seconds a paragraph. Phrase colours (GLiNER2) run on the CPU, about 2 s a paragraph.
- In Reading view, a list item that contains a nested list is skipped (its nested items are processed).
- Desktop only: the plugin starts the server as a process.
- Built and tested on Windows with CUDA, and on the CPU. The macOS (Metal) and Linux builds are written for but not yet built or tested; NVML numbers GPUs like CUDA only on a computer with one GPU.

## Licence

MIT (`LICENSE`). The third-party licences of the plugin's and the server's crates, and of the native code compiled into the server, are in `licences/` (`plugin.txt`, `server.txt`; regenerate them with `python licences/generate.py` and cargo-about after a dependency changes). `server/data/wordfreq-en.txt` is derived from wordfreq's English word list (Robyn Speer, <https://github.com/rspeer/wordfreq>) and is under CC BY-SA 4.0. The models the server downloads keep their own licences (Server, above).
