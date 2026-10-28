# Models

[← README](../README.md)

- [The models](#the-models)
- [What is found, and by what](#what-is-found-and-by-what)
- [Downloads, checksums and conversion](#downloads-checksums-and-conversion)
- [How sentence colours are chosen](#how-sentence-colours-are-chosen)
- [What else was measured](#what-else-was-measured)

## The models

| Id | Model | Used for | Download | Licence |
|---|---|---|---|---|
| `parser` | [UDPipe 1, Universal Dependencies 2.5 English EWT](https://hdl.handle.net/11234/1-3131) (`english-ewt-ud-2.5-191206.udpipe`) | sentence structure: the parse that key terms, quotations and definitions are read from | 16 MB, from LINDAT | CC BY-NC-SA 4.0: personal and non-commercial use only |
| `terms` | [BAAI/bge-base-en-v1.5](https://huggingface.co/BAAI/bge-base-en-v1.5) | key terms: an embedding model ranking each paragraph's words and phrases | 0.44 GB | MIT |
| `colours` | [Qwen/Qwen3-4B-Instruct-2507](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507) | sentence colours and contrast words | 8.06 GB, kept as a 2.4 GB 4-bit file | Apache-2.0 |
| `phrases` | [fastino/gliner2-base-v1](https://huggingface.co/fastino/gliner2-base-v1) | phrase colours that no rule or kind finds (only for codes that have them; on the CPU, loaded on first use) | 0.84 GB | Apache-2.0 |

The key-term model can be changed in the settings (*Model*: any BERT-type embedding model on Hugging Face). A model
set by hand is listed from the Hugging Face API when it is downloaded, and its licence is on its page.

How much memory they need, and where they run: [GPU, memory and fallbacks](gpu-and-memory.md).

## What is found, and by what

Each colour of a code says which of these it shows ([Colour codes and rules](colour-codes.md)).

| Kind | In the default codes | Found by |
|---|---|---|
| Sentence colours | every clause colour (Claims, Signposts, Findings, Evidence and context, Limitations, Connections; in Methods: Questions, Definitions, Design limits, Ethics) | the instruct model choosing each sentence's colour from the colours' descriptions (`/colours`) |
| Key terms | Key terms | noun phrases from UDPipe's parse, ranked by their similarity to the paragraph (the embedding model, KeyBERT's method), plus how specific their words are (how rarely English uses them: wordfreq's word frequencies, in `server/data/`) and how often the paragraph repeats them; also a word or two quoted to be talked about ("positivism"); never a cited author's name |
| Definitions | Key terms; in Methods, Definitions | rules on the parse (`server/src/meaning.rs`): a quotation presented as a definition ("defined as", after the term and a comma, after "is a"), a gloss ("an adversarial attack, a change to the pixels …"), "X is a Y that …", what follows "means", "denotes" or "refers to", an abbreviation's clause ("FAR counts attacks accepted"), and a naming ("Choosing the range … is called calibration": the clause underlined, the name bold) |
| Quotations | Evidence and context | rule: words between quotation marks, outside code, links and citations |
| Citations | Citations | rule, in the plugin's worker (`core/src/rules.rs`): `[@key, p. 3]`, `[@a; @b]` and `@key`, outside code; in Reading view too, where a citation keeps its text |
| Contrast words | none | the instruct model, asked which single words the paragraph's meaning turns on ("only", "never", "external") |
| Phrase colours | Frameworks, Analysis, Data, Design type (Methods) | GLiNER2 finding the phrases each colour's description fits, on the CPU (`server/src/phrases.rs`) |
| Your own rules | the colour each rule belongs to | words or patterns you give a colour, in the worker; they win over the models ([Rules](colour-codes.md#rules)) |

## Downloads, checksums and conversion

- **You start every download** ([Installation](installation.md#3-the-models)): with `--download`, or by answering yes
  to the [terminal question](server.md#the-terminal-question). The plugin never does.
- **Pinned.** Each default model is pinned to a Hugging Face commit, with its files' sizes and checksums taken from the
  Hugging Face API.
- **Checked and resumable.** Each file goes through a `.part` file, is resumed with an HTTP range request after an
  interruption, and is checked against the size and checksum the Hugging Face API publishes (SHA-256, or the Git blob
  SHA-1 for small files) before it is renamed into place. The parser is checked against the SHA-256 of the file whose
  MD5 LINDAT publishes (`4284eb5c16348e716d739b8ecec7de6d`).
- **Where.** Files are read from the local Hugging Face cache when there, else from the server's cache folder
  ([Disclosures](disclosures.md#files-outside-the-vault)).
- **Converted once.** Qwen publishes no GGUF file for the instruct model, and candle reads quantized weights only from
  GGUF, so the server quantizes the official weights once, at its first start after the download (Q4K, with the tied
  embedding Q6K), into a 2.4 GB file in the cache folder (about two minutes), and deletes the 8 GB of official weights
  it downloaded for it.

## How sentence colours are chosen

`POST /colours` gives each sentence one clause colour of the note's code. The instruct model reads the colours
(lettered, with their descriptions), the paragraph, then each sentence in turn, and the letters' probabilities pick the
colour, or "none of these". The plugin asks with the note's code, so a paragraph's colours are kept per code, and
changing a description asks again only for that code's notes.

**Agreement with a careful reader** (measured 2026-10-25 on 20 long paragraphs of literature notes, 245 sentences,
against one independent reader's labels; strict: the reader's label; lenient: or the reader's second choice):

| | strict | lenient | clause colours drawn | time a paragraph |
|---|---|---|---|---|
| The earlier kinds (points, quotations) mapped onto the colours | 10% | 13% | 2 of 6; 179 sentences unmarked | 1.4 s |
| `/colours`, first one-line descriptions, two orders | 44% | 55% | 6 of 6 | 2.4 s |
| `/colours`, first one-line descriptions, one order | 40% | 47% | 6 of 6 | 1.4 s |
| `/colours`, sharper descriptions, two orders | 51% | 64% | 6 of 6 | 2.4 s |
| `/colours`, sharper descriptions, one order (kept) | 53% | 67% | 6 of 6 | 1.4 s |

Calibrating against a content-free sentence (Zhao et al. 2021) lowered agreement (49% / 61%). Below a probability of
about 0.9 a colour agreed with the reader about half the time, above it 72%, so the sensitivity slider sets the floor:
at sensitivity *s* a sentence is coloured when its colour's probability is at least 1 − *s* (every sentence at 100%).
**The descriptions matter most**: one-line descriptions let a broad colour (findings described as "macro conclusions")
swallow the writer's own verdicts.

A second independent reader agreed with the first on 92% of sentences (Transitions F1 0.90, Thematic connections
0.84), so the categories are clear to a careful reader: what limits the colours is the model's judgement, not the task.

## A second measurement (2026-10-28)

Two finished chapters of a thesis, a literature review read with a code of six clause colours and a methods chapter
read with one of four, about 300 sentences, each judged by one reader against the code's descriptions. The sentences
were split into two halves by alternate paragraph; rules and descriptions were written looking at the first half only,
and the second half shows whether a change holds. Error rates at full sensitivity, sentences the reader called
borderline left out (literature 50 / 69 sentences in the two halves, methods 73 / 66):

| | literature, first half | second half | methods, first half | second half |
|---|---|---|---|---|
| before | 36% | 39% | 49% | 29% |
| the server's corrected sentences and the "none of these" instruction | 32% | 41% | 49% | 33% |
| the same, with the user's descriptions and rules refined | 28% | 39% | 30% | 29% |

- **Sentences.** The corrected splitting ([Colour codes](colour-codes.md#what-the-models-are-told)) removes fragments
  such as "Lee et al." coloured on their own and two sentences coloured as one; accuracy is unchanged within noise
  (one or two sentences a half either way).
- **"None of these".** Telling the model to choose it when no colour fits changed one or two sentences a half, never for
  the worse; kept. Rewording the option itself made the methods chapter worse and was dropped.
- **The heading path** of each paragraph ("Method > Threats to validity") given as context made both chapters slightly
  worse: not kept.
- **Descriptions** rewritten for the literature code moved errors between colours (fewer in the first half, more in the
  second): not a gain. For the methods code, a narrower measurement colour plus a hidden colour for design, materials
  and data cut errors from 49% to 42% (first half) and 35% to 30% (second).
- **Rules** worked where a pattern is unambiguous: gap sentences opening "First, no …" or saying "has not been examined"
  (limitations) held in the second half; one turning "@key found …" into findings did not (39% → 42%) and was dropped.
- **Sensitivity.** In the literature chapter, a floor of 0.95 (sensitivity 5%) left 10–20% of sentences unmarked and
  raised the share of drawn colours that were right from 71% to 78% (first half) and 61% to 63% (second).

## What else was measured

**Prompt variants** (2026-10-25, same sample, sharper descriptions; the baseline, one order: 53% strict, 66% lenient,
1.4 s a paragraph; Transitions F1 0.33, Thematic connections 0.17). None is kept.

| Variant | strict | lenient | time | |
|---|---|---|---|---|
| the sentence's place in the paragraph ("sentence 3 of 9") | 52% | 66% | same | transitions and connections found more often, as often wrongly |
| a hint to prefer a sentence's job over its topic | 50% | 63% | same | the same, worse overall |
| the sentence marked inside the paragraph instead of repeated after it | 39% | 49% | 3 × | connections everywhere |
| the colours' names scored instead of letters | 23% | 31% | same | |
| a short reason generated before the answer | 54% | 68% | 18 × | |
| a zero-shot NLI classifier (MoritzLaurer/deberta-v3-large-zeroshot-v2.0, MIT) | 32% | 44% | | never chose transitions or connections |

**Other models** (2026-10-25), only those Rust can run (candle or a Rust crate), on the same samples:

| Model | Literature sentences (strict / lenient) | Methodology sentences | Methodology phrases (recall / precision) | time a paragraph | memory |
|---|---|---|---|---|---|
| Qwen3-4B-Instruct-2507, 4-bit (kept for sentences) | 53% / 66% | 37% | 60% / 15% | 0.9–1.4 s (GPU) | 3.5 GB GPU |
| Qwen3-8B, 4-bit (no thinking) | 47% / 58% | 32% | not run | 2–3 × | about 5 GB GPU |
| GLiNER2 base (fastino/gliner2-base-v1; kept for phrases) | 31% / 42% | | 63% / 41% | 2.3 s (CPU) | about 1 GB of the computer's |
| GLiNER2 large (fastino/gliner2-large-v1) | 50% / 61% | | 61% / 40% at 0.3 (Python) | | 2 GB |
| GLiNER2.5 base and small | | | 49% / 40% and 24% / 47% at 0.3 (Python) | | 0.9 and 0.4 GB |

Left out because Rust cannot run them yet: Qwen3.5-4B and 9B (February 2026; their linear-attention layers are not in
candle) and the GLiNER2.5 checkpoints (a third-party ONNX port only). The NLI classifier runs in candle (DeBERTa-v2) but
was worse.

So the instruct model keeps the sentence colours, and GLiNER2 base finds the phrase colours that no kind or rule finds
(the gliner2 crate). In 20 methodology paragraphs it found 63% of the 164 phrases a reader marked, and 41% of what it
marks is the reader's (47% at the slider's 50%, 51% at 30%), against 60% and 15% for the instruct model choosing a
colour for every noun phrase. It reads a paragraph a few sentences at a time (its encoder takes 512 tokens) and runs on
the CPU, about 2.3 s a paragraph, only for codes with such colours; the crate makes its inputs on the CPU, so using the
GPU would need a change to it.
