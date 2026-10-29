# Building from source

[← README](../README.md)

- [The plugin](#the-plugin)
- [The server](#the-server)
- [Build profiles](#build-profiles)
- [Code layout](#code-layout)

## The plugin

This repository is one plugin of a Rust workspace: clone
[obsidian-cargo-lib](https://github.com/Gameboard-dev/obsidian-cargo-lib) (the shared crate and the build tool, pack),
then clone this repository into it as `augmented-view`, and build from the workspace folder.

```bash
git clone https://github.com/Gameboard-dev/obsidian-cargo-lib
git clone https://github.com/Gameboard-dev/obsidian-augmented-view obsidian-cargo-lib/augmented-view
```

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129      # must match the wasm-bindgen crate (core/Cargo.toml)
cd obsidian-cargo-lib                                 # the workspace of the plugins
cargo run -p pack --release -- augmented-view --server --deploy <vault>/.obsidian/plugins/augmented-view   # plugin + server
cargo run -p pack --release -- augmented-view --fast --deploy <dir>   # the plugin only, built quickly while working
cargo run -p pack --release -- augmented-view --release <dir>       # the release's main.js (the module inlined), manifest.json, styles.css
cargo run -p pack --release -- augmented-view --cpu-server            # the CPU-only server, into server/target/cpu
cargo test --release -p augmented-view-core              # the plugin's tests (native)
(cd augmented-view/server && cargo test --release)      # the server's tests (masking, clauses, terms, the model manifest, the terminal question)
cargo doc -p augmented-view-core --no-deps --open         # the code's documentation, with links to the APIs it relies on
```

`--deploy` also reads its folder from the environment variable `AUGMENTED_VIEW_DEPLOY`. It copies the plugin only;
point the plugin's *Server* setting at the server, or copy it into the plugin's folder as `augmented-view-server`
(`.exe` on Windows), and the CPU build as `augmented-view-server-cpu`. Then turn the plugin off and on in Obsidian
(Settings → Community plugins).

The release assets are the files to copy into `<vault>/.obsidian/plugins/augmented-view/`. With a server built,
`--release` also writes the server as `augmented-view-server-<platform>` with its `.sha256` and its two licence files,
and refuses to release a server without them. Since 0.6.4 those go to a release of their own, `server-<version>`
(created with `--latest=false`, so the plugin's release stays the latest), and the plugin's release (tag = the
manifest's version) carries only `main.js`, `manifest.json` and `styles.css`, as Obsidian's review asks.

A plugin release is built by GitHub Actions (`.github/workflows/release.yml`) when its tag is pushed: it checks out
obsidian-cargo-lib at the commit pinned in the workflow, puts this repository in it as `augmented-view`, runs
`cargo run -p pack --release -- augmented-view --release <dir>`, attests the three files' build provenance
([artifact attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds))
and publishes them. Check a downloaded file with `gh attestation verify main.js --repo Gameboard-dev/obsidian-augmented-view`.

## The server

One build per kind of computer (`server/Cargo.toml` features):

| Computer | Build | GPU readings |
|---|---|---|
| Windows or Linux with an NVIDIA GPU | `cargo build --release` (feature `cuda`, the default) | load, memory, power, temperature (NVML, from the driver) |
| macOS (Apple silicon) | `cargo build --release --no-default-features --features metal` | none: Apple's GPU shares the computer's memory, which is reported |
| Any, CPU only | `cargo build --release --no-default-features` | none |

Every build reports CPU load and memory (the computer's and the server's), and every build falls back to the CPU by
itself ([GPU, memory and fallbacks](gpu-and-memory.md)).

- **CUDA's libraries.** candle links cuBLAS and cuRAND when the program loads. On Windows the server delay-loads them
  (`build.rs`), so it starts without CUDA installed, finds the libraries missing and uses the CPU. On Linux a CUDA
  build cannot start without them; the plugin then starts the CPU build (*CPU server*).
- **Machine-specific settings** go in a cargo config outside this folder (cargo merges config files from parent
  folders), for example `obsidian-cargo-lib/.cargo/config.toml`: `target-cpu=native`, `NVCC_CCBIN` (nvcc needs
  Microsoft's C++ compiler on Windows), `CUDA_COMPUTE_CAP` (the GPU's compute capability; a build for others should use
  the lowest it supports, as newer GPUs compile its kernels when they load) and
  `NVCC_APPEND_FLAGS = "-Xcompiler /Zc:preprocessor"` (CUDA 13 with MSVC). The checked-in `server/.cargo/config.toml`
  holds only what every build needs (Windows: the C runtime linked statically, as candle's CUDA kernels are).
- The first CUDA build compiles candle's kernels (several minutes); later ones take about 20 s.
- A server you build and start yourself asks, in the terminal, whether to download missing models
  ([The terminal question](server.md#the-terminal-question)).

## Build profiles

Two build profiles for the plugins (the workspace's `Cargo.toml`):

- `release`, the one shipped: `opt-level = 3`, fat LTO, one codegen unit, WebAssembly SIMD (passed by pack); about 10 s
  after a change.
- `fast`: the same optimisation level with thin LTO, 16 codegen units and incremental compilation; about 1 s after a
  change, a 12% larger module.

Dependencies are compiled once and kept in the workspace's `target/`, and pack regenerates nothing when the compiled
module is unchanged (0.3 s). Measured and left out (2026-10-24): Binaryen's wasm-opt made the module 6% smaller but the
engine 5–10% slower.

## Code layout

Everything is Rust except `main.js` (18 lines, the only JavaScript), which loads `augmented_view.wasm` and hands it
Obsidian; the .wasm carries its own wasm-bindgen bindings in a custom section named `glue`, so nothing else is shipped
or compiled from JavaScript.

- `core/src/`: the plugin crate.
  - On Obsidian's main thread: `app.rs` (settings, status bar, menu, commands, settings tab, the server's life cycle
    and readings, fetch and file I/O), `key.rs` (the floating key), `colours.rs` (colour codes, links and the generated stylesheet), `models.rs` (the notice when
    models are missing, and the Models section of the settings, with the download command to copy; it starts nothing),
    `rules_tab.rs` (the settings' Rules section).
  - In a Web Worker started from the same module: `engine.rs` (notes, cache, request planning), with `rules.rs`
    (citations, and the colours' own rules), `pick.rs` (the sensitivity rule), `bold.rs` (the *Always mark* terms),
    `reading.rs` (the chosen spans in rendered text), `cache.rs` (per-note cache files).
  - `msg.rs` holds the settings and the messages between the two (postcard).
  - The helpers through which Obsidian's and CodeMirror's JavaScript APIs are called by name (`call!`, `obj!`) and the
    settings-tab builder are in the shared crate, `shared` (obsidian-kit), in obsidian-cargo-lib.
  - What draws the marks is the shared crate `view-marks` (obsidian-cargo-lib): the CodeMirror view plugin
    (`editor.rs`), Reading view (`reading.rs`), the paragraphs (`split.rs`), the plan of which paragraphs a view
    wants (`plan.rs`, used by `engine.rs`), finding a passage in rendered text (`locate.rs`), the copy with highlights
    (`highlights.rs`, `copy.rs`) and the stylesheet's rules (`style.rs`). Augmented View gives it its classes
    (`pick::class`), each note's colour code as the scope (`data-av-code`) and the worker as the place its events go.
- `pack/` (in obsidian-cargo-lib): the build shared by the plugins: compiles to WebAssembly, runs wasm-bindgen, stores
  the bindings in the .wasm, builds the server, copies the plugin into a vault.
- `server/`: the model server, a native Rust program (its own cargo workspace; `src/main.rs` describes the protocol).
  `text.rs` blanks markup and aligns UDPipe's parse to the text; `rules.rs` finds terms and clauses and ranks them;
  `meaning.rs` finds quotations and definitions; `embed.rs` runs the embedding model; `claims.rs` the instruct model;
  `phrases.rs` GLiNER2 for the phrase colours; `models.rs` the models' manifest, the start check, the terminal question
  and the download (`--download`); `build.rs` makes the Windows CUDA build start without CUDA. The hardware (each
  model's device and fallbacks, the readings, GPU pacing) and the checked downloads are the shared crate `native-kit`
  in obsidian-cargo-lib.

Dependencies of the plugin are the official rustwasm crates (wasm-bindgen, wasm-bindgen-futures, js-sys) and serde,
serde_json, serde-wasm-bindgen, postcard, xxhash-rust; no web-sys (the DOM is reached by name too) and no JavaScript
tooling. No maintained Obsidian binding crate exists (magnetite-lib was archived in 2024; trashhalo/obsidian-rust-plugin
is a template that still needs yarn and rollup).
