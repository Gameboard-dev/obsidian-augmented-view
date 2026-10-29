# Using Augmented View

[← README](../README.md)

- [The status bar](#the-status-bar)
- [Commands](#commands)
- [The floating key](#the-floating-key)
- [Always mark as a key term](#always-mark-as-a-key-term)
- [The sensitivity slider](#the-sensitivity-slider)
- [How the colours are drawn](#how-the-colours-are-drawn)

## The status bar

The **AugView** item shows where the models run ("AugView 50% · CUDA", "· CPU", "· CUDA + CPU" when each model runs on
a different one, or "· Metal") and a dot: grey stopped, orange starting or stopping, green running, red when a reading
is at or above *Warn at*. Its tooltip gives the readings and why anything is not on the GPU
([GPU, memory and fallbacks](gpu-and-memory.md)).

Click it for a menu: *Start* or *Stop the server*, the readings, *Show* or *Hide emphasis*, and the sensitivity levels.

## Commands

*Start or stop the server*, *Show or hide emphasis*, *More emphasis*, *Less emphasis*, and, with text selected in an
editor, *Copy selection with highlights* ([Copying with highlights](colour-codes.md#copying-with-highlights): the
selection to the clipboard with its colours as markers, also in the editor's right-click menu). None has a default
hotkey.

## The floating key

A small panel over the workspace that shows the open note's colour code, each colour as a chip (a thin swatch for a
phrase colour), with a check box that shows or hides that colour in the notes. Hover a chip for what it marks.

- **Choosing a code.** Its picker links the note to a code, or to none. With the box ticked it links the note's folder
  instead (for a note at the vault's root, the whole vault). A note with no code is not marked, and nothing of it is
  sent to the server until one is chosen. There is no pop-up asking, because Obsidian opens many notes in passing.
- **Adding a colour.** **+** adds a colour to the code (its colour and name; the rest in the settings).
- **Moving it.** Drag it by its title bar; fold it with the arrow. Its place and fold are kept for this device.

How links between codes and notes work: [Colour codes and rules](colour-codes.md#linking-codes-to-notes).

## Always mark as a key term

Select up to six words on one line, right-click, and choose **Always mark as a key term**. They are added to the
*Always mark* setting and marked as a key term in every paragraph, each time they appear, whatever the sensitivity:
whole words, in any case, never inside links, code, maths, comments or citations. A term the server sometimes misses is
then never missed. For a listed term the item reads **Stop always marking as a key term** and takes it out.

The list is kept in the plugin's settings in the vault, so devices sharing the vault share it. It is applied in the
plugin's worker to the text, so a change shows at once, also on cached paragraphs, and never asks the server.

## The sensitivity slider

The slider (status bar menu or settings) is applied in the plugin's worker to the ranked results it already holds, so
moving it never asks the server. At sensitivity *s*:

- **a sentence's colour** shows when the model gave it a probability of at least 1 − *s* (every one at 100%, the surer
  half or so at 50%);
- **every quotation, definition and citation** shows whenever *s* > 0 (they are found by rule, not ranked);
- **key terms**: every named one (an acronym, a quoted word, the name a naming sentence gives), then ranked ones, up to
  `round(s × words / 20)` in all (at least one), scoring within `0.04 + 0.16 s` of the paragraph's best;
- **contrast words**: `round(s × n)` of the model's *n* (at least one).

## How the colours are drawn

A clause colour is a pale background over its sentence; a phrase colour is drawn over it, so a key term inside a claim
still shows. Each colour has its own opacity. Light themes use the colour as given, dark themes a deeper version of the
same hue (or the colour's own dark-theme colour), so light text stays readable.

The note is never changed: the colours are CodeMirror decorations in the editor and wrapper spans in Reading view.
Links, citations, comments, code and maths are never emphasised, and a phrase that would run across one is dropped.

Synonyms and definitions, which earlier versions offered on selection, are in a separate plugin, Dictionary and
Synonyms.
