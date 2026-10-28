# GPU, memory and fallbacks

[← README](../README.md)

- [Where the models run](#where-the-models-run)
- [Fallbacks](#fallbacks)
- [Memory needed](#memory-needed)
- [GPU load limit](#gpu-load-limit)
- [Readings and warnings](#readings-and-warnings)
- [When the GPU is full](#when-the-gpu-is-full)

## Where the models run

Each model goes on the GPU when there is one with room for it, else on the CPU. The status bar says where they run:
"AugView 50% · CUDA", "· CPU", "· CUDA + CPU" (one model each) or "· Metal". When anything is not on the GPU, a notice
says why once per server start, and the status bar's tooltip keeps the reasons.

Every build falls back to the CPU by itself: a CUDA build without a usable NVIDIA GPU, or with too little GPU memory
free, runs that model on the CPU and says why. GLiNER2 (phrase colours) always runs on the CPU.

## Fallbacks

| Situation | What happens |
|---|---|
| CUDA build, NVIDIA GPU with room | both models on CUDA |
| *Use the GPU* off | both on the CPU ("the GPU is turned off in the settings") |
| CUDA's libraries missing (Windows) | the same server runs on the CPU ("CUDA's libraries are not installed (cublas64_13.dll, …)"): the Windows build delay-loads them, so it starts without CUDA installed |
| CUDA's libraries missing (Linux) | the CUDA build cannot start; the plugin starts the CPU build ("… could not start; using the CPU build") |
| No NVIDIA GPU, or the driver fails | the CPU ("CUDA could not start: …") |
| Too little free GPU memory for a model | that model on the CPU ("sentence colours on the CPU: the GPU has 1200 MB free and needs about 3600 MB") |
| Too little memory anywhere for the instruct model | the server runs without sentence colours, and says so |
| Too little memory for the embedding model | the server stops; the notice says how much memory it needs and how much is available |
| No server where the settings point | the plugin says where it looked |

## Memory needed

About 0.55 GB of GPU memory for the embedding model (measured, with CUDA's own) and about 3.5 GB for the instruct model
(4.1 GB for both, measured); on the CPU about 1.2 GB and 4 GB of the computer's memory. GLiNER2 takes about 1 GB of the
computer's memory, loaded on first use. Before loading each model the server checks the free GPU memory (and the
computer's, for the CPU): a model that does not fit on the GPU goes on the CPU, and one that fits nowhere is left out
(sentence colours) or stops the server with the reason (key terms).

## GPU load limit

*GPU load limit* (default 100%): the server rests after each piece of GPU work so that the GPU works for it at most that
share of the time (at 50%, as long resting as working).

Measured on 4 paragraphs at 100%: the GPU 100% busy for 3.6 s, 111 W on average and 148 W at most of its 160 W limit
(idle: 10 W). That full-power burst is what makes some cards' coils whine; a lower limit lowers the average power and
heat but makes results slower, and cannot change the card's own sound while it works.

## Readings and warnings

- **Readings**: every 2 s while the server runs, the status bar's tooltip and menu show the GPU's load, memory, power
  and temperature (NVIDIA; all programs together, as the card reports them), the CPU's load and the computer's memory
  (all programs), and the server's own memory and CPU share. They come from NVML for the GPU
  (<https://docs.rs/nvml-wrapper>) and sysinfo for the rest (<https://docs.rs/sysinfo>), and stay on your computer.
  Apple's GPU shares the computer's memory, which is reported instead; a CPU-only build reports CPU and memory only.
- **Warnings**: when the GPU's memory or the computer's memory reaches *Warn at* (default 90%), or the GPU has been that
  busy for three readings running, a notice says so and what to do, and the status dot turns red. Each warns once until
  it has fallen 5 points below the level.

## When the GPU is full

When the GPU has too little free memory for the models, the plugin asks you what to do: run on the CPU, or have your
other local GPU services that use the same mechanism free their memory first. While the server holds GPU memory it
keeps a small claim file saying how much ([Disclosures](disclosures.md#files-outside-the-vault)), so those services can
see which of them holds memory; if another one asks it to free its memory, the server stops and says why.
