# Troubleshooting

[← README](../README.md)

## Common problems

| Problem | What to do |
|---|---|
| "Augmented View's server did not start: N models are not downloaded yet" | The server needs its models on disk before it starts. Press the notice's **Show the models** (Settings → Augmented View → Models), copy the download command and run it in a terminal, then press **Check again** ([Installation](installation.md#3-the-models)). A download that stopped (Ctrl+C, or a lost connection) resumes when you run it again |
| A download says the checksum does not match | The file is not used; run the command again to fetch it again |
| The plugin says where it looked for the server and found none | The server program is not in the plugin's folder (or at the path in *Server*). The plugin does not download it: get it from the [server release](https://github.com/Gameboard-dev/obsidian-augmented-view/releases/tag/server-0.6.2) ([Installation](installation.md#2-the-server-program)). On Linux and macOS, also make it executable (`chmod +x`) |
| "port 8790: …" | Another program uses the port. Change *Port* in the settings |
| The status bar says "· CPU" although you have an NVIDIA GPU | Hover over it: the tooltip says why. Usually CUDA's libraries are missing (install a current NVIDIA driver and CUDA), *Use the GPU* is off, or the GPU had too little free memory when the server started ([Fallbacks](gpu-and-memory.md#fallbacks)) |
| The plugin asks what to do because the GPU is full | Another program holds the GPU's memory. Choose the CPU (slower), or have your other local GPU services free their memory ([When the GPU is full](gpu-and-memory.md#when-the-gpu-is-full)) |
| A note shows no colours | Choose a colour code for it in the floating key; a note with no code is never marked. Check that *Show emphasis* is on, the sensitivity is above zero, and the colours are ticked in the key |
| Sentence colours are missing, other colours show | The server runs without the instruct model when no memory could hold it; the status bar's tooltip says so |
| Colours seem wrong for some sentences | About half the model's choices agree with a careful reader. Sharpen the colours' descriptions, or add [rules](colour-codes.md#rules) for the sentences it gets wrong |
| Colours are slow on the CPU | Expected: sentence colours take seconds a paragraph on the CPU. Phrase colours (GLiNER2) always run on the CPU, about 2 s a paragraph |
| Something else | Turn on *Debug log* and read `debug-desktop.log` in the plugin's folder; start the server yourself in a terminal to see its own output ([Starting it yourself](server.md#starting-it-yourself)) |

## Known limits

- Links, citations, comments, code and maths are never emphasised; a phrase that would run across one is dropped.
- UDPipe sometimes mis-tags a word ("NER uses" as a noun phrase; "counts" as a noun), and coordinated phrases ("noisy
  and clean images") count as one; the definition rules that matter most (abbreviations, namings) therefore read the
  words, not the parse.
- The marks are for English only (the parser model, the definition cues and the prompts).
- Sentence colours are only as good as a 4B model's judgement: about half the sentences agree with a careful reader's
  labels, two thirds counting the reader's second choice ([Models](models.md#how-sentence-colours-are-chosen)).
- In Reading view, a list item that contains a nested list is skipped (its nested items are processed).
- Desktop only: the plugin starts the server as a process.
- Built and tested on Windows with CUDA, and on the CPU. The macOS (Metal) and Linux builds are written for but not yet
  built or tested; NVML numbers GPUs like CUDA only on a computer with one GPU.
