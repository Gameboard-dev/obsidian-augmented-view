//! Colour codes (docs/colour-codes.md): keys of colours, each colour a category drawn as a background tint, never bold,
//! italic or underline. A colour marks a clause (a pale `background-color`) or a phrase (a `background-image` drawn
//! over it, so a key term inside a claim still shows); each has its own opacity. Light themes use the hex as given;
//! dark themes a deeper version of the same hue ([`dark`]), so light text stays readable. Everything is one generated
//! stylesheet ([`stylesheet`]): changing a colour or an opacity never asks the worker or the server.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ColourCode {
    /// Stable, for links and the stylesheet's selectors: lower-case letters, digits and hyphens ([`ensure_ids`]).
    pub id: String,
    pub name: String,
    pub colours: Vec<Colour>,
}

impl Default for ColourCode {
    fn default() -> Self {
        ColourCode { id: String::new(), name: "New colour code".into(), colours: Vec::new() }
    }
}

/// A colour code linked to a note (`Library/paper.md`) or a folder and everything in it (`Library/`). An empty `code`
/// means the note or folder is not marked.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Link {
    pub path: String,
    pub code: String,
}

/// What applies to a note.
#[derive(Clone, Debug, PartialEq)]
pub enum Assigned {
    /// Nothing linked: the key asks, and nothing is sent to the server.
    Unset,
    /// Linked to "not marked".
    Off,
    /// The index of the code in the list.
    Code(usize),
}

/// The most specific link that covers `path`: the note's own, else its nearest folder's. A link to a code that no
/// longer exists counts as unset.
pub fn assigned(codes: &[ColourCode], links: &[Link], path: &str) -> Assigned {
    match link_for(links, path) {
        None => Assigned::Unset,
        Some(l) if l.code.is_empty() => Assigned::Off,
        Some(l) => codes.iter().position(|c| c.id == l.code).map_or(Assigned::Unset, Assigned::Code),
    }
}

/// The link that applies to `path`: its own, else its nearest folder's (the vault's root, `/`, last).
pub fn link_for<'a>(links: &'a [Link], path: &str) -> Option<&'a Link> {
    let covers = |l: &&Link| l.path == path || l.path == "/" || (l.path.ends_with('/') && path.starts_with(&l.path));
    links.iter().filter(covers).max_by_key(|l| (l.path == path, l.path.len()))
}

/// The folder a note is in, as a link path (`a/b/`; the vault's root is `/`).
pub fn folder_of(path: &str) -> String {
    path.rfind('/').map_or("/".to_string(), |i| path[..=i].to_string())
}

/// Link `path` (a note, or its folder when `folder`) to `code` ("" for not marked), replacing a link already there.
pub fn link(links: &mut Vec<Link>, path: &str, code: &str, folder: bool) {
    let path = if folder { folder_of(path) } else { path.to_string() };
    links.retain(|l| l.path != path);
    links.push(Link { path, code: code.to_string() });
}

/// Links follow a renamed or moved note or folder.
pub fn rename(links: &mut [Link], from: &str, to: &str) {
    for l in links {
        if l.path == from {
            l.path = to.to_string();
        } else if let Some(rest) = l.path.strip_prefix(&format!("{from}/")) {
            l.path = format!("{to}/{rest}");
        }
    }
}

/// Give every code an id: kept if it has one, else made from its name, unique in the list.
pub fn ensure_ids(codes: &mut [ColourCode]) {
    for i in 0..codes.len() {
        if !codes[i].id.is_empty() {
            continue;
        }
        let slug: String = codes[i].name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let base = slug.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
        let base = if base.is_empty() { "code".to_string() } else { base };
        let (mut id, mut n) = (base.clone(), 2);
        while codes.iter().any(|c| c.id == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        codes[i].id = id;
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    #[default]
    Clause,
    Phrase,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Colour {
    pub name: String,
    pub hex: String,
    /// The dark-theme colour; empty: derived from `hex`.
    pub dark: String,
    /// 0 to 1.
    pub opacity: f32,
    pub level: Level,
    /// What the colour marks, in words: for the key, and for the model once it classifies (step c).
    pub description: String,
    /// Which of the server's kinds and the worker's rules this colour shows ("term", "stress", "quote", "definition",
    /// "citation"; "point" and "main" from before the model classified sentences). A clause colour is also found by
    /// the model (its description); where one of its rules finds something, the rule wins.
    pub kinds: Vec<String>,
    /// Hidden from the notes (its check box in the floating key off): no rule in the stylesheet, so what it marks is
    /// not drawn (user, 2026-10-28).
    pub hidden: bool,
    /// The user's own rules for this colour (rules.rs, [`Rule`]): applied in the worker, they win over the model.
    pub rules: Vec<Rule>,
}

impl Default for Colour {
    fn default() -> Self {
        Colour { name: "New colour".into(), hex: "#FFF59D".into(), dark: String::new(), opacity: 0.6, level: Level::Clause, description: String::new(), kinds: Vec::new(), hidden: false, rules: Vec::new() }
    }
}

/// How a rule finds what it applies to.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    /// Words and phrases, separated by commas: whole words, in any case, outside links, code, maths and citations
    /// (bold.rs's matching).
    #[default]
    Words,
    /// A regular expression (regex-lite's syntax), at most `rules::MAX_PATTERN` characters.
    Pattern,
}

/// What a rule does where it matches.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// A clause colour: a sentence it matches takes this colour, whatever the model chose. A phrase colour: what it
    /// matches is marked in this colour.
    #[default]
    Force,
    /// A clause colour: a sentence it matches never takes this colour (the model's choice is dropped, and this
    /// colour's own Force rules do not apply). A phrase colour: the model's phrases of this colour that it overlaps
    /// are dropped.
    Forbid,
}

/// One of the user's rules for a colour (docs/colour-codes.md, "Rules"; docs/colour-codes.md, "Rules"). Kept in data.json with its colour.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    pub kind: RuleKind,
    pub action: Action,
    /// The words (comma-separated) or the pattern.
    pub text: String,
    /// Off: kept but not applied.
    pub on: bool,
}

impl Default for Rule {
    fn default() -> Self {
        Rule { kind: RuleKind::Words, action: Action::Force, text: String::new(), on: true }
    }
}

/// Two plain codes, one per column: reading the literature, and a study's methods. A user's own codes are kept in
/// their settings.
pub fn defaults() -> Vec<ColourCode> {
    let c = |name: &str, hex: &str, level: Level, description: &str, kinds: &[&str]| Colour {
        name: name.into(),
        hex: hex.into(),
        opacity: if level == Level::Phrase { 0.9 } else { 0.6 },
        level,
        description: description.into(),
        kinds: kinds.iter().map(|k| k.to_string()).collect(),
        ..Colour::default()
    };
    use Level::{Clause, Phrase};
    vec![
        ColourCode {
            id: "reading".into(),
            name: "Reading".into(),
            colours: vec![
                c("Claims", "#FFD2D2", Clause, "The writer's own arguments and conclusions.", &[]),
                c("Signposts", "#FFE0B2", Clause, "Sentences that organise the text: what comes next, a summary, a change of topic.", &[]),
                c("Key terms", "#FFF59D", Phrase, "Terms and their definitions.", &["term", "definition"]),
                c("Findings", "#C8E6C9", Clause, "What a study found or measured.", &[]),
                c("Evidence and context", "#B3E5FC", Clause, "What a source did or describes, how something works, quotations, background.", &["quote"]),
                c("Limitations", "#E1BEE7", Clause, "Weaknesses, contradictions and open questions.", &[]),
                c("Connections", "#D7CCC8", Clause, "Links to the questions, to other sections or to other work.", &[]),
                c("Citations", "#CFD8DC", Phrase, "In-text citations.", &["citation"]),
            ],
        },
        ColourCode {
            id: "methods".into(),
            name: "Methods".into(),
            colours: vec![
                c("Questions", "#FFD2D2", Clause, "Sentences that state the questions or hypotheses the study answers.", &[]),
                c("Frameworks", "#FFE0B2", Phrase, "Names of paradigms and theoretical frameworks.", &[]),
                c("Definitions", "#FFF59D", Clause, "Sentences that say how a concept is measured or bounded in the study.", &["definition"]),
                c("Analysis", "#C8E6C9", Phrase, "Names of analysis procedures, statistical tests, models and software.", &[]),
                c("Data", "#B3E5FC", Phrase, "Names of datasets, instruments, samples and participants.", &[]),
                c("Design limits", "#E1BEE7", Clause, "Sentences about weaknesses of the design and threats to validity.", &[]),
                c("Design type", "#D7CCC8", Phrase, "Names of the study's design (an experiment, a case study, a survey).", &[]),
                c("Ethics", "#CFD8DC", Clause, "Sentences about approval, consent, data protection and licences.", &[]),
            ],
        },
    ]
}

/// The colours the model looks for: a code's clause colours, with their places in it.
pub fn model_colours(code: &ColourCode) -> Vec<(usize, &Colour)> {
    code.colours.iter().enumerate().filter(|(i, c)| c.level == Level::Clause && *i < crate::pick::MAX_COLOURS).collect()
}

/// The phrase colours GLiNER2 looks for (the server's phrases.rs): those no kind or rule finds.
pub fn model_phrases(code: &ColourCode) -> Vec<(usize, &Colour)> {
    code.colours.iter().enumerate().filter(|(i, c)| c.level == Level::Phrase && c.kinds.is_empty() && *i < crate::pick::MAX_COLOURS).collect()
}

/// The kinds (a mask, pick::cut) whose finds a clause colour of the code shows: they win over the model's colour.
pub fn rule_mask(code: &ColourCode) -> u32 {
    use crate::pick::{CITATION, DEFINITION, QUOTE};
    let kinds: Vec<&str> = code.colours.iter().filter(|c| c.level == Level::Clause).flat_map(|c| c.kinds.iter().map(String::as_str)).collect();
    [("quote", QUOTE), ("definition", DEFINITION), ("citation", CITATION)].iter().filter(|(k, _)| kinds.contains(k)).fold(0, |m, (_, b)| m | (1 << b))
}

/// `#RRGGBB` or `#RGB`.
pub fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().trim_start_matches('#');
    let v = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        6 => Some((v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?)),
        3 => Some((v(&h[0..1])? * 17, v(&h[1..2])? * 17, v(&h[2..3])? * 17)),
        _ => None,
    }
}

/// The dark-theme version of a colour: the same hue, saturation at least 55%, lightness 32%
/// (<https://en.wikipedia.org/wiki/HSL_and_HSV#From_RGB>).
pub fn dark((r, g, b): (u8, u8, u8)) -> (u8, u8, u8) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (l, d) = ((max + min) / 2.0, max - min);
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()) };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let (s, l) = (if d == 0.0 { 0.0 } else { s.max(0.55) }, 0.32);
    let c = (1.0 - (2.0 * l - 1.0f32).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// The classes the editor and Reading view put on marked text (editor.rs, view.rs), with the kinds each shows.
const CLASSES: [(&str, &[&str]); 6] = [("av-term", &["term"]), ("av-stress", &["stress"]), ("av-point", &["point", "main"]), ("av-quote", &["quote"]),
    ("av-definition", &["definition"]), ("av-citation", &["citation"])];

/// The stylesheet for every code, each scoped to the editors and Reading views of the notes it applies to (they carry
/// `data-av-code`, editor.rs and view.rs).
pub fn stylesheet_all(codes: &[ColourCode]) -> String {
    codes.iter().map(stylesheet).collect()
}

/// A colour as the notes paint it, (light theme, dark theme), opacity included: the key's swatches show exactly
/// these (user, 2026-10-27: the key's colours were paler than the note's in dark mode).
pub fn painted(colour: &Colour) -> Option<(String, String)> {
    let light = rgb(&colour.hex)?;
    let deep = rgb(&colour.dark).unwrap_or_else(|| dark(light));
    let a = colour.opacity.clamp(0.0, 1.0);
    let css = |(r, g, b): (u8, u8, u8)| format!("rgba({r}, {g}, {b}, {a:.2})");
    Some((css(light), css(deep)))
}

/// The stylesheet for a code: each class takes the first colour that shows one of its kinds, and each clause colour
/// its own class for the sentences the model gives it (`av-c0`, `av-c1` …, pick::class).
pub fn stylesheet(code: &ColourCode) -> String {
    let scope = format!("[data-av-code=\"{}\"]", code.id);
    let mut css = String::new();
    let mut rule = |class: &str, colour: &Colour| {
        if colour.hidden {
            return;
        }
        let Some(light) = rgb(&colour.hex) else { return };
        let deep = rgb(&colour.dark).unwrap_or_else(|| dark(light));
        let a = colour.opacity.clamp(0.0, 1.0);
        let paint = |(r, g, b): (u8, u8, u8)| match colour.level {
            Level::Clause => format!("background-color: rgba({r}, {g}, {b}, {a:.2});"),
            Level::Phrase => format!("background-image: linear-gradient(rgba({r}, {g}, {b}, {a:.2}), rgba({r}, {g}, {b}, {a:.2}));"),
        };
        css += &format!("{scope} .{class} {{ {} }}\n.theme-dark {scope} .{class} {{ {} }}\n", paint(light), paint(deep));
    };
    for (class, kinds) in CLASSES {
        if let Some(colour) = code.colours.iter().find(|c| c.kinds.iter().any(|k| kinds.contains(&k.as_str()))) {
            rule(class, colour);
        }
    }
    // the colours the models find, and any colour with rules of its own (a phrase colour's words a rule marks)
    let own = code.colours.iter().enumerate().filter(|(i, c)| *i < crate::pick::MAX_COLOURS && c.rules.iter().any(|r| r.on));
    let mut seen = Vec::new();
    for (i, colour) in model_colours(code).into_iter().chain(model_phrases(code)).chain(own) {
        if !seen.contains(&i) {
            seen.push(i);
            rule(&crate::pick::class(crate::pick::COLOUR + i as u8), colour);
        }
    }
    css
}

/// The colour of the code a mark of kind `k` is drawn in, as the stylesheet draws it: a sentence's or phrase's colour
/// by its place, any other kind in the first colour that shows it; None when nothing is drawn (no such colour, one
/// hidden in the floating key, or one whose hex is not a colour). Used by the copy with highlights (highlights.rs).
pub fn colour_of_kind(code: &ColourCode, k: u8) -> Option<usize> {
    let i = if k >= crate::pick::COLOUR {
        (k - crate::pick::COLOUR) as usize
    } else {
        let class = crate::pick::class(k);
        let kinds = CLASSES.iter().find(|(c, _)| *c == class)?.1;
        code.colours.iter().position(|c| c.kinds.iter().any(|x| kinds.contains(&x.as_str())))?
    };
    let c = code.colours.get(i)?;
    (!c.hidden && rgb(&c.hex).is_some()).then_some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_takes_the_colour_the_stylesheet_draws_it_in() {
        use crate::pick::{CITATION, COLOUR, STRESS, TERM};
        let mut code = defaults()[0].clone();
        assert_eq!(colour_of_kind(&code, COLOUR + 3), Some(3), "a sentence: its colour");
        assert_eq!(colour_of_kind(&code, TERM), Some(2), "key terms");
        assert_eq!(colour_of_kind(&code, CITATION), Some(7));
        assert_eq!(colour_of_kind(&code, STRESS), None, "no colour shows contrast words");
        assert_eq!(colour_of_kind(&code, COLOUR + 20), None);
        code.colours[3].hidden = true;
        assert_eq!(colour_of_kind(&code, COLOUR + 3), None, "hidden in the key: not drawn, not copied");
    }

    #[test]
    fn hex_and_dark() {
        assert_eq!(rgb("#FFD2D2"), Some((255, 210, 210)));
        assert_eq!(rgb("abc"), Some((170, 187, 204)));
        assert_eq!(rgb("#12"), None);
        let (r, g, b) = dark((255, 245, 157)); // Gemini's yellow: still yellow, much deeper
        assert!(r > b && g > b && (r as u32 + g as u32 + b as u32) < 360, "{r} {g} {b}");
        assert_eq!(dark((200, 200, 200)), (82, 82, 82), "a grey stays grey");
    }

    #[test]
    fn stylesheet_maps_kinds_to_the_first_colour_with_light_and_dark_rules() {
        let css = stylesheet(&defaults()[0]);
        assert!(css.contains("[data-av-code=\"reading\"] .av-c0 { background-color: rgba(255, 210, 210, 0.60); }"), "the model's sentences");
        assert!(css.contains("[data-av-code=\"reading\"] .av-c6 { background-color:") && !css.contains(".av-c7 "), "clause colours only");
        assert!(css.contains("[data-av-code=\"reading\"] .av-term { background-image: linear-gradient(rgba(255, 245, 157, 0.90)"));
        assert!(css.contains(".theme-dark [data-av-code=\"reading\"] .av-quote { background-color:"));
        assert!(css.contains("[data-av-code=\"reading\"] .av-citation { background-image: linear-gradient(rgba(207, 216, 220"));
        assert!(!css.contains("av-stress") && !css.contains("av-point"), "contrast words and points have no colour in this code");
        let mut code = defaults()[0].clone();
        code.colours[2].dark = "#334455".into();
        assert!(stylesheet(&code).contains(".theme-dark [data-av-code=\"reading\"] .av-term { background-image: linear-gradient(rgba(51, 68, 85, 0.90)"));
    }

    #[test]
    fn the_defaults_rules_and_model_colours() {
        let codes = defaults();
        assert_eq!(rule_mask(&codes[0]), 1 << crate::pick::QUOTE, "quotations win over the model's colour; definitions and citations are phrases here");
        assert_eq!(model_colours(&codes[0]).iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 1, 3, 4, 5, 6]);
        assert!(model_phrases(&codes[0]).is_empty(), "key terms and citations have kinds");
        assert_eq!(model_phrases(&codes[1]).iter().map(|(i, _)| *i).collect::<Vec<_>>(), [1, 3, 4, 6]);
    }

    #[test]
    fn the_most_specific_link_applies_and_links_follow_renames() {
        let codes = defaults();
        let mut links = Vec::new();
        assert_eq!(assigned(&codes, &links, "Library/a.md"), Assigned::Unset);
        link(&mut links, "Library/a.md", "reading", true);
        link(&mut links, "Library/sub/b.md", "methods", false);
        link(&mut links, "prompts/x.md", "", true);
        assert_eq!(assigned(&codes, &links, "Library/a.md"), Assigned::Code(0), "the folder's");
        assert_eq!(assigned(&codes, &links, "Library/sub/c.md"), Assigned::Code(0), "a sub-folder inherits");
        assert_eq!(assigned(&codes, &links, "Library/sub/b.md"), Assigned::Code(1), "the note's own wins");
        assert_eq!(assigned(&codes, &links, "prompts/y.md"), Assigned::Off);
        assert_eq!(assigned(&codes, &links, "Libraryx/a.md"), Assigned::Unset, "a folder covers only its own notes");
        rename(&mut links, "Library", "Papers");
        assert_eq!(assigned(&codes, &links, "Papers/sub/b.md"), Assigned::Code(1));
        assert_eq!(assigned(&codes, &links, "Papers/z.md"), Assigned::Code(0));
        link(&mut links, "top.md", "gone", false);
        assert_eq!(assigned(&codes, &links, "top.md"), Assigned::Unset, "a removed code counts as unset");
        link(&mut links, "top.md", "methods", true);
        assert_eq!(assigned(&codes, &links, "other.md"), Assigned::Code(1), "the vault's root covers every note");
        assert_eq!(folder_of("top.md"), "/");
    }

    #[test]
    fn ids_are_made_unique_from_names() {
        let mut codes = vec![ColourCode { name: "My Code!".into(), ..Default::default() }, ColourCode { name: "my code".into(), ..Default::default() }, ColourCode::default()];
        ensure_ids(&mut codes);
        assert_eq!(codes.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["my-code", "my-code-2", "new-colour-code"]);
    }

    #[test]
    fn settings_without_colour_codes_take_the_defaults() {
        let s: crate::msg::Settings = serde_json::from_str(r#"{"port": 8790}"#).unwrap();
        assert_eq!(s.colour_codes, defaults());
        let back: crate::msg::Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.colour_codes[1].colours[3].level, Level::Phrase);
    }
}
