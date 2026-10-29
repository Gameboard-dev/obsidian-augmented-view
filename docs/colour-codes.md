# Colour codes and rules

[← README](../README.md)

- [Colour codes](#colour-codes)
- [The default codes](#the-default-codes)
- [Themes and opacity](#themes-and-opacity)
- [Linking codes to notes](#linking-codes-to-notes)
- [What the models are told](#what-the-models-are-told)
- [Rules](#rules)
- [Exporting a note's colours](#exporting-a-notes-colours)

## Colour codes

A **colour code** is a whole key, such as the default *Reading* or *Methods*; a **colour** is one row of it, one
category. You can have any number of codes, each with a name and a list of colours (up to 24 are used). Each colour has:

- a name;
- a hex colour, and optionally a dark-theme colour;
- its own opacity;
- a **description**, which the models read to decide what the colour marks;
- its **level**: *clause* (a pale background over a whole sentence) or *phrase* (a few words, drawn over the clause
  colour);
- its rules ([below](#rules)).

The same hex may appear in several codes, or twice in one code. Codes and colours are edited, added and removed in
Settings → Augmented View, where each colour row also has a button that opens its rules.

The plugin draws background tints only: bold, italics and underlines are left for you to write in the text.

## The default codes

| Hex | Reading | Level | Methods | Level |
|---|---|---|---|---|
| #FFD2D2 | Claims | clause | Questions | clause |
| #FFE0B2 | Signposts | clause | Frameworks | phrase |
| #FFF59D | Key terms | phrase | Definitions | clause |
| #C8E6C9 | Findings | clause | Analysis | phrase |
| #B3E5FC | Evidence and context | clause | Data | phrase |
| #E1BEE7 | Limitations | clause | Design limits | clause |
| #D7CCC8 | Connections | clause | Design type | phrase |
| #CFD8DC | Citations | phrase | Ethics | clause |

Each has a one-sentence description (for example, Claims: "The writer's own arguments and conclusions."), which you
can sharpen: the descriptions matter more than anything else to how well sentences are coloured
([Models](models.md#how-sentence-colours-are-chosen)). Some colours also show a kind found by rule: Key terms shows key
terms and definitions, Evidence and context shows quotations, Citations shows citations, and the Methods code's
Definitions shows definitions.

## Themes and opacity

- **Light themes** use the hex as given.
- **Dark themes** use a deeper, more saturated version of the same hue, so light text stays readable: the hex is
  converted to HSL, its saturation raised to at least 55% and its lightness set to 32%. A colour's own dark-theme
  colour, when given, replaces the automatic one.
- The opacity applies in both themes; a phrase tint is drawn over a clause tint, each at its own colour's opacity.
- All of it is one stylesheet of CSS variables generated from the settings (`core/src/colours.rs`), so changing a
  colour or an opacity redraws at once without asking the worker or the server. The floating key's swatches show the
  colours exactly as the notes paint them.

## Linking codes to notes

- A link is on a note or on a folder, and is made from the [floating key](usage.md#the-floating-key). A folder's link
  applies to everything inside it; the most specific link wins, so a note's own link wins over its folder's.
- "None" is a valid link: the note is not marked.
- A note with no link is not marked and nothing of it is sent to the server; the key shows "No colour code" with a
  picker.
- The links are kept in the plugin's settings (data.json), so notes are never changed and the links sync with the
  vault. They are listed in the settings, where they can be changed or removed, and they follow renamed or moved notes
  and folders.

## What the models are told

With each request the server receives the note's code: each colour's name, description and level. The plugin's cache
key includes a hash of that list, so changing a description re-reads only the paragraphs of notes using that code,
while changing a colour, an opacity or a rule never asks the server.

The boundaries of what is coloured come from the parse (sentences, UDPipe's noun phrases and clauses); the instruct
model only chooses, for each sentence, which colour fits, or "none of these" (it is told to choose that when no colour
fits well), so it never has to write spans out (language models are weakest at span boundaries). Phrase colours with no
kind or rule of their own are found by GLiNER2 ([Models](models.md#what-is-found-and-by-what)).

The parser reads the text with its markup blanked, and the server corrects the sentences where that misleads it: a
sentence ending in a narrative citation ("… comes from @baker2019.") or starting with a link after a citation ("…
[[EV-1]]. [[q#^rq|RQ2]] asks …") is split from the next; a "sentence" that does not end with `.`, `?` or `!` (one cut at
a colon, or at a line break inside a link's words) is joined to the next, as is one followed by a piece starting with a
small letter or a possessive ("Lee et al." | "'s results", or LaTeX's "et al.\ label"); and a sentence opening with
emphasis ("*Note:* …") starts at its markers.

**A colour for what you do not want marked.** A clause colour whose box is off in the floating key (hidden) is still
offered to the model; only its drawing is off. When a code's colours cover only some of a text's sentences (a methods
code with no colour for the design and its procedure, say), the model tends to give the rest the nearest colour rather
than "none of these". A hidden colour described as what the others are not ("Sentences about the design, materials and
data rather than its measures …") gives those sentences somewhere to go, and leaves them unmarked
([Models](models.md#a-second-measurement-2026-10-28)).

## Rules

Each colour of a code can have rules of your own, which win over the models: Settings → Augmented View → **Rules**, or
a colour row's list button, which opens the section on that colour (scrolled to and flashed). Choose a code at the top;
under each of its colours are its rules, each with:

- **Force** or **Forbid**;
- **Words** (separated by commas: whole words, in any case, never inside links, code, maths, comments or citations, as
  for *Always mark*) or a **Pattern** (a regular expression in [regex-lite](https://docs.rs/regex-lite)'s syntax,
  matched against the note's Markdown; `(?i)` for any case; `^` and `$` are a sentence's start and end);
- a switch, to keep a rule without applying it, and a button to delete it.

A rule is checked as you type: one that is not valid says why under it and is not applied. **Test on a sentence**, at
the end of the section, shows which rules match a sentence you type and the colour that results.

**Precedence.** For each sentence of a paragraph (a clause colour's rules):

1. a colour whose **Forbid** rule matches the sentence is never given to it (also when one of its own Force rules
   matches);
2. otherwise the first colour, in the code's order, whose **Force** rule matches gives the sentence its colour,
   whatever the model chose, at any sensitivity above zero;
3. otherwise the model's colour stays, shown or not by the sensitivity slider.

Quotations still win over a sentence's colour. A **phrase** colour's Force rules mark what they match in that colour (a
word list here works like *Always mark*, in the colour of your choice); its Forbid rules drop the model's phrases of
that colour (key terms, definitions, GLiNER2's phrases) that they overlap. The sentences are the model's; where it has
not answered yet, or left a sentence without a colour, the worker splits the text itself (at `.`, `?` and `!` before a
capital, a digit, a link or a citation, not after "et al.", "e.g.", "p." or an initial), so a rule shows before the
server has answered.

Rules are kept with their colour in the plugin's settings and applied in the plugin's worker (`core/src/rules.rs`) to
the results it holds, so a change shows at once, also on cached paragraphs, and never asks the server.

**Safe patterns.** regex-lite never backtracks, so a search takes time linear in the text whatever the pattern: no
pattern can hang Obsidian. A pattern is limited to 200 characters, its compiled size and nesting are capped, one that
matches empty text is refused (it would match every sentence), and each is compiled once when the settings change.

**Examples** of rules that fix common mistakes of the model (about half of its choices agree with a careful reader):

| Colour | Rule | For sentences such as |
|---|---|---|
| a limitations colour | Force, pattern `^(First\|Second\|Third), no\b` | "First, no study measures …" (given Findings) |
| a signposts colour | Force, pattern `^(Another\|A further\|A second) \w+ (is\|concerns)` | "Another consideration is how the codes are spaced." |
| a methods limitations colour | Force, words `threat, threats, confound, confounds` | "Three threats lie in the data." |

## Exporting a note's colours

Off by default. Turn on **Export colours** (Settings → Augmented View → Export), and the command **Export this note
with its colours** appears in the command palette and in a note's file menu. It writes a **new** note with the note's
text, each coloured sentence wrapped in a marker; the note itself is never changed.

- **Which colours.** The ones the editor shows for the note: its colour code, the sensitivity, the rules. Colours
  hidden in the floating key are left out, as they are not drawn. Phrase colours (key terms, citations and the like)
  are left out unless **Include phrases** is on; then they are marked inside their sentences' markers. Hiding emphasis
  (*Show emphasis* off) does not empty an export.
- **Colours not computed yet.** Paragraphs the server has not coloured yet (the note was never scrolled through, say)
  are sent first, all of them, in the same requests the views make, so the same GPU settings apply. A notice says how
  many paragraphs it is waiting for, and the server is started if it is not running (as the status bar's *Start* does,
  with its question when the GPU has no room). If no server can answer, the export is written with the colours there
  are, and the notice says how many paragraphs were left plain.
- **Where it goes.** `<note's name> (colours).md` next to the note, or in **Export folder**, a folder in the vault that
  you type (it is made if it does not exist). An export already there is replaced only after you say so. Files are
  written through Obsidian's Vault API, inside the vault only.
- **What else changes.** Nothing: uncoloured sentences stay as they are, and the export opens with one line saying
  which colour code was used and when (`Colours exported by Augmented View with the colour code "Reading" on
  2026-10-29 14:05.`, after the note's properties if it has any). The properties are copied as they are, so an export
  of a note with aliases has the same aliases.

**Markers.** *Marker format* chooses between two forms. The colour is the colour's hex as its code defines it, the
category its name:

```markdown
~={#FFD2D2, Claims}Detectors fail on new data.=~ The datasets are small.
```

```markdown
<mark data-colour="#FFD2D2" data-category="Claims">Detectors fail on new data.</mark> The datasets are small.
```

With **Include phrases**, a phrase sits inside its sentence: `~={#FFD2D2, Claims}Detectors fail on
~={#FFF59D, Key terms}new data=~.=~`. In the `~=` form, a colour's name loses any `~` and has `{` `}` written as
`(` `)`, so it cannot end the marker; the colour comes before the first comma. In the HTML form, the attributes are
escaped, and a stray `<`, `>` or `&` in the marked text is written as an entity (`&lt;`), which Markdown shows as the
same character; code, links and HTML tags are left as they are.

No Obsidian plugin we know of uses `~={…}…=~`: the coloured-highlight plugins write a colour inside Obsidian's own
highlight, `=={colour}text==` (Extended Markdown Syntax, Style Obmd, Hilo), so the export does not use `==`, and an
exported note shows its markers as plain text unless something reads them.

**The Markdown stays intact.** A marker covers only the text of a line, so it never takes in:

- front matter, code blocks, maths blocks, `%%` and `<!-- -->` comment blocks, dividers, embeds or block-id lines;
- a line's indent, blockquote `>`, callout header, list marker, task box, heading `#`, footnote label, a trailing
  block id (` ^id`) or its line break: a sentence over two lines gets a marker on each;
- part of inline code, inline maths, a comment, a link or embed (its target or its text), a footnote, a citation, an
  HTML tag or a bare URL: a marker that would start or end inside one takes all of it;
- a table's `|`: the marker is split around it.

Where a phrase would cross the end of its sentence, it is split there. Where a `~=` marker would touch a `~` or an `=`
and make `~~` (strikethrough) or `==` (a highlight) that was not in the note, an empty HTML comment, `<!---->`, which
Obsidian does not show, is put between them. The code is `core/src/export.rs`.
