# Privacy and security

[← README](../README.md)

What leaves the plugin, and how the plugin and the server are protected. The policy disclosures (network hosts, files
outside the vault, telemetry) are on [Disclosures and compliance](disclosures.md).

## Your notes

- Only the paragraphs on screen (and a few around them, *Context paragraphs*) of notes linked to a colour code are
  sent, with the code's colour names and descriptions, to the model server on `127.0.0.1`. A note with no colour code
  is never sent.
- The results are cached in the plugin's folder, one file per note, holding only the current paragraphs' results
  ([The cache](performance.md#the-cache)). Delete the `cache/` folder to clear them.
- Nothing is sent off your computer unless you set *Server address* to another machine yourself. Nothing is collected
  about you or your use.
- The debug log, off by default, is written to the plugin's folder and nowhere else.

## The server

- It listens on `127.0.0.1` only, so other computers cannot reach it.
- Any web page open in a browser on your computer can send a simple request to `127.0.0.1`, so the server refuses every
  request other than GET or OPTIONS that names a web origin other than Obsidian's own (desktop, iOS, Android) or one you
  added with `--origin`. A web page therefore cannot stop the server, free its GPU memory or send it work.
- It downloads nothing when it is started to serve by the plugin. The downloads you start yourself are pinned to a
  commit and every file is checked against its published size and checksum before use
  ([Models](models.md#downloads-checksums-and-conversion)).
- Once the models are on disk it needs no network.

## The plugin

- No `innerHTML`, `outerHTML` or `insertAdjacentHTML`: text is set with `textContent` and the DOM API.
- No `eval`. The worker runs the same bundled bindings from a Blob URL, never code from elsewhere.
- The only program it starts is the server, at the path in its settings, with arguments built as a list (no shell),
  never including `--download`.
- Rule patterns use regex-lite, which never backtracks: no pattern can hang Obsidian
  ([Safe patterns](colour-codes.md#rules)).
