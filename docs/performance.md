# Performance and cache

[← README](../README.md)

- [In the plugin](#in-the-plugin)
- [What is sent, and when](#what-is-sent-and-when)
- [In the server](#in-the-server)
- [The cache](#the-cache)

## In the plugin

- **Nothing that costs time runs on Obsidian's main thread.** Splitting, hashing, planning, the protocol, choosing
  spans and Reading-view mapping run in a Web Worker (on a 1.6 MB, 5,000-paragraph note: split and hash 10 ms, plan
  0.4 ms, marks 0.1 ms). The main thread forwards events, makes the requests, reads and writes cache files, and draws.
  `bench_viewport` (`cargo test --release -p augmented-view-core bench_viewport -- --ignored --nocapture`, native)
  measures the worker on a 5,000-paragraph note whose paragraphs all have results: a viewport render (the plan, then
  the marks of 40 rendered paragraphs) about 0.12 ms, a Reading-view block mapped and wrapped 0.07 ms, an edit (split
  and hash) 1.8 ms, a 400-paragraph selection copied with its highlights 7.5 ms (2026-10-29, before and after the
  marks moved to the shared crate view-marks: the same within the noise).
- **The editor never scans the note.** An edit maps the existing marks through the change at once; the text goes to
  the worker only after 400 ms without typing; the visible range at most once a frame; marks come back for the
  editor's rendered range only.
- **Reading view touches only what is near the screen** (an IntersectionObserver, one screen either side), in animation
  frames with a 6 ms budget; elements are held weakly and forgotten when Obsidian discards them.
- **Colours, rules and the slider never ask the server.** Colours and opacities are one generated stylesheet; rules,
  *Always mark* and the sensitivity are applied in the worker to the results it holds.

## What is sent, and when

What is on screen goes first, and the rest only when the server is idle:

- The paragraphs on screen are sent once the view has rested 150 ms, so a fast scroll sends nothing. Up to
  *Paragraphs per request* (64) go in a request.
- *Context paragraphs* (default 1) either side follow once the view has rested 1 s, nothing on screen is left to ask
  for and no request is running. They go nearest first, two to a request, so a scroll never waits behind a long one.
- One request per layer runs at a time, streamed back paragraph by paragraph. A failing server is retried after 1, 2,
  4 … 30 s.
- Before 2026-10-25 the context (then 5 paragraphs either side) went in the same request as the screen: opening a page
  sent about 12 paragraphs through the language model at once, keeping the GPU at full power for about 10 s.

## In the server

- **It parses in parallel and runs the models one paragraph at a time.** A request's paragraphs are parsed on all CPU
  cores (one UDPipe model per thread, loaded at start-up) while the models work through them in order, each result
  sent as soon as it is done. The two layers (key terms; sentence colours) have their own threads but take turns on
  the GPU, never loading it together.
- **The language model reads each paragraph's listing once per order, not once per question** (the attention cache is
  reused for both questions on the GPU): two forward passes over the listing instead of four.
- **On the CPU** the embedding model encodes the sentences and the candidate phrases as separate batches, so short
  phrases are not padded to a sentence's length: 4 paragraphs' key terms in 10.9 s instead of 54 s, the same results
  as the GPU's (2026-10-24).
- **Measured** on 8 paragraphs in one request (RTX 4060 Ti, before the changes above): terms and phrases 97 ms a
  paragraph, points and main point 300 ms (the Python server it replaced: 66 and 323 ms). Sentence colours take about
  1.4 s a paragraph on the GPU, phrase colours about 2.3 s on the CPU. Start-up about 2.5 s once the models are on
  disk. Batching the models across paragraphs was measured slower and changed the 4-bit model's results.

## The cache

One small binary file per note in the plugin's `cache/` folder (named by a hash of the note's path), read only when that
note is opened. Results are kept by the hash of each paragraph's text, so:

- **an unchanged paragraph is never sent again**, also after Obsidian restarts;
- **editing one paragraph re-sends only that one**, once it has been unchanged for 1.5 s (typing does not send every
  draft): the editor reports each change, the worker splits the note into paragraphs and hashes each one, and only a
  paragraph whose hash it has no result for is sent; editing N paragraphs sends those N;
- **nothing accumulates**: the note's file is rewritten with its current paragraphs' results only, a deleted note's
  file is removed (and files of notes deleted while Obsidian was closed are removed by name at start-up), and a renamed
  note's file is renamed;
- results made by other models, another *Diversity*, another server protocol or another colour code's descriptions are
  ignored, not shown.
