# Installation

[← README](../README.md)

- [1. The plugin](#1-the-plugin)
- [2. The server program](#2-the-server-program)
- [3. The models](#3-the-models)
- [4. First start](#4-first-start)
- [Using a server on another computer](#using-a-server-on-another-computer)
- [Removing Augmented View](#removing-augmented-view)

Augmented View has three parts, each of which you install yourself: the plugin, the model server (a separate program;
[why](server.md#why-a-separate-program)) and the models the server runs. The plugin never downloads or installs any of
them ([Disclosures](disclosures.md#no-self-installation-or-self-updating)).

## 1. The plugin

Install it from Obsidian's community plugins, or copy `main.js`, `manifest.json` and `styles.css` from a
[release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases) into
`<vault>/.obsidian/plugins/augmented-view/`, then turn it on under Settings → Community plugins.

## 2. The server program

Download the server for your computer from the same
[release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases):

| Computer | File |
|---|---|
| Windows with an NVIDIA GPU | `augmented-view-server-windows-x64.exe` (CUDA; falls back to the CPU by itself when CUDA is missing) |
| Windows without one | the CPU build, `augmented-view-server-cpu-windows-x64.exe` |
| macOS and Linux | build it from `server/` ([Building from source](building.md#the-server)); these builds are written for but not yet tested |

Save it in the plugin's folder as `augmented-view-server.exe` (the CPU build as `augmented-view-server-cpu.exe`), or
keep it anywhere under any name and set the *Server* (or *CPU server*) setting to its path. The `.sha256` file beside
it in the release lets you check it (`certutil -hashfile <file> SHA256` on Windows, `shasum -a 256 <file>` elsewhere).
Keep the release's two licence files with it: `augmented-view-server-LICENSE.txt` and
`augmented-view-server-THIRD-PARTY-LICENSES.txt`.

## 3. The models

The server needs its models on disk before it starts. You download them once, in one of two ways.

**The download command.** Settings → Augmented View → **Models** lists each model with its purpose, size, licence, a
link to its page (opened in your browser), the folder it goes in and its files, and below them the download command for
your computer, with **Copy**:

```
& "<vault>\.obsidian\plugins\augmented-view\augmented-view-server.exe" --download all      # Windows, in PowerShell
"<vault>/.obsidian/plugins/augmented-view/augmented-view-server" --download all            # macOS and Linux
```

The section shows it with your server's real path; it adds `--phrases` when a colour code has phrase colours, and
`--model` for a key-term model set by hand. Run it in a terminal. To download only some models, name them:
`--download terms,phrases` (the ids are `parser`, `terms`, `colours`, `phrases`).

**The terminal question.** If you start the server yourself in a terminal, without `--download`, and models are
missing, it lists them (each one's size, page, licence and folder, and the total) and asks:

```
Download them now? [y/N]
```

"y" or "yes" downloads them, as `--download all` would, and then the server starts. Anything else (including just
Enter) downloads nothing; the server prints the command to run later and exits. The question appears only when the
server's input and output are a terminal you opened; when the plugin starts the server it never appears. `--no-prompt`
turns it off ([The server](server.md#the-terminal-question)).

Either way, the server downloads the missing files (about 8.5 GB once, about 2.9 GB kept on disk; 0.84 GB more for
phrase colours), checks each against its published size and checksum, and prints its progress. Ctrl+C stops it;
running it again resumes where it stopped. At its first start after the download, the server converts the sentence
model once into a 2.4 GB 4-bit file (about two minutes) and deletes the 8 GB of official weights it downloaded for it.

You can also download the files by hand from each model's page into the folder the Models section shows. A model
already in your Hugging Face cache is used from there and not downloaded again.

What each model is for, and its licence: [Models](models.md).

## 4. First start

Click **AugView** in the status bar and choose *Start the server*. If a model is still missing the server does not
start; a notice says which models are missing and how large they are, and its button **Show the models** opens the
Models section. After downloading, press **Check again** there (or start the server from the status bar again).

Then open a note, choose a colour code in the floating key, and read ([Using Augmented View](usage.md)).

## Using a server on another computer

A device that cannot start the server (a phone, Obsidian in a browser, or a sandbox) can use one running elsewhere:
set *Server address* on that device to the server's address. With an address set, the plugin never starts or stops a
server there. In Ignis (Obsidian in a browser), with no address set, the plugin uses the server that desktop Obsidian
runs on the computer hosting Ignis, through Ignis's relay. Where a proxy passes answers on only once they are complete,
a smaller *Paragraphs per request* (16) shows results sooner.

The server answers Obsidian's own origins (desktop, iOS, Android) and listens on `127.0.0.1` only; reaching it from
another device needs a relay or proxy you set up, and `--origin` adds a web origin it should answer
([The server](server.md#command-line-options)).

## Removing Augmented View

- Turn the plugin off and delete its folder, `<vault>/.obsidian/plugins/augmented-view/` (its settings, cache and
  log are in it; the server program too, if you put it there).
- Delete the server's cache folder to remove the models: `%LOCALAPPDATA%\augmented-view` on Windows,
  `~/Library/Caches/augmented-view` on macOS, `~/.cache/augmented-view` on Linux.
- Nothing else is left: the GPU claim file is removed whenever the server stops
  ([Disclosures](disclosures.md#files-outside-the-vault)).
