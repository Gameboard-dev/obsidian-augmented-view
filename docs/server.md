# The server

[← README](../README.md)

- [Why a separate program](#why-a-separate-program)
- [How the plugin runs it](#how-the-plugin-runs-it)
- [Starting it yourself](#starting-it-yourself)
- [The terminal question](#the-terminal-question)
- [Command-line options](#command-line-options)
- [Endpoints](#endpoints)
- [Offline, origins and precision](#offline-origins-and-precision)

## Why a separate program

The models that find the colours are several gigabytes and need a GPU to be fast. A plugin runs inside Obsidian's
JavaScript environment, which cannot load native GPU libraries such as CUDA, so the models run in a native program of
their own. The plugin sends it the paragraphs on screen over HTTP on `127.0.0.1` and draws what comes back.

The server is a native Rust program: [candle](https://github.com/huggingface/candle) for the models, Hugging Face's
tokenizers, udpipe-rs for the parse, and axum for HTTP. Which models it runs: [Models](models.md).

## How the plugin runs it

- The plugin starts the server as a child process when you choose *Start the server* in the status bar menu (or when
  Obsidian starts, with *Start with Obsidian*), and stops it when you choose *Stop the server* or the plugin unloads.
  A server already running on its port is adopted.
- With *Stop when idle* (default 30 minutes) the server exits after that long without a request, freeing the GPU.
- It starts the server with its input ignored and its output piped, and with only these arguments: `--port`, `--model`,
  `--idle-minutes`, `--gpu-limit`, `--device cpu` or `--gpu`, and `--no-claims` or `--phrases` when they apply. Never
  `--download`: a test checks it.
- When a model is missing, the server prints one line listing the missing models (each one's purpose, page, licence,
  folder, files and size) and exits with code 4. The plugin then shows a notice saying which, with a button to the
  Models section. Started by the plugin, the server never downloads and never asks.
- On Linux, a CUDA build cannot start without CUDA's libraries; the plugin then starts the CPU build (*CPU server*).
  On Windows the CUDA build starts without them and runs on the CPU ([GPU, memory and fallbacks](gpu-and-memory.md)).

## Starting it yourself

You can run the server by hand, for example after building it from source, or to watch its output:

```
augmented-view-server                       # serve on 127.0.0.1:8790
augmented-view-server --download all        # download the missing models only, then exit
```

The plugin adopts a server already running on its port, so a server you started yourself is used as it is.

## The terminal question

When you start the server to serve, by hand in a terminal, and models it needs are missing, it lists them and asks
before doing anything:

```
Augmented View's server needs these models, which are not on this computer yet:
  - BAAI/bge-base-en-v1.5 (0.44 GB): Key terms: ranks each paragraph's words and phrases for emphasis …
      page https://huggingface.co/BAAI/bge-base-en-v1.5  licence MIT (https://opensource.org/license/mit)
      into …
  …
In all about 8.5 GB, from Hugging Face (huggingface.co) and LINDAT (lindat.mff.cuni.cz), into ….
Each file is checked against its published size and checksum; Ctrl+C stops, and running again resumes.
Download them now? [y/N]
```

- **"y" or "yes"** (any case) downloads them exactly as `--download all` would (resumable, each file checked), then the
  server goes on to start. Ctrl+C during the download stops it and keeps what was fetched; Ctrl+C after it, while the
  server runs, stops the server as usual.
- **Anything else**, an empty line or the end of the input, downloads nothing: the server prints the command that
  downloads them later (this program, `--download all`, and the arguments you gave it) and exits with code 4.
- The question is asked **only when the server's input, output and error output are all terminals**, which is the case
  when you start it yourself in a terminal you opened. When the plugin starts the server, its input is ignored and its
  output piped, so the question never appears and the plugin's start never downloads anything. In a script, a pipe or
  with output redirected to a file it does not appear either.
- `--no-prompt` turns the question off: the server then behaves as when the plugin starts it (lists the missing models
  and exits with code 4).

## Command-line options

| Option | What it does |
|---|---|
| `--port N` | The port on `127.0.0.1` (default 8790) |
| `--model REPO` | The key-term embedding model, a BERT-type model on Hugging Face (default `BAAI/bge-base-en-v1.5`) |
| `--claims-model REPO` | The instruct model for sentence colours (default `Qwen/Qwen3-4B-Instruct-2507`) |
| `--no-claims` | Run without the instruct model (no sentence colours or contrast words; spares its memory) |
| `--phrases` | Also need GLiNER2, for colour codes with phrase colours |
| `--idle-minutes N` | Exit after N minutes without a request (0: never) |
| `--cache DIR` | The folder for the model files (default: the operating system's per-user cache, [Installation](installation.md#removing-augmented-view)) |
| `--device cpu` | Run every model on the CPU (`auto`, the default, uses the GPU when it can) |
| `--gpu ask\|free` | When the GPU has no room: `ask` stops at once so the plugin can ask you; `free` has your other local GPU services free their memory first; without it, models go where they fit |
| `--gpu-limit P` | The most of the time the GPU may work for the server, in percent ([GPU load limit](gpu-and-memory.md#gpu-load-limit)) |
| `--origin URL` | A further web origin to answer, beside Obsidian's own |
| `--download [ids\|all]` | Download the missing models only, then exit (0 done, 1 failed, 2 a wrong id, 130 stopped with Ctrl+C). Ids: `parser`, `terms`, `colours`, `phrases`, separated by commas; none means all |
| `--no-prompt` | Never ask in a terminal to download missing models ([above](#the-terminal-question)) |

Exit codes when starting to serve: 4 when models are missing, 3 when the GPU has no room and `--gpu ask` was given
(after a `gpu_shortage` line for the plugin), 1 for any other failure (its reason on the last line of error output).

## Endpoints

JSON Lines (<https://jsonlines.org/>), streamed one line per paragraph as each is done:

| Endpoint | What it does |
|---|---|
| `POST /emphasis` | Key terms, quotations and definitions for a list of paragraphs |
| `POST /colours` | Each sentence's colour from the code's clause colours (with its probability); with `phrases`, those colours' phrases (GLiNER2); with `stress`, the contrast words |
| `GET /health` | Whether it is ready, the protocol, the models and where each runs, and why anything is not on the GPU |
| `GET /stats` | The GPU's and the computer's load and memory, and the server's own |
| `GET /models` | The models this run uses: each one's purpose, page, licence, folder, files and status |
| `POST /shutdown` | Stops the server |
| `POST /gpu/release` | Another local GPU service needs the memory: the server stops and says why |

Offsets are UTF-16 code units of the text sent. The protocol has a version, which the plugin's cache is keyed by, so
results from another server version are never mixed in. The full protocol is described at the top of
`server/src/main.rs`.

## Offline, origins and precision

- **Offline**: once the models are on disk, the server needs no network at all, and the plugin talks only to the
  server.
- **Origins**: the server answers Obsidian's own origins (desktop, iOS, Android); `--origin` adds others. A POST from a
  web page with any other origin is refused, so a page open in your browser cannot stop the server or send it work.
- **Precision**: the embedding model runs in float32 (TF32 matrix products on CUDA): candle's BERT builds its attention
  mask with `f32::MIN`, which is −∞ in fp16 and bf16 and gives NaN.
- **Against the Python server it replaced** (8 paragraphs): the same main point in 5 of 8, overlap of the top terms
  0.61, phrases 0.48, points 0.42; most differences come from UDPipe's parse, which is less accurate than spaCy's.
