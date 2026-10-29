# Disclosures and compliance

[← README](../README.md)

Augmented View is written to meet Obsidian's
[Developer policies](https://docs.obsidian.md/Community+directory/Developer+policies), the
[Submission requirements for plugins](https://docs.obsidian.md/Plugins/Releasing/Submission+requirements+for+plugins)
and the [Plugin guidelines](https://docs.obsidian.md/Plugins/Releasing/Plugin+guidelines). This page goes through what
those policies ask a plugin to disclose, and what they forbid, and says plainly what Augmented View does about each and
why.

- [In short](#in-short)
- [Payment](#payment)
- [Accounts](#accounts)
- [Network use](#network-use)
- [Files outside the vault](#files-outside-the-vault)
- [The separate server program](#the-separate-server-program)
- [No self-installation or self-updating](#no-self-installation-or-self-updating)
- [Telemetry](#telemetry)
- [Ads](#ads)
- [Source code and licences](#source-code-and-licences)
- [Desktop only](#desktop-only)
- [The plugin guidelines](#the-plugin-guidelines)

## In short

Augmented View is free and open source, needs no account, shows no ads and collects nothing about you. The plugin
talks to one thing only, its model server on your own computer, and never downloads or installs anything. The server is
a separate program because the models need the GPU, which a plugin inside Obsidian cannot use. You download the server
and its models yourself; once they are on disk, nothing needs the network.

## Payment

None. The plugin, the server and the models are free, and every feature works without paying anyone.

## Accounts

None. There is no sign-in and no API key. The models are downloaded from public pages that need no account.

## Network use

**The plugin connects only to its own model server.** By default that is `http://127.0.0.1:8790`, on this computer
(the port is a setting). It sends the paragraphs of the note on screen, with the note's colour code, and receives
their colours. It makes no other network request. On a device where you set *Server address* yourself (for example a
phone using the server on your computer), it connects to that address instead, and still to nothing else.

**Your notes never leave your computer**, unless you point *Server address* at another machine yourself.

**The server downloads its models only when you ask it to, in a terminal you opened.** The models are too large to
ship with the plugin, so they are fetched once from their official pages:

| Host | What | Size |
|---|---|---|
| Hugging Face (`huggingface.co`) | BAAI/bge-base-en-v1.5 (key terms) | 0.44 GB |
| Hugging Face (`huggingface.co`) | Qwen/Qwen3-4B-Instruct-2507 (sentence colours), converted once on your computer into a 2.4 GB 4-bit file | 8.06 GB |
| Hugging Face (`huggingface.co`) | fastino/gliner2-base-v1 (phrase colours, only if a colour code has them) | 0.84 GB |
| LINDAT (`lindat.mff.cuni.cz`) | the UDPipe English parser model | 16 MB |

There are two ways to start that download, and you start both:

- **The download command**, `augmented-view-server --download all`, which you run in a terminal. The plugin's Models
  section shows the command for your computer, with a Copy button, but never runs it.
- **The terminal question.** If you start the server yourself in a terminal (for example after building it from
  source) and models are missing, it lists them, with their sizes, pages and licences, and asks
  `Download them now? [y/N]`. Only "y" or "yes" downloads; anything else exits and prints the command to run later.
  The question appears only when the server's input and output are a terminal you opened. When the plugin starts the
  server, its input is not a terminal, so the question is never asked and nothing is downloaded: the server says
  which models are missing and exits, and the plugin tells you. `--no-prompt` turns the question off.

Each file is checked against the size and checksum its source publishes before it is used.

**On this computer only**, when the GPU has too little free memory and you choose to free it, the server asks your
other local model servers that use the same mechanism (on `127.0.0.1`) to release their GPU memory. This is a request
to programs on your own computer, never to the internet, and happens only after you choose it.

## Files outside the vault

The plugin itself writes only inside its own folder in the vault (`.obsidian/plugins/augmented-view/`: its settings,
its per-note cache and an optional debug log) and in Obsidian's local storage (the settings kept on this device only).
The server, being a program of its own, uses these places outside the vault, each for a stated reason:

- **The server program**, which you download and place yourself: by default in the plugin's folder (inside the vault),
  or anywhere you choose, set in the *Server* setting.
- **The model files**, in the server's cache folder, because they are large and belong to the computer, not to one
  vault: `%LOCALAPPDATA%\augmented-view` on Windows, `~/Library/Caches/augmented-view` on macOS,
  `$XDG_CACHE_HOME/augmented-view` (by default `~/.cache/augmented-view`) on Linux. Delete the folder to remove them.
- **The Hugging Face cache** (`$HF_HUB_CACHE`, `$HF_HOME/hub` or `~/.cache/huggingface/hub`), read only, so that a
  model you already have is not downloaded again. Nothing is written there.
- **A GPU claim file**, one small JSON file (the server's port, process id and the GPU memory it holds) while the server
  holds GPU memory, removed when it stops: `%LOCALAPPDATA%\obsidian-kit\gpu` on Windows,
  `$XDG_DATA_HOME/obsidian-kit/gpu` (by default `~/.local/share/obsidian-kit/gpu`) on Linux and macOS. It lets your GPU
  services see which of them holds memory, so one can ask another to free it.

## The separate server program

The colours come from machine-learning models of several gigabytes that need a GPU to be fast. A plugin runs inside
Obsidian's JavaScript environment, which cannot load native GPU libraries such as CUDA. So the models run in a
separate native program, the **model server**, and the plugin sends it paragraphs over HTTP on `127.0.0.1`.

The server is open source, built from the Rust source in this repository. You download it from the
[server release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases/tag/server-0.6.2), published apart
from the plugin's releases (it carries each program's SHA-256 checksum and the licence files) or build it yourself, and you put it in place. The plugin starts it as a child process
when you ask (or with Obsidian, if you turn on *Start with Obsidian*), stops it when Obsidian closes or after 30 idle
minutes, and adopts one already running on its port. It starts no other program.

## No self-installation or self-updating

The policies forbid plugins that install or update themselves or their dependencies. Augmented View never downloads,
installs or updates anything, and never starts a download:

- you download the server program yourself, and put it in place yourself;
- you download the models yourself, with the command or the terminal question above; the plugin only shows the
  command, with a Copy button;
- the arguments the plugin starts the server with never include `--download`, and the plugin starts the server with
  its input ignored and its output piped, so the server's terminal question cannot appear. Tests check both.

## Telemetry

None, client-side or server-side. Nothing about you or your use is collected or sent anywhere. The readings the status
bar shows (GPU and memory load) are read on your computer and stay there.

## Ads

None, static or dynamic.

## Source code and licences

Open source, under the MIT licence ([LICENSE](../LICENSE)). The plugin and the server are built from the Rust source in
this repository and the shared crates and build tool in
[obsidian-cargo-lib](https://github.com/Gameboard-dev/obsidian-cargo-lib) ([Building from source](building.md)). The
code is not obfuscated: `main.js` is a short loader, and the plugin's code is a WebAssembly module compiled from that
source, with readable JavaScript bindings generated by wasm-bindgen.

Each server release carries the third-party licences of everything compiled into it. The models are not distributed
with the plugin or the server and keep their own licences; the UDPipe English parser model is under CC BY-NC-SA 4.0,
so it is for personal and non-commercial use only. All of them: [Licences](licences.md).

## Desktop only

`isDesktopOnly` is `true`: the plugin starts the server with Node's `child_process`, which exists only in desktop
Obsidian, and checks that it is in desktop Obsidian before doing so. A phone or a browser can still use a server
running elsewhere, through *Server address* ([Installation](installation.md#using-a-server-on-another-computer)).

## The plugin guidelines

| Guideline | How Augmented View meets it |
|---|---|
| No obfuscation | `main.js` is a short loader; the code is a WebAssembly module compiled from the Rust source here (inlined in `main.js` in a release), with its bindings generated by wasm-bindgen and kept readable |
| Security | No `innerHTML`, `outerHTML` or `insertAdjacentHTML`: text is set with `textContent` and the DOM API. No `eval`. The worker runs the same bundled bindings from a Blob URL, never code from elsewhere |
| Command ids | Without the plugin id (`toggle-server`, `toggle-emphasis`, `more`, `less`); no default hotkeys |
| Settings | Headings with `setHeading`, in sentence case, without the word "settings" |
| Resources | On unload the plugin stops its worker, its requests and a server it started, and removes its stylesheet and floating key; it never detaches leaves |
| `this.app` | The plugin uses its own `app`, never the global one |
| Licence and attribution | MIT; each server release carries `augmented-view-server-THIRD-PARTY-LICENSES.txt`, and the release tool refuses to publish the server without it |
