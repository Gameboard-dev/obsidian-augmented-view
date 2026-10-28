//! Rules (COLOUR-CODES.md, "Rules"; README, "Rules"), found in the worker in each paragraph's text, never by the server,
//! so they show at once, also on cached paragraphs, and cost no GPU.
//!
//! - **Built-in:** citations, Pandoc's bracketed `[@key, p. 3]` and `[@a; @b]` and narrative `@key`
//!   (<https://pandoc.org/MANUAL.html#citation-syntax>), outside inline code. Reading view keeps a citation's text
//!   (Zotero Cite Links only wraps its key in a link), so they are found in a rendered block the same way.
//! - **The user's own** ([`CodeRules`]): each colour's rules ([`crate::colours::Rule`]), words or a pattern, forcing or
//!   forbidding the colour. They rewrite a paragraph's spans before the sensitivity rule chooses what to draw, so the
//!   editor and Reading view show the same. Precedence, for each sentence of a paragraph (a clause colour's rules):
//!   1. a colour whose Forbid rule matches the sentence is never given to it;
//!   2. else the first colour, in the code's order, with a Force rule that matches gives the sentence its colour,
//!      whatever the model chose and at any sensitivity above zero;
//!   3. else the model's colour stays (if any), shown by the sensitivity rule as before.
//!
//!   The built-in rules still win over a sentence's colour after that (quotations, pick::cut). A phrase colour's Force
//!   rules mark what they match; its Forbid rules drop the server's phrases of that colour that they overlap.
//!
//! Patterns are regex-lite's (<https://docs.rs/regex-lite>), which never backtracks: a search takes time linear in the
//! text whatever the pattern, so no pattern can hang the worker. A pattern is also limited to `MAX_PATTERN` characters,
//! its compiled size and nesting are capped, one that matches empty text is refused (it would match every sentence),
//! and each is compiled once, when the settings arrive.

use regex_lite::{Regex, RegexBuilder};

use crate::colours::{Action, ColourCode, Level, Rule, RuleKind};
use crate::pick::{self, Span, COLOUR, MAX_COLOURS};

/// The longest pattern accepted, in characters.
pub const MAX_PATTERN: usize = 200;
/// The most a compiled pattern may take (regex-lite's `size_limit`), and how deeply it may nest.
const SIZE_LIMIT: usize = 100 * 1024;
const NEST_LIMIT: u32 = 16;
/// Words before a full stop that do not end a sentence (the worker's own splitting, [`sentences`]).
const ABBREVIATIONS: [&str; 22] = ["al", "e.g", "i.e", "cf", "fig", "figs", "eq", "eqs", "p", "pp", "vs", "etc", "dr", "mr", "ms", "no", "vol",
    "ch", "sec", "approx", "ref", "refs"];

/// A rule ready to run.
pub enum Matcher {
    Words(Vec<Vec<char>>),
    Pattern(Regex),
}

/// A rule's words or pattern, compiled, or why it cannot be (shown beside the rule in the settings).
pub fn compile(rule: &Rule) -> Result<Matcher, String> {
    let text = rule.text.trim();
    if text.is_empty() {
        return Err(match rule.kind {
            RuleKind::Words => "No words yet.".into(),
            RuleKind::Pattern => "No pattern yet.".into(),
        });
    }
    match rule.kind {
        RuleKind::Words => Ok(Matcher::Words(crate::bold::terms(text))),
        RuleKind::Pattern => {
            let n = text.chars().count();
            if n > MAX_PATTERN {
                return Err(format!("The pattern is {n} characters long; the most is {MAX_PATTERN}."));
            }
            let re = RegexBuilder::new(text).size_limit(SIZE_LIMIT).nest_limit(NEST_LIMIT).build().map_err(|e| format!("Not a valid pattern: {e}"))?;
            if re.is_match("") {
                return Err("The pattern matches empty text, so it would match everything.".into());
            }
            Ok(Matcher::Pattern(re))
        }
    }
}

struct Compiled {
    colour: usize,
    forbid: bool,
    m: Matcher,
}

/// A code's rules, compiled: those switched on and valid, of its first `MAX_COLOURS` colours.
pub struct CodeRules {
    rules: Vec<Compiled>,
    levels: Vec<Level>,
    /// The kinds (pick's) each colour draws: its own sentence or phrase class, and the kinds the stylesheet gives it.
    kinds: Vec<Vec<u8>>,
}

/// Which pick kinds the stylesheet draws in each colour of a code (colours::stylesheet: a kind takes the first colour
/// that lists it), with each colour's own `COLOUR + i`.
fn kinds_of(code: &ColourCode) -> Vec<Vec<u8>> {
    let names: [(&str, &[u8]); 6] = [("term", &[pick::TERM]), ("stress", &[pick::STRESS]), ("point", &[pick::POINT, pick::MAIN]), ("quote", &[pick::QUOTE]),
        ("definition", &[pick::DEFINITION]), ("citation", &[pick::CITATION])];
    let mut out: Vec<Vec<u8>> = (0..code.colours.len()).map(|i| vec![COLOUR + i.min(MAX_COLOURS - 1) as u8]).collect();
    for (name, ks) in names {
        if let Some(i) = code.colours.iter().position(|c| c.kinds.iter().any(|k| k == name || (name == "point" && k == "main"))) {
            out[i].extend_from_slice(ks);
        }
    }
    out
}

impl CodeRules {
    /// None when the code has no rule to apply.
    pub fn new(code: &ColourCode) -> Option<CodeRules> {
        let mut rules = Vec::new();
        for (i, c) in code.colours.iter().enumerate().take(MAX_COLOURS) {
            for r in c.rules.iter().filter(|r| r.on) {
                if let Ok(m) = compile(r) {
                    rules.push(Compiled { colour: i, forbid: r.action == Action::Forbid, m });
                }
            }
        }
        (!rules.is_empty()).then(|| CodeRules { rules, levels: code.colours.iter().map(|c| c.level).collect(), kinds: kinds_of(code) })
    }

    fn level(&self, colour: usize) -> Level {
        self.levels.get(colour).copied().unwrap_or_default()
    }

    /// A paragraph's spans (both layers, the server's) with the rules applied: sentences forced to a colour or
    /// relieved of one, phrases marked or dropped. `text` is the paragraph's source.
    pub fn apply(&self, text: &str, spans: &[&Span]) -> Vec<Span> {
        let off = Offsets::new(text);
        let is_sentence = |s: &Span| s.k >= COLOUR && self.level((s.k - COLOUR) as usize) == Level::Clause;
        // the sentences: the model's, and the worker's own splitting of the text the model left without a colour
        let mut model: Vec<&Span> = spans.iter().copied().filter(|s| is_sentence(s) && !s.at.is_empty()).collect();
        model.sort_by_key(|s| s.at[0].0);
        let mut sents: Vec<((u32, u32), Option<&Span>)> = Vec::new();
        let mut pos = 0;
        for s in &model {
            let (a, e) = s.at[0];
            if a < pos {
                continue; // overlaps the one before: the server never sends such, but keep the first
            }
            sents.extend(sentences(text, pos, a).into_iter().map(|r| (r, None)));
            sents.push(((a, e), Some(*s)));
            pos = e;
        }
        sents.extend(sentences(text, pos, off.len16()).into_iter().map(|r| (r, None)));
        // each rule's matches in the paragraph, once
        let words: Vec<Vec<(u32, u32)>> = self.rules.iter().map(|r| match &r.m {
            Matcher::Words(t) => crate::bold::source_ranges(text, t),
            Matcher::Pattern(_) => Vec::new(),
        }).collect();
        let hits = |i: usize, (a, e): (u32, u32)| -> bool {
            match &self.rules[i].m {
                Matcher::Words(_) => words[i].iter().any(|&(s, t)| s >= a && t <= e),
                Matcher::Pattern(re) => re.is_match(off.slice(text, a, e)),
            }
        };
        let clause = |i: usize| self.level(self.rules[i].colour) == Level::Clause;
        let mut out: Vec<Span> = spans.iter().filter(|s| !is_sentence(s)).map(|s| (*s).clone()).collect();
        for ((a, e), model) in sents {
            let matched: Vec<usize> = (0..self.rules.len()).filter(|&i| clause(i) && hits(i, (a, e))).collect();
            let forbidden = |c: usize| matched.iter().any(|&i| self.rules[i].forbid && self.rules[i].colour == c);
            let forced = matched.iter().filter(|&&i| !self.rules[i].forbid).map(|&i| self.rules[i].colour).filter(|&c| !forbidden(c)).min();
            if let Some(c) = forced {
                let v = model.map_or_else(|| visible(off.slice(text, a, e)), |m| m.v.clone());
                out.push(Span { k: COLOUR + c as u8, w: 1.0, at: vec![(a, e)], v });
            } else if let Some(m) = model.filter(|m| !forbidden((m.k - COLOUR) as usize)) {
                out.push(m.clone());
            }
        }
        // phrase colours: Force rules mark their matches, Forbid rules drop the server's phrases they overlap
        for (i, r) in self.rules.iter().enumerate().filter(|&(i, _)| !clause(i)) {
            let found: Vec<(u32, u32)> = match &r.m {
                Matcher::Words(_) => words[i].clone(),
                Matcher::Pattern(re) => re.find_iter(text).filter(|m| !m.is_empty()).map(|m| (off.to16(m.start()), off.to16(m.end()))).collect(),
            };
            if r.forbid {
                let mine = &self.kinds[r.colour];
                out.retain(|s| !(mine.contains(&s.k) && s.at.iter().any(|&(a, e)| found.iter().any(|&(x, y)| x < e && a < y))));
            } else {
                out.extend(found.into_iter().map(|(a, e)| Span { k: COLOUR + r.colour as u8, w: 1.0, at: vec![(a, e)], v: visible(off.slice(text, a, e)) }));
            }
        }
        out
    }
}

/// What the rules of a code do to one sentence, in words, for the settings' test: each rule that matches, and the
/// result.
pub fn explain(code: &ColourCode, sentence: &str) -> String {
    let sentence = sentence.trim();
    if sentence.is_empty() {
        return "Type or paste a sentence to see which rules match it.".into();
    }
    let mut said = Vec::new();
    let mut forced: Option<usize> = None;
    let mut forbidden: Vec<usize> = Vec::new();
    let mut phrases = Vec::new();
    for (i, c) in code.colours.iter().enumerate().take(MAX_COLOURS) {
        for (n, r) in c.rules.iter().enumerate().filter(|(_, r)| r.on) {
            let Ok(m) = compile(r) else { continue };
            let what = if r.action == Action::Forbid { "forbids" } else { "forces" };
            if c.level == Level::Phrase {
                let found: Vec<String> = match &m {
                    Matcher::Words(t) => {
                        let u: Vec<u16> = sentence.encode_utf16().collect();
                        crate::bold::source_ranges(sentence, t).into_iter().map(|(a, e)| String::from_utf16_lossy(&u[a as usize..e as usize])).collect()
                    }
                    Matcher::Pattern(re) => re.find_iter(sentence).filter(|m| !m.is_empty()).map(|m| m.as_str().to_string()).collect(),
                };
                if !found.is_empty() {
                    phrases.push(format!("{} rule {} {} \u{201c}{}\u{201d}", c.name, n + 1, if r.action == Action::Forbid { "drops the phrases on" } else { "marks" }, found.join("\u{201d}, \u{201c}")));
                }
                continue;
            }
            let hit = match &m {
                Matcher::Words(t) => !crate::bold::source_ranges(sentence, t).is_empty(),
                Matcher::Pattern(re) => re.is_match(sentence),
            };
            if hit {
                said.push(format!("{} rule {} matches ({what} the colour)", c.name, n + 1));
                if r.action == Action::Forbid {
                    forbidden.push(i);
                }
            }
        }
    }
    for (i, c) in code.colours.iter().enumerate().take(MAX_COLOURS).filter(|(_, c)| c.level == Level::Clause) {
        let forces = c.rules.iter().filter(|r| r.on && r.action == Action::Force).filter_map(|r| compile(r).ok()).any(|m| match m {
            Matcher::Words(t) => !crate::bold::source_ranges(sentence, &t).is_empty(),
            Matcher::Pattern(re) => re.is_match(sentence),
        });
        if forces && !forbidden.contains(&i) {
            forced = Some(i);
            break;
        }
    }
    let result = match forced {
        Some(i) => format!("The sentence is coloured {}, whatever the model says.", code.colours[i].name),
        None if !forbidden.is_empty() => {
            let names: Vec<&str> = forbidden.iter().map(|&i| code.colours[i].name.as_str()).collect();
            format!("The model decides, but never {}.", names.join(" or "))
        }
        None => "No rule decides its colour: the model does.".into(),
    };
    let mut parts = said;
    parts.extend(phrases);
    if parts.is_empty() {
        result
    } else {
        format!("{}. {result}", parts.join("; "))
    }
}

/// UTF-8 and UTF-16 offsets of a text's characters, to move between the two.
struct Offsets {
    bytes: Vec<usize>,
    u16s: Vec<u32>,
}

impl Offsets {
    fn new(text: &str) -> Offsets {
        let (mut bytes, mut u16s) = (Vec::with_capacity(text.len() + 1), Vec::with_capacity(text.len() + 1));
        let mut pos = 0u32;
        for (b, c) in text.char_indices() {
            bytes.push(b);
            u16s.push(pos);
            pos += c.len_utf16() as u32;
        }
        bytes.push(text.len());
        u16s.push(pos);
        Offsets { bytes, u16s }
    }

    fn len16(&self) -> u32 {
        *self.u16s.last().unwrap_or(&0)
    }

    fn to16(&self, byte: usize) -> u32 {
        let i = self.bytes.partition_point(|&b| b < byte);
        self.u16s.get(i).copied().unwrap_or_else(|| self.len16())
    }

    fn to_byte(&self, u16: u32) -> usize {
        let i = self.u16s.partition_point(|&p| p < u16);
        self.bytes.get(i).copied().unwrap_or(*self.bytes.last().unwrap_or(&0))
    }

    fn slice<'a>(&self, text: &'a str, a: u32, e: u32) -> &'a str {
        let (a, e) = (self.to_byte(a), self.to_byte(e));
        text.get(a..e.max(a)).unwrap_or("")
    }
}

/// The sentences of `text` between UTF-16 offsets `from` and `to`, trimmed, each with a letter: the worker's own
/// splitting, for the text the model has not coloured (yet). A sentence ends at `.`, `?` or `!` (and any closing quote
/// or bracket) followed by white space and a capital, a digit, a link, a citation or an opening quote, unless the word
/// before the full stop is an abbreviation ("et al.", "e.g.", "p.") or a single letter (an initial).
pub fn sentences(text: &str, from: u32, to: u32) -> Vec<(u32, u32)> {
    let chars: Vec<(u32, char)> = text.chars().scan(0u32, |pos, c| {
        let at = *pos;
        *pos += c.len_utf16() as u32;
        Some((at, c))
    }).filter(|&(at, _)| at >= from && at < to).collect();
    let mut out = Vec::new();
    let push = |out: &mut Vec<(u32, u32)>, piece: &[(u32, char)]| {
        let a = piece.iter().position(|(_, c)| !c.is_whitespace());
        let e = piece.iter().rposition(|(_, c)| !c.is_whitespace());
        if let (Some(a), Some(e)) = (a, e) {
            if piece[a..=e].iter().any(|(_, c)| c.is_alphabetic()) {
                out.push((piece[a].0, piece[e].0 + piece[e].1.len_utf16() as u32));
            }
        }
    };
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i].1;
        if matches!(c, '.' | '?' | '!') {
            let mut j = i + 1;
            while j < chars.len() && "\"'\u{201d}\u{2019})]*_".contains(chars[j].1) {
                j += 1;
            }
            let spaced = j < chars.len() && chars[j].1.is_whitespace();
            let mut k = j;
            while k < chars.len() && chars[k].1.is_whitespace() {
                k += 1;
            }
            let next = chars.get(k).map(|x| x.1);
            let opens = next.is_some_and(|n| n.is_uppercase() || n.is_ascii_digit() || "[@(\"'\u{201c}\u{2018}*_".contains(n));
            let word: String = chars[start..i].iter().rev().map(|x| x.1).take_while(|c| c.is_alphanumeric() || *c == '.').collect::<Vec<_>>().into_iter().rev().collect::<String>().to_lowercase();
            let abbreviation = c == '.' && (ABBREVIATIONS.contains(&word.as_str()) || (word.chars().count() == 1 && word.chars().all(char::is_alphabetic)));
            if spaced && opens && !abbreviation {
                push(&mut out, &chars[start..j]);
                start = k;
                i = k;
                continue;
            }
        }
        i += 1;
    }
    push(&mut out, &chars[start..]);
    out
}

/// A source range's text roughly as Reading view shows it (links as their shown words, emphasis markers and inline
/// code ticks dropped, white space collapsed), for finding it in a rendered block (reading.rs, which also tries the
/// first and last words).
fn visible(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("[[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("]]") {
            Some(j) => {
                let inner = &after[..j];
                out.push_str(inner.rsplit('|').next().unwrap_or(inner));
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    let out: String = out.chars().filter(|c| !"*=~`".contains(*c)).collect();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The citations' [start, end) in UTF-16 offsets of the text.
pub fn citations(text: &str) -> Vec<(u32, u32)> {
    let chars: Vec<(u32, char)> = text.chars().scan(0u32, |pos, c| {
        let at = *pos;
        *pos += c.len_utf16() as u32;
        Some((at, c))
    }).collect();
    let end16 = |i: usize| chars.get(i).map_or(text.encode_utf16().count() as u32, |c| c.0);
    let key = |c: char| c.is_alphanumeric() || "_:.-".contains(c);
    let mut out = Vec::new();
    let (mut i, mut code) = (0, false);
    while i < chars.len() {
        let c = chars[i].1;
        if c == '`' {
            code = !code;
        } else if !code && c == '[' && chars.get(i + 1).is_some_and(|n| n.1 == '@') {
            // a bracketed citation, to its closing bracket on the same line
            if let Some(j) = (i + 1..chars.len()).take_while(|&j| chars[j].1 != '\n' && chars[j].1 != '[').find(|&j| chars[j].1 == ']') {
                out.push((chars[i].0, end16(j + 1)));
                i = j + 1;
                continue;
            }
        } else if !code && c == '@' && chars.get(i + 1).is_some_and(|n| n.1.is_alphabetic())
            && !(i > 0 && (chars[i - 1].1.is_alphanumeric() || chars[i - 1].1 == '.')) // an e-mail address
        {
            let mut j = i + 1;
            while j < chars.len() && key(chars[j].1) {
                j += 1;
            }
            while j > i + 1 && ".:-".contains(chars[j - 1].1) {
                j -= 1; // the sentence's full stop, not the key's
            }
            out.push((chars[i].0, end16(j)));
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracketed_and_narrative_citations_not_emails_nor_code() {
        let text = "😀 As @hinton2015 argue [@a2020, p. 3; @b-c:2021]. Mail me@x.org, see `@not` and @Cos21.";
        let got: Vec<String> = citations(text).iter().map(|&(a, e)| String::from_utf16(&text.encode_utf16().collect::<Vec<_>>()[a as usize..e as usize]).unwrap()).collect();
        assert_eq!(got, ["@hinton2015", "[@a2020, p. 3; @b-c:2021]", "@Cos21"]);
    }

    fn rule(kind: RuleKind, action: Action, text: &str) -> Rule {
        Rule { kind, action, text: text.into(), on: true }
    }

    /// The Reading code (colours::defaults) with rules added: (colour index, rule).
    fn code(rules: &[(usize, Rule)]) -> ColourCode {
        let mut c = crate::colours::defaults()[0].clone();
        for (i, r) in rules {
            c.colours[*i].rules.push(r.clone());
        }
        c
    }

    fn text_of(text: &str, (a, e): (u32, u32)) -> String {
        String::from_utf16_lossy(&text.encode_utf16().collect::<Vec<_>>()[a as usize..e as usize])
    }

    #[test]
    fn patterns_are_checked_before_use() {
        let p = |t: &str| compile(&rule(RuleKind::Pattern, Action::Force, t)).err();
        assert!(p("^(First|Second|Third), no\\b").is_none());
        assert!(p("(unclosed").unwrap().starts_with("Not a valid pattern"));
        assert!(p("a*").unwrap().contains("empty text"), "would match every sentence");
        assert!(p(&"a".repeat(MAX_PATTERN + 1)).unwrap().contains("the most is"));
        assert!(p("((((((((((((((((((a))))))))))))))))))").unwrap().starts_with("Not a valid pattern"), "nested too deeply");
        assert!(p("\\w{1000}{1000}").is_some(), "too large once compiled");
        assert!(p("   ").unwrap().starts_with("No pattern"));
        assert!(compile(&rule(RuleKind::Words, Action::Force, "")).is_err());
        // a pattern that would hang a backtracking engine runs in linear time here
        let Ok(Matcher::Pattern(re)) = compile(&rule(RuleKind::Pattern, Action::Force, "(a+)+$")) else { panic!() };
        assert!(!re.is_match(&format!("{}!", "a".repeat(50_000))));
        // switched-off and invalid rules are left out; a code with none has nothing to apply
        let mut off = rule(RuleKind::Words, Action::Force, "gap");
        off.on = false;
        assert!(CodeRules::new(&code(&[(0, off), (5, rule(RuleKind::Pattern, Action::Force, "("))])).is_none());
    }

    #[test]
    fn the_worker_splits_sentences_but_not_after_abbreviations() {
        let t = "As Shen et al. note, e.g. here, results vary (p. 3). J. Smith agrees! \"Quoted.\" [[Link]] starts one. 3 more.";
        let got: Vec<String> = sentences(t, 0, t.encode_utf16().count() as u32).into_iter().map(|r| text_of(t, r)).collect();
        assert_eq!(got, ["As Shen et al. note, e.g. here, results vary (p. 3).", "J. Smith agrees!", "\"Quoted.\"", "[[Link]] starts one.", "3 more."]);
    }

    /// A sentence's colour from the model: (colour, probability, text).
    fn model(text: &str, c: u8, w: f32, sentence: &str) -> Span {
        let a = text.find(sentence).unwrap();
        let a16 = text[..a].encode_utf16().count() as u32;
        Span { k: COLOUR + c, w, at: vec![(a16, a16 + sentence.encode_utf16().count() as u32)], v: sentence.into() }
    }

    fn colours_of(text: &str, spans: &[Span]) -> Vec<(u8, String)> {
        let mut v: Vec<(u8, String)> = spans.iter().filter(|s| s.k >= COLOUR).map(|s| (s.k - COLOUR, text_of(text, s.at[0]))).collect();
        v.sort_by_key(|x| x.1.clone());
        v
    }

    #[test]
    fn force_wins_over_the_model_forbid_over_force_and_the_first_colour_wins() {
        let text = "Detectors fail on new data. First, no study measures distance. Another consideration is spacing. It is 5% worse.";
        let (s1, s2, s3, s4) = ("Detectors fail on new data.", "First, no study measures distance.", "Another consideration is spacing.", "It is 5% worse.");
        // the model: s1 Claims, s2 Findings (wrong), s3 Evidence (wrong), s4 Findings
        let spans = [model(text, 0, 0.9, s1), model(text, 3, 0.97, s2), model(text, 4, 1.0, s3), model(text, 3, 1.0, s4)];
        let refs: Vec<&Span> = spans.iter().collect();
        let rules = code(&[
            (5, rule(RuleKind::Pattern, Action::Force, "^(First|Second|Third), no\\b")), // Limitations
            (1, rule(RuleKind::Pattern, Action::Force, "^(Another|A further) \\w+ (is|concerns)")), // Signposts
            (6, rule(RuleKind::Words, Action::Force, "consideration")), // Connections, later in the code: Signposts wins
            (0, rule(RuleKind::Words, Action::Forbid, "detectors")), // never Claims
            (3, rule(RuleKind::Pattern, Action::Force, "\\d%")), // Findings
            (3, rule(RuleKind::Words, Action::Forbid, "worse")), // ... but never for a sentence with "worse"
        ]);
        let out = CodeRules::new(&rules).unwrap().apply(text, &refs);
        assert_eq!(colours_of(text, &out), [(1, s3.into()), (5, s2.into())], "s1's Claims forbidden, s4's Findings forbidden despite its Force rule");
        assert!(out.iter().filter(|s| s.k >= COLOUR).all(|s| s.w == 1.0), "a rule's colour shows at any sensitivity above zero");
        // without the forbid rules, the model's colours stay where no rule applies
        let mut lighter = rules.clone();
        lighter.colours[0].rules.clear();
        lighter.colours[3].rules.retain(|r| r.action == Action::Force);
        let out = CodeRules::new(&lighter).unwrap().apply(text, &refs);
        assert_eq!(colours_of(text, &out), [(1, s3.into()), (0, s1.into()), (5, s2.into()), (3, s4.into())], "the model's colours stay where no rule applies");
    }

    #[test]
    fn rules_apply_before_the_model_answers_and_to_sentences_it_left_uncoloured() {
        let text = "😀 Background first. Second, no study has tried. Third sentence here.";
        let rules = CodeRules::new(&code(&[(5, rule(RuleKind::Pattern, Action::Force, "^(First|Second|Third), no\\b"))])).unwrap();
        let out = rules.apply(text, &[]);
        assert_eq!(colours_of(text, &out), [(5, "Second, no study has tried.".to_string())]);
        assert_eq!(out[0].v, "Second, no study has tried.", "its visible text, for Reading view");
        // the model coloured the first sentence only: the rule still finds the second in the rest
        let m = model(text, 0, 0.8, "😀 Background first.");
        let out = rules.apply(text, &[&m]);
        assert_eq!(colours_of(text, &out).len(), 2);
    }

    #[test]
    fn phrase_rules_mark_words_and_drop_the_server_s_phrases() {
        let text = "We use the [[glossary#ROUGE|ROUGE]] and BLEU scores, and `ROUGE` in code; a bootstrap interval.";
        let term = |w: &str| {
            let a = text.find(w).unwrap() as u32;
            Span { k: pick::TERM, w: 0.9, at: vec![(a, a + w.len() as u32)], v: w.into() }
        };
        let spans = [term("BLEU"), term("bootstrap interval")];
        let refs: Vec<&Span> = spans.iter().collect();
        // Key terms (index 2, a phrase colour showing the server's terms): mark the scores, never "bootstrap interval"
        let rules = code(&[(2, rule(RuleKind::Words, Action::Force, "ROUGE, BLEU")), (2, rule(RuleKind::Pattern, Action::Forbid, "bootstrap"))]);
        let out = CodeRules::new(&rules).unwrap().apply(text, &refs);
        let marked: Vec<(u8, String)> = out.iter().map(|s| (s.k, text_of(text, s.at[0]))).collect();
        assert_eq!(marked, [(pick::TERM, "BLEU".into()), (COLOUR + 2, "BLEU".into())], "words outside links and code; the forbidden phrase dropped");
        assert!(crate::colours::stylesheet(&rules).contains(".av-c2 { background-image:"), "a phrase colour with rules gets its own class");
    }

    #[test]
    fn the_settings_test_explains_what_the_rules_do() {
        let rules = code(&[
            (5, rule(RuleKind::Pattern, Action::Force, "^(First|Second|Third), no\\b")),
            (3, rule(RuleKind::Words, Action::Forbid, "never")),
            (7, rule(RuleKind::Pattern, Action::Force, "@\\w+")),
        ]);
        let said = explain(&rules, "First, no study has never measured it, says @smith2020.");
        assert!(said.contains("Limitations rule 1 matches (forces the colour)"), "{said}");
        assert!(said.contains("Findings rule 1 matches (forbids the colour)"), "{said}");
        assert!(said.contains("Citations rule 1 marks \u{201c}@smith2020\u{201d}"), "{said}");
        assert!(said.ends_with("The sentence is coloured Limitations, whatever the model says."), "{said}");
        assert_eq!(explain(&rules, "Nothing here."), "No rule decides its colour: the model does.");
        assert!(explain(&rules, "It never does.").ends_with("The model decides, but never Findings."));
    }
}
