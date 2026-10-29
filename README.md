# Augmented View

**Colours the paragraphs of a note for skim reading, with local models running on your own computer.**

Each sentence takes the colour of the role it plays (a claim, a finding, a limitation, a signpost …) and key phrases
take theirs (terms, definitions, citations, methods). It works in Live Preview, Source mode and Reading view. The note
is never changed: the colours are drawn over the text.

The colours come from a **colour code**: a list of colours you can edit, each with a name, a hex colour and a
description of what it marks, which the models read. Two codes come with the plugin (one for reading the literature,
one for a study's methods). You link a note or a folder to a code from the floating key; a note with no code is never
sent anywhere.

## Disclosures (Obsidian's developer policies)

What Augmented View sends over the network, stores outside the vault and runs, as Obsidian's
[Developer policies](https://docs.obsidian.md/Community+directory/Developer+policies) ask, is set out item by item in
**[docs/disclosures.md](docs/disclosures.md)**. In short: it is free, needs no account, shows no ads and collects no
telemetry. The plugin talks only to its own model server on your computer and never downloads or installs anything;
you download the server and its models yourself. The server is a separate program because the models need the GPU,
which a plugin inside Obsidian cannot use.

## Contents

- [Disclosures](#disclosures-obsidians-developer-policies)
- [Features](#features)
- [Requirements](#requirements)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Documentation](#documentation)
- [Licence](#licence)

## Features

- **Sentence colours**: a local instruct model (Qwen3-4B-Instruct, 4-bit) gives each sentence one colour of the note's
  code, chosen from the colours' descriptions.
- **Phrase colours**: key terms (an embedding model ranking the paragraph's noun phrases), definitions, quotations,
  citations, and phrases such as methods or datasets found by GLiNER2 from each colour's description.
- **Your own rules**: words or patterns that force or forbid a colour, applied at once and always winning over the
  models.
- **Sensitivity slider**: from only the surest colours to everything the models found, without asking the server again.
- **Light and dark themes**, each colour with its own opacity.
- **Fast and private**: paragraphs on screen go first; results are cached per paragraph, so an unchanged paragraph is
  never sent again; nothing leaves your computer.

## Requirements

- **Desktop Obsidian** (Windows; macOS and Linux builds are written for but not yet tested). Mobile Obsidian cannot
  start the server, but can use one running on another computer.
- **An NVIDIA GPU with about 4 GB of free memory** is recommended. Without one the server runs on the CPU, using about
  5 GB of the computer's memory, and is much slower.
- **About 8.5 GB to download once** (about 2.9 GB kept on disk; 0.84 GB more for phrase colours).

## Installation

1. Install the plugin from Obsidian's community plugins, or copy `main.js`, `manifest.json` and `styles.css` from a
   [release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases) into
   `<vault>/.obsidian/plugins/augmented-view/`.
2. Download the **model server** for your computer from its own
   [server release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases/tag/server-0.6.2) and put it in
   the plugin's folder, with its two licence files.
3. **Download the models** by running the server's download command in a terminal (Settings → Augmented View →
   **Models** shows it, with a Copy button). Or start the server yourself in a terminal: it lists the missing models
   and asks whether to download them.

Step by step, with the file names for each computer: [Installation](docs/installation.md).

## Quick start

1. Click **AugView** in the status bar and choose *Start the server* (or turn on *Start with Obsidian*).
2. Open a note. In the floating key, choose a colour code for the note, or for its whole folder.
3. Read. Move the sensitivity slider (status bar menu, or the settings) for more or fewer colours, and tick a colour off
   in the key to hide it.

## Documentation

| Page | What it covers |
|---|---|
| [Disclosures and compliance](docs/disclosures.md) | Network use, files outside the vault, the separate server, telemetry, ads, payment, licences, and each of Obsidian's rules |
| [Installation](docs/installation.md) | The plugin, the server program, downloading the models, using a server on another computer, removing it all |
| [Using Augmented View](docs/usage.md) | The status bar, commands, the floating key, *Always mark*, the sensitivity slider |
| [Colour codes and rules](docs/colour-codes.md) | Colour codes and their colours, themes, linking codes to notes, the Rules section |
| [Settings](docs/settings.md) | Every setting and what it does |
| [The server](docs/server.md) | Why it exists, how the plugin runs it, its command-line options and the terminal question, its endpoints |
| [Models](docs/models.md) | What each model finds, its size, source and licence, and how the choice was measured |
| [GPU, memory and fallbacks](docs/gpu-and-memory.md) | Where the models run, the GPU load limit, readings and warnings, what happens when memory is short |
| [Performance and cache](docs/performance.md) | What runs where, what is sent when, and the per-note cache |
| [Privacy and security](docs/privacy-and-security.md) | What leaves the plugin, and how the server and the plugin are protected |
| [Troubleshooting](docs/troubleshooting.md) | Common problems, and the known limits |
| [Building from source](docs/building.md) | Building the plugin and the server, build profiles, the code's layout |
| [Licences](docs/licences.md) | The plugin's licence, third-party licences, and the models' licences |

## Licence

MIT ([LICENSE](LICENSE)). The models keep their own licences; one of them (the UDPipe English parser model) is for
personal and non-commercial use only. Details: [Licences](docs/licences.md).
