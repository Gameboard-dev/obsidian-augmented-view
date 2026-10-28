# Settings

[← README](../README.md)

Settings → Augmented View. A column of contents beside the settings (above them in a narrow window) links to each
heading. Most settings are kept in the plugin's `data.json` in the vault, so devices sharing the vault share them;
those under **This device** are kept in this device's own storage, so devices sharing a vault can differ.

## Emphasis

| Setting | What it does |
|---|---|
| Show emphasis | Off hides the colours without stopping the server |
| Sensitivity | Left: only the surest colours and the strongest term. Right: every sentence and phrase the models coloured, up to one term per 20 words and every contrast word ([the rule](usage.md#the-sensitivity-slider)) |
| Always mark | Words and phrases marked as key terms in every paragraph, each time they appear, separated by commas ([Always mark](usage.md#always-mark-as-a-key-term)) |
| Every occurrence | Emphasise a chosen term or contrast word each time it appears in the paragraph, not only the first time |
| Diversity | How strongly the ranking avoids terms that mean much the same. Changing it asks the server again |

## Colour codes and rules

The colour codes (each colour's name, colour, level, opacity, dark-theme colour and description; colours and codes
added and removed), the links between codes and notes or folders, and each colour's rules:
[Colour codes and rules](colour-codes.md).

## What to send

| Setting | What it does |
|---|---|
| Context paragraphs | After the paragraphs on screen, and only when the server is idle and you have stayed put for a second, this many paragraphs above and below (default 1), two at a time, so they are ready as you scroll. More: more ready ahead, and more work for the GPU |
| Debug log | Write what the plugin does to `debug-desktop.log` (`debug-remote.log` on a device using a server elsewhere) in the plugin's folder |

## Server

| Setting | What it does |
|---|---|
| Start with Obsidian | Start the server when Obsidian starts |
| Stop when idle | Minutes without a request before the server exits and frees the GPU (default 30; 0 = never) |
| Use the GPU | CUDA (NVIDIA) or Metal (Apple) when the server's build supports it and the GPU has room; otherwise, or off, the CPU (slower). Takes effect at the server's next start |
| GPU load limit | The most of the time the GPU may work for Augmented View, in percent ([GPU load limit](gpu-and-memory.md#gpu-load-limit)). Takes effect at the server's next start |
| Warn at | Warn when the GPU's load or memory, or the computer's memory, reaches this percent (default 90; all programs together) |
| Model | The key-term embedding model, a BERT-type model on Hugging Face. If it is not on this computer, the server does not start and Models shows the command that downloads it. Takes effect at the server's next start |
| Port | The server's port on `127.0.0.1` (default 8790) |

## Models

Each model the server needs: its purpose, size, licence, a link to its page, its folder and files, and whether it is on
this computer; the download command for your computer with **Copy**; and **Check again**, which looks for the models
again (starting the server if it is not running). The plugin never runs the command
([Installation](installation.md#3-the-models)).

## This device

| Setting | What it does |
|---|---|
| Server | The server's executable for this computer. Empty: `augmented-view-server` in the plugin's folder |
| CPU server | A CPU-only build, started when the server above cannot start at all (a CUDA build where CUDA's libraries are missing). Empty: `augmented-view-server-cpu` in the plugin's folder, if there |
| Server address | Only for a device that cannot start the server itself: the address of one running elsewhere ([Using a server on another computer](installation.md#using-a-server-on-another-computer)) |
| Paragraphs per request | Up to this many paragraphs in one request (default 64). Fewer show sooner where a proxy passes answers on only once they are complete (16 suits one) |
