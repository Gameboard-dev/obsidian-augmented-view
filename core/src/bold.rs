//! The user's own key terms (the setting *Always bold*, added and removed from the editor's menu with "Make emphasised
//! bold"): found in the worker in each paragraph's text, so they apply at once, to cached paragraphs too, and never ask
//! the server. Every whole-word occurrence is drawn, in any case, outside code, maths, comments, links, citations,
//! footnotes, tags and HTML tags: the server's masking (server/src/text.rs, `MASKS`) in one pass, without regular
//! expressions. A space in a term matches any run of white space, emphasis markers in it allowed ("**network**
//! intrusion").
//!
//! Reading view shows rendered text, in which a link is only its visible words: there the occurrences are counted in
//! the paragraph as rendered (markup that is not shown left out, a link's words kept) and in the block, and when the
//! counts agree, those that fall in links are left out by their place in order; when they do not, every one is drawn.
//!
//! The syntax: <https://help.obsidian.md/syntax>, <https://help.obsidian.md/Editing+and+formatting/Advanced+formatting+syntax>.

/// The most words a selection may have to be added.
pub const MAX_WORDS: usize = 6;
/// Characters stripped from a selection's ends (markup and punctuation picked up with the word).
const EDGE: &[char] = &['*', '_', '=', '~', '.', ',', ';', ':', '!', '?', '"', '\'', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '(', ')', '[', ']', '{', '}'];
/// Characters a term cannot hold: the list's separator, and markup a term outside links and code never contains.
const NEVER: &[char] = &[',', '`', '$', '[', ']', '|'];
/// Emphasis markers allowed between a term's words.
const MARKERS: &[char] = &['*', '_', '=', '~'];

/// A selection as a term to add: trimmed of white space and of the markup and punctuation at its ends, white space
/// collapsed. None when empty, over `MAX_WORDS` words, across lines, or holding a comma or markup.
pub fn term(selection: &str) -> Option<String> {
    if selection.contains(['\n', '\r']) {
        return None;
    }
    let t = selection.trim_matches(|c: char| c.is_whitespace() || EDGE.contains(&c));
    let words: Vec<&str> = t.split_whitespace().collect();
    (!words.is_empty() && words.len() <= MAX_WORDS && !t.contains(NEVER)).then(|| words.join(" "))
}

/// The setting's entries, as written: separated by commas or new lines, white space collapsed.
fn entries(list: &str) -> impl Iterator<Item = String> + '_ {
    list.split([',', '\n']).map(|e| e.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|e| !e.is_empty())
}

fn key(term: &str) -> String {
    term.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" ")
}

/// Whether the list holds the term (in any case).
pub fn listed(list: &str, term: &str) -> bool {
    entries(list).any(|e| key(&e) == key(term))
}

/// The list with the term added, or taken out if it is there (in any case), and whether it was added. The list is
/// written back one entry after another, separated by commas.
pub fn toggle(list: &str, term: &str) -> (String, bool) {
    let k = key(term);
    let mut out: Vec<String> = entries(list).collect();
    let before = out.len();
    out.retain(|e| key(e) != k);
    let added = out.len() == before;
    if added {
        out.push(term.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    (out.join(", "), added)
}

/// The terms to find: lower-case characters, one space between words, each once.
pub fn terms(list: &str) -> Vec<Vec<char>> {
    let mut out: Vec<Vec<char>> = Vec::new();
    for e in entries(list) {
        let t: Vec<char> = key(&e).chars().map(lower).collect();
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// The [start, end) UTF-16 offsets of the terms' occurrences in a paragraph's source text, outside markup.
pub fn source_ranges(para: &str, terms: &[Vec<char>]) -> Vec<(u32, u32)> {
    if terms.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = para.chars().collect();
    let cls = classes(&chars);
    let text: Vec<char> = chars.iter().map(|&c| lower(c)).collect();
    let at16 = utf16(&chars);
    let mut out = Vec::new();
    for t in terms {
        let found = occurrences(&text, t).into_iter().filter(|&(a, e)| cls[a..e].iter().all(|&k| k == Cls::Prose));
        out.extend(found.map(|(a, e)| (at16[a], at16[e])));
    }
    out
}

/// The [start, end) UTF-16 offsets of the terms' occurrences in a Reading-view block's text (`block`), which shows the
/// paragraph `para`: those outside markup, matched by their place in order (see the module's notes).
pub fn block_ranges(para: &str, block: &str, terms: &[Vec<char>]) -> Vec<(u32, u32)> {
    if terms.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = para.chars().collect();
    let cls = classes(&chars);
    // the paragraph as rendered: the characters shown, each with whether it is prose
    let (shown, prose): (Vec<char>, Vec<bool>) = chars.iter().zip(&cls).filter(|(_, &k)| k != Cls::Hidden).map(|(&c, &k)| (lower(c), k == Cls::Prose)).unzip();
    let bchars: Vec<char> = block.chars().collect();
    let btext: Vec<char> = bchars.iter().map(|&c| lower(c)).collect();
    let at16 = utf16(&bchars);
    let mut out = Vec::new();
    for t in terms {
        let in_para = occurrences(&shown, t);
        let in_block = occurrences(&btext, t);
        let keep: Vec<bool> = if in_para.len() == in_block.len() { in_para.iter().map(|&(a, e)| prose[a..e].iter().all(|&p| p)).collect() } else { vec![true; in_block.len()] };
        out.extend(in_block.into_iter().zip(keep).filter(|&(_, k)| k).map(|((a, e), _)| (at16[a], at16[e])));
    }
    out
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Each character's UTF-16 offset, and the text's length last.
fn utf16(chars: &[char]) -> Vec<u32> {
    let mut out = Vec::with_capacity(chars.len() + 1);
    let mut pos = 0u32;
    for c in chars {
        out.push(pos);
        pos += c.len_utf16() as u32;
    }
    out.push(pos);
    out
}

/// Whole-word occurrences of `term` in `text` (both lower-case), left to right, not overlapping: [start, end) in
/// characters. A word's edge is where a letter or digit meets anything else.
fn occurrences(text: &[char], term: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let Some(&first) = term.first() else { return out };
    let mut i = 0;
    while i < text.len() {
        if text[i] == first && (i == 0 || !text[i - 1].is_alphanumeric()) {
            if let Some(e) = match_at(text, i, term).filter(|&e| e == text.len() || !text[e].is_alphanumeric()) {
                out.push((i, e));
                i = e;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Where `term` ends if it starts at `i`: its spaces match a run of white space and emphasis markers.
fn match_at(text: &[char], mut i: usize, term: &[char]) -> Option<usize> {
    for &c in term {
        if c == ' ' {
            let mut space = false;
            while i < text.len() && (text[i].is_whitespace() || MARKERS.contains(&text[i])) {
                space |= text[i].is_whitespace();
                i += 1;
            }
            if !space {
                return None;
            }
        } else if text.get(i) == Some(&c) {
            i += 1;
        } else {
            return None;
        }
    }
    Some(i)
}

/// What a character of the source is: prose, shown in Reading view but not prose (a link's words, a citation, a tag,
/// a web address), or not shown as text (markup, code, maths, comments, footnote markers).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Cls {
    Prose,
    Shown,
    Hidden,
}

/// Each character's class, in one pass.
fn classes(c: &[char]) -> Vec<Cls> {
    use Cls::*;
    let n = c.len();
    let mut out = vec![Prose; n];
    // whether `s` (ASCII) starts at `i`; the next `s` from `from`, on the same line unless `lines`
    let at = |i: usize, s: &[u8]| c.len() >= i + s.len() && c[i..i + s.len()].iter().zip(s).all(|(&x, &y)| x == y as char);
    let find = |from: usize, s: &[u8], lines: bool| (from..n).take_while(|&j| lines || c[j] != '\n').find(|&j| at(j, s));
    let word = |i: usize| c.get(i).is_some_and(|ch| ch.is_alphanumeric() || *ch == '_');
    let mut i = 0;
    while i < n {
        let ch = c[i];
        let after_space = i == 0 || c[i - 1].is_whitespace();
        // (end, class) of a construct starting here; the brackets of links are handled in place
        let mut span: Option<(usize, Cls)> = None;
        if at(i, b"%%") {
            span = Some((find(i + 2, b"%%", true).map_or(n, |j| j + 2), Hidden)); // Obsidian comments
        } else if at(i, b"<!--") {
            span = Some((find(i + 4, b"-->", true).map_or(n, |j| j + 3), Hidden));
        } else if ch == '\\' && i + 1 < n {
            out[i] = Hidden; // an escape: the next character is literal
            i += 2;
            continue;
        } else if ch == '`' {
            let run = c[i..].iter().take_while(|&&x| x == '`').count();
            let close = (i + run..n).find(|&j| c[j] == '`' && c[j..].iter().take_while(|&&x| x == '`').count() == run && c[j - 1] != '`');
            match close {
                Some(j) => span = Some((j + run, Hidden)), // inline code
                None => {
                    i += run;
                    continue;
                }
            }
        } else if at(i, b"$$") {
            span = Some((find(i + 2, b"$$", true).map_or(n, |j| j + 2), Hidden));
        } else if ch == '$' && c.get(i + 1).is_some_and(|x| !x.is_whitespace()) {
            // inline maths: closed by a `$` after a non-space, not before a digit ("$5 and $10" is not maths)
            let close = (i + 1..n).take_while(|&j| c[j] != '\n').find(|&j| c[j] == '$' && !c[j - 1].is_whitespace() && !c.get(j + 1).is_some_and(char::is_ascii_digit));
            span = close.map(|j| (j + 1, Hidden));
        } else if at(i, b"((<") {
            span = find(i, b"))", false).map(|j| (j + 2, Hidden)); // a caption's tag, ((<tag|Figure)) (Tags for Markdown)
        } else if at(i, b"![[") || at(i, b"[[") {
            let s = if ch == '!' { i + 3 } else { i + 2 };
            if let Some(j) = find(s, b"]]", false) {
                if ch == '!' {
                    out[i..j + 2].fill(Hidden); // an embed
                } else {
                    // [[target|alias]] shows the alias, [[target]] the target
                    let bar = (s..j).find(|&k| c[k] == '|');
                    out[i..s].fill(Hidden);
                    out[s..bar.map_or(s, |b| b + 1)].fill(Hidden);
                    out[bar.map_or(s, |b| b + 1)..j].fill(Shown);
                    out[j..j + 2].fill(Hidden);
                }
                i = j + 2;
                continue;
            }
        } else if at(i, b"[^") {
            span = find(i, b"]", false).map(|j| (j + 1, Hidden)); // a footnote's marker
        } else if at(i, b"^[") {
            let mut depth = 0;
            let close = (i + 1..n).find(|&j| {
                depth += match c[j] { '[' => 1, ']' => -1, _ => 0 };
                depth == 0
            });
            span = Some((close.map_or(n, |j| j + 1), Hidden)); // an inline footnote
        } else if at(i, b"[@") {
            span = find(i, b"]", false).map(|j| (j + 1, Shown)); // a citation
        } else if at(i, b"[!") {
            span = find(i, b"]", false).filter(|&j| (i + 2..j).all(|k| word(k) || c[k] == '-')).map(|j| (j + 1, Hidden)); // a callout's type
        } else if ch == '[' || at(i, b"![") {
            // [text](url) shows its text; ![alt](url), an image, none
            let s = if ch == '!' { i + 2 } else { i + 1 };
            let link = find(s, b"]", false).filter(|&j| c.get(j + 1) == Some(&'(')).and_then(|j| Some((j, find(j + 2, b")", false)?)));
            if let Some((j, close)) = link {
                out[i..=close].fill(Hidden);
                if ch != '!' {
                    out[s..j].fill(Shown);
                }
                i = close + 1;
                continue;
            }
        } else if at(i, b"http://") || at(i, b"https://") {
            span = Some(((i..n).find(|&j| c[j].is_whitespace()).unwrap_or(n), Shown));
        } else if ch == '<' && c.get(i + 1).is_some_and(|x| x.is_ascii_alphabetic() || *x == '/') {
            span = find(i, b">", false).map(|j| (j + 1, Hidden)); // an HTML tag (its contents are prose)
        } else if ch == '@' && !(i > 0 && (word(i - 1) || c[i - 1] == '.')) && word(i + 1) {
            let e = (i + 1..n).find(|&j| !(word(j) || matches!(c[j], ':' | '.' | '-'))).unwrap_or(n);
            span = Some((e, Shown)); // a narrative citation, @key
        } else if ch == '#' && after_space && c.get(i + 1).is_some_and(|x| !x.is_whitespace() && *x != '#') {
            span = Some(((i..n).find(|&j| c[j].is_whitespace()).unwrap_or(n), Shown)); // a tag
        } else if ch == '^' && after_space && word(i + 1) {
            // a block id, the paragraph's last word
            let e = (i + 1..n).find(|&j| !(word(j) || c[j] == '-')).unwrap_or(n);
            if c[e..].iter().all(|x| x.is_whitespace()) {
                span = Some((n, Hidden));
            }
        }
        match span {
            Some((e, k)) => {
                out[i..e].fill(k);
                i = e;
            }
            None => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(para: &str, list: &str) -> Vec<String> {
        let utf: Vec<u16> = para.encode_utf16().collect();
        source_ranges(para, &terms(list)).iter().map(|&(a, e)| String::from_utf16_lossy(&utf[a as usize..e as usize])).collect()
    }

    #[test]
    fn whole_words_any_case_every_occurrence() {
        assert_eq!(found("A Session, the session's end, sessions and a subsession.", "session"), ["Session", "session"]);
        assert_eq!(found("Two-session design; SESSION.", "session"), ["session", "SESSION"]); // a hyphen is a word's edge
        assert_eq!(found("no match here", "session"), Vec::<String>::new());
        assert_eq!(found("😀 session", "session"), ["session"]); // UTF-16 offsets past the emoji
    }

    #[test]
    fn multiword_terms_span_white_space_and_emphasis_markers() {
        assert_eq!(found("a network  intrusion and a\nnetwork intrusions", "network intrusion"), ["network  intrusion"]);
        assert_eq!(found("the **network** intrusion", "Network   Intrusion"), ["network** intrusion"]);
        assert_eq!(found("networkintrusion", "network intrusion"), Vec::<String>::new());
    }

    #[test]
    fn never_inside_links_code_maths_citations_or_comments() {
        let para = "session [[session]] [[Note|a session]] [text session](http://x/session) `session` $session$ \
                    [@session2020] @session %% session %% <span title=\"session\">session</span> #session ![[session.png]] \
                    https://session.org [^session] ^[a session note] and session ^session";
        assert_eq!(found(para, "session"), ["session", "session", "session"]); // the first, the span's content, the last
        assert_eq!(found("costs $5 and session $10", "session"), ["session"]); // not maths
        assert_eq!(found("a\\[session\\]", "session"), ["session"]);
    }

    #[test]
    fn reading_view_leaves_out_link_words_by_their_place() {
        let para = "The session and [[Other|the session]] then **session** with `session`.";
        let block = "The session and the session then session with ."; // as rendered: code is not in the block's text
        let got: Vec<(u32, u32)> = block_ranges(para, block, &terms("session"));
        assert_eq!(got, vec![(4, 11), (33, 40)]);
        // counts that do not agree (a citation rendered otherwise): every occurrence in the block
        assert_eq!(block_ranges("session [@x]", "session session", &terms("session")).len(), 2);
    }

    #[test]
    fn selections_and_the_list() {
        assert_eq!(term("  **Session**, "), Some("Session".into()));
        assert_eq!(term("network \t intrusion"), Some("network intrusion".into()));
        assert_eq!(term("two\nlines"), None);
        assert_eq!(term("   "), None);
        assert_eq!(term("one two three four five six seven"), None);
        assert_eq!(term("a, b"), None);
        assert_eq!(term("[[link]]"), Some("link".into()));
        let (list, added) = toggle("", "session");
        assert_eq!((list.as_str(), added), ("session", true));
        let (list, added) = toggle(&list, "Network  intrusion");
        assert_eq!((list.as_str(), added), ("session, Network intrusion", true));
        assert!(listed(&list, "SESSION"));
        let (list, added) = toggle(&list, "Session");
        assert_eq!((list.as_str(), added), ("Network intrusion", false));
        assert_eq!(terms("a,\nB , b, c  d"), vec![vec!['a'], vec!['b'], "c d".chars().collect()]);
    }
}
