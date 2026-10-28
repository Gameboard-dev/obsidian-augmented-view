//! A paragraph as the rules see it: markup blanked (same length, so offsets carry over), its links, and its words as
//! UDPipe parses them, aligned back to the text (UDPipe's default tokenizer reports no character offsets, but its words
//! follow the text in order).
//!
//! References: UDPipe and the Universal Dependencies labels it outputs (<https://ufal.mff.cuni.cz/udpipe/1>,
//! <https://universaldependencies.org/u/pos/>, <https://universaldependencies.org/u/dep/>), udpipe-rs
//! (<https://docs.rs/udpipe-rs>), fancy-regex for the patterns that need lookaround (<https://docs.rs/fancy-regex>).

use std::collections::HashSet;
use std::sync::LazyLock;

use fancy_regex::Regex;

/// Markup blanked before parsing; the groups listed are kept (a link's visible text).
static MASKS: LazyLock<Vec<(Regex, &'static [usize])>> = LazyLock::new(|| {
    [
        (r"(?s)%%.*?%%", &[][..]),                                   // Obsidian comments
        (r"`[^`]*`", &[]),                                           // inline code
        (r"(?s)\$\$.*?\$\$|\$[^$\n]+\$", &[]),                       // maths
        (r"\\\[\[\[.*?\]\]\\\]", &[]),                               // numbered citation runs \[[[#^ref-5|5]]\]
        (r"\[@[^\]]*\]|(?<![\w.])@[\w:.-]+", &[]),                   // pandoc citations
        (r"!?\[\[[^\]|]*\|([^\]]*)\]\]", &[1]),                      // [[target|alias]]
        (r"!?\[\[([^\]]*)\]\]", &[1]),                               // [[target]]
        (r"!?\[([^\]]*)\]\([^)]*\)", &[1]),                          // [text](url)
        (r"https?://\S+", &[]),
        (r"<[^>\n]+>", &[]),                                         // HTML tags
        (r"\^\[.*?\]|\[\^[^\]]*\]", &[]),                            // footnotes
        (r"(?<!\S)\^[\w-]+\s*$", &[]),                               // block id
        (r"\*\*|__|==|~~|(?<!\w)[*_]|[*_](?!\w)|\\(?=\S)", &[]),     // emphasis markers, escapes
        (r"(?m)^\s*(?:(?:[-*+]|\d+[.)]|>+|#+)\s+)+(?:\[[ xX]\]\s+)?", &[]), // list, quote, heading and task markers
        (r"\[![\w-]+\][+-]?", &[]),                                  // callout type
        (r"\(\(<[^)\n]*\)\)", &[]),                                  // a caption's tag, ((<tag|Figure)) (Tags for Markdown)
        // Locators in brackets: "(Clause 3.4, p. 2)", "(Introduction, p. viii)", "(Note 2 to Clause 3.5)", "(Annex A, p. 34)"
        (r"\([^()\n]*\bpp?\.\s*[\dxivlc]+[^()\n]*\)|\((?:Clause|Annex|Table|Note|Figure|Fig\.|Section|Eq\.|Equation|Chapter)\s[^()\n]*\)", &[]),
    ]
    .into_iter()
    .map(|(p, keep)| (Regex::new(p).expect("mask pattern"), keep))
    .collect()
});

static STOP: LazyLock<HashSet<&'static str>> = LazyLock::new(|| stop_words::get("en").iter().copied().collect());

pub struct Tok {
    pub s: usize, // byte offsets in the text (equal in the masked text)
    pub e: usize,
    pub form: String,
    pub lower: String,
    pub lemma: String,
    pub upos: String,
    pub dep: String,
    pub head: Option<usize>,
    pub sent: i32,
}

impl Tok {
    pub fn stop(&self) -> bool {
        STOP.contains(self.lower.as_str())
    }
    pub fn punct(&self) -> bool {
        self.upos == "PUNCT"
    }
    pub fn alpha(&self) -> bool {
        !self.form.is_empty() && self.form.chars().all(char::is_alphabetic)
    }
    pub fn num(&self) -> bool {
        self.upos == "NUM" || self.form.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    }
}

pub struct Doc {
    pub text: String,
    pub masked: String,
    pub links: Vec<(usize, usize)>,
    pub toks: Vec<Tok>,
    pub children: Vec<Vec<usize>>,
    pub left: Vec<usize>,  // leftmost token of each token's subtree
    pub right: Vec<usize>, // rightmost
}

/// The text with markup blanked byte for byte, and the spans of its links (styled already: never a term or phrase).
pub fn mask(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut bytes = text.as_bytes().to_vec();
    let mut links = Vec::new();
    for (re, keep) in MASKS.iter() {
        let current = String::from_utf8_lossy(&bytes).into_owned();
        for caps in re.captures_iter(&current).flatten() {
            let m = caps.get(0).unwrap();
            if !keep.is_empty() {
                links.push((m.start(), m.end()));
            }
            let kept: Vec<(usize, usize)> = keep.iter().filter_map(|&g| caps.get(g)).map(|g| (g.start(), g.end())).collect();
            for i in m.start()..m.end() {
                if !kept.iter().any(|&(a, b)| a <= i && i < b) {
                    bytes[i] = b' ';
                }
            }
        }
    }
    (String::from_utf8_lossy(&bytes).into_owned(), links)
}

impl Doc {
    pub fn parse(text: &str, parser: &udpipe_rs::Model) -> Doc {
        let (masked, links) = mask(text);
        let words = parser.parse(&masked).unwrap_or_default();
        let (mut toks, mut pos, mut base, mut sent) = (Vec::with_capacity(words.len()), 0usize, 0usize, -1);
        for w in words {
            if w.sentence_id != sent {
                (sent, base) = (w.sentence_id, toks.len());
            }
            // a word not found leaves no gap; the search starts after white space, so a long blanked citation or
            // locator ("(Section [[#A long heading|A long heading]])") never puts the next word out of reach
            let from = masked.len() - masked[pos..].trim_start().len();
            let found = masked[from..].find(&w.form).filter(|&i| i < 64).map(|i| from + i);
            let (s, e) = found.map_or((pos, pos), |s| (s, s + w.form.len()));
            pos = e;
            let head = (w.head > 0).then(|| base + w.head as usize - 1);
            toks.push(Tok { s, e, lower: w.form.to_lowercase(), lemma: w.lemma.to_lowercase(), form: w.form, upos: w.upostag, dep: w.deprel, head, sent });
        }
        let n = toks.len();
        let mut children = vec![Vec::new(); n];
        let (mut left, mut right): (Vec<usize>, Vec<usize>) = ((0..n).collect(), (0..n).collect());
        for i in 0..n {
            if let Some(h) = toks[i].head.filter(|&h| h < n) {
                children[h].push(i);
            }
            let mut a = toks[i].head;
            let mut guard = 0;
            while let Some(h) = a.filter(|&h| h < n && guard < n) {
                left[h] = left[h].min(i);
                right[h] = right[h].max(i);
                a = toks[h].head;
                guard += 1;
            }
        }
        Doc { text: text.to_string(), masked, links, toks, children, left, right }
    }

    /// False when [s, e) runs across a citation, comment or link (emphasis markers inside are fine).
    pub fn clean(&self, s: usize, e: usize) -> bool {
        let (m, t) = (&self.masked.as_bytes()[s..e], &self.text.as_bytes()[s..e]);
        m.iter().zip(t).all(|(a, b)| a == b || b.is_ascii_whitespace() || b"*_=~\\".contains(b)) && !self.links.iter().any(|&(a, b)| s < b && a < e)
    }

    pub fn in_link(&self, t: usize) -> bool {
        self.links.iter().any(|&(a, b)| a <= self.toks[t].s && self.toks[t].s < b)
    }

    /// For each token, whether it is inside round or square brackets ("(Literature Search and Screening)").
    pub fn bracketed(&self) -> Vec<bool> {
        let mut depth = 0i32;
        self.toks.iter().map(|t| match t.form.as_str() {
            "(" | "[" => {
                depth += 1;
                false
            }
            ")" | "]" => {
                depth = (depth - 1).max(0);
                false
            }
            _ => depth > 0,
        }).collect()
    }

    /// Whether white space follows token `t` in the text.
    pub fn space_after(&self, t: usize) -> bool {
        self.masked[self.toks[t].e..].starts_with(char::is_whitespace)
    }

    /// The sentences, as token index ranges.
    pub fn sentences(&self) -> Vec<std::ops::Range<usize>> {
        let mut out = Vec::new();
        let mut start = 0;
        for i in 1..=self.toks.len() {
            if i == self.toks.len() || self.toks[i].sent != self.toks[start].sent {
                out.push(start..i);
                start = i;
            }
        }
        out.retain(|r| !r.is_empty());
        out
    }

    /// The sentences as byte ranges of the text, each running to the next one's start so that a citation, a link or the
    /// full stop after its last word is part of it (and one that starts with a link starts at the link's brackets);
    /// white space at either end left out. A sentence that opens with a narrative citation ("… [@b2021]. @c2022 report")
    /// starts there too: the parser sees only "… . report" and either runs the two together or starts the sentence at
    /// "report".
    pub fn sentence_spans(&self) -> Vec<(usize, usize)> {
        static CITED_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?<=[.!?])\s+(@[A-Za-z])").expect("pattern"));
        let cited: Vec<usize> = CITED_START.captures_iter(&self.text).flatten().filter_map(|c| c.get(1)).map(|m| m.start()).collect();
        let mut starts: Vec<usize> = self.sentences().iter().map(|r| {
            let s = self.toks[r.start].s;
            let s = self.links.iter().find(|&&(a, b)| a < s && s < b).map_or(s, |&(a, _)| a);
            // back over a narrative citation (blanked in the parse) that opens the sentence
            cited.iter().copied().find(|&c| c < s && self.masked[c..s].trim().is_empty()).unwrap_or(s)
        }).collect();
        starts.extend(cited);
        starts.sort_unstable();
        starts.dedup();
        let mut out = Vec::new();
        for (i, &s) in starts.iter().enumerate() {
            let s = if i == 0 { 0 } else { s };
            let e = starts.get(i + 1).copied().unwrap_or(self.text.len());
            let seg = &self.text[s..e];
            let (a, b) = (s + (seg.len() - seg.trim_start().len()), s + seg.trim_end().len());
            if a < b {
                out.push((a, b));
            }
        }
        out
    }

    /// A span's text for a language model to read: links as their visible text, evidence links ([[EV-0123]]) and
    /// comments left out, a bracketed citation as "(citation)" and a narrative one (@pereira2013) as its key.
    pub fn readable(&self, s: usize, e: usize) -> String {
        static READABLE: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
            [
                (r"(?s)%%.*?%%", ""),
                (r",?\s*\[\[EV-\d+\]\]", ""),
                (r"!?\[\[[^\]|]*\|([^\]]*)\]\]", "$1"),
                (r"!?\[\[([^\]]*)\]\]", "$1"),
                (r"!?\[([^\]]*)\]\([^)]*\)", "$1"),
                (r"\[@[^\]]*\]", "(citation)"),
                (r"(?<![\w.])@([A-Za-z][\w:-]*)", "$1"),
                (r"\\%", "%"),
                (r"\*\*|__|==", ""),
                (r"\s+", " "),
            ]
            .into_iter()
            .map(|(p, r)| (Regex::new(p).expect("readable pattern"), r))
            .collect()
        });
        let mut t = self.text[s..e].to_string();
        for (re, rep) in READABLE.iter() {
            t = re.replace_all(&t, *rep).into_owned();
        }
        t.trim().to_string()
    }

    /// A span's text as a reader sees it (markup blanked, spaces collapsed): what Reading view looks for.
    pub fn visible(&self, s: usize, e: usize) -> String {
        self.masked[s..e].split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A byte offset as UTF-16 code units, how JavaScript indexes strings.
    pub fn utf16(&self, byte: usize) -> u32 {
        self.text[..byte].encode_utf16().count() as u32
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// UDPipe's model, where the server keeps it; tests that need it are skipped without it.
    pub fn parser() -> Option<udpipe_rs::Model> {
        udpipe_rs::Model::load(crate::models::default_cache().join(crate::PARSER)).ok()
    }

    const TEXT: &str = "Two questions sit under every **research philosophy** [@oates2006, p. 287]. Oates calls it inaccurate to equate \
        positivism with quantitative research, because either kind of data can serve any paradigm [[EV-0432]]; \
        Biggam agrees (and he gives an example, which is not discussed here). See [[glossary#ROUGE|ROUGE]] too.";

    #[test]
    fn mask_keeps_offsets_and_records_links() {
        let (masked, links) = mask(TEXT);
        assert_eq!(masked.len(), TEXT.len());
        assert!(!masked.contains("**") && !masked.contains("@oates") && !masked.contains("[["), "{masked}");
        assert!(masked.contains("ROUGE") && masked.contains("EV-0432"));
        let mut found: Vec<&str> = links.iter().map(|&(a, b)| &TEXT[a..b]).collect();
        found.sort();
        assert_eq!(found, ["[[EV-0432]]", "[[glossary#ROUGE|ROUGE]]"]);
        assert!(!mask("((<tag|Figure)) A caption").0.contains("tag"));
    }

    #[test]
    fn clauses_cut_at_because_never_in_brackets_nor_at_links() {
        let Some(p) = parser() else { return };
        let d = Doc::parse(TEXT, &p);
        let got: Vec<&str> = crate::rules::clauses(&d).iter().map(|&(s, e)| &TEXT[s..e]).collect();
        assert!(got.contains(&"Oates calls it inaccurate to equate positivism with quantitative research"), "{got:?}");
        assert!(got.contains(&"because either kind of data can serve any paradigm"), "{got:?}");
        assert!(got.iter().all(|c| !c.contains("[[EV") && !c.ends_with("[@")), "{got:?}");
        assert!(got.iter().any(|c| c.contains("(and he gives an example, which is not discussed here)")), "{got:?}");
    }

    #[test]
    fn words_after_a_long_blanked_locator_are_aligned_and_sentences_cover_their_markup() {
        let Some(p) = parser() else { return };
        let text = "The gain falls on accepted requests (Section [[#How Detectors Fail outside Their Training Data|How Detectors \
            Fail outside Their Training Data]]) [[EV-0329]], but its size is not established. [[glossary#MAE|MAE]] \
            hides it [@a2020; @b2021]. @c2022 report a student.";
        let d = Doc::parse(text, &p);
        let last = d.toks.last().unwrap();
        assert_eq!(&text[last.s..last.e], ".", "every word aligned to the text");
        assert!(d.toks.iter().any(|t| &text[t.s..t.e] == "established"));
        let got: Vec<&str> = d.sentence_spans().iter().map(|&(s, e)| &text[s..e]).collect();
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(got[1].starts_with("[[glossary#MAE") && got[1].ends_with("@b2021]."), "{got:?}");
        assert_eq!(d.readable(0, text.len()).split(". ").nth(1), Some("MAE hides it (citation)"));
        assert!(d.readable(0, text.len()).ends_with("c2022 report a student."));
        assert_eq!(got[2], "@c2022 report a student.");
        let text = "Two limits remain. The first is that only one detector was tested [@x2020], so the result may not carry over. \
            @y2021 found an MAE of 12% on unseen servers.";
        let d = Doc::parse(text, &p);
        let got: Vec<&str> = d.sentence_spans().iter().map(|&(s, e)| &text[s..e]).collect();
        assert_eq!(got.last(), Some(&"@y2021 found an MAE of 12% on unseen servers."), "{got:?}");
    }

    #[test]
    fn terms_whole_hyphenated_and_without_punctuation() {
        let Some(p) = parser() else { return };
        let text = "PAD uses either sensor based methods or feature based methods. Sensor-based methods mainly involve \
            additional hardware, as the standard rates \u{201c}not conformant\u{201d} systems.";
        let terms = crate::rules::terms(&Doc::parse(text, &p));
        assert!(terms.contains_key("sensor-based methods"), "{:?}", terms.keys());
        assert!(!terms.contains_key("based methods") || terms["based methods"].iter().all(|&(s, _)| text.as_bytes()[s - 1] == b' '));
        assert!(terms.keys().all(|k| !k.contains('\u{201c}') && !k.contains(',')), "{:?}", terms.keys());
    }
}
