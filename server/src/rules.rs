//! Candidates from the parse: key terms (noun phrases) and clauses (for points), on Universal Dependencies labels
//! (<https://universaldependencies.org/u/dep/>). (Key phrases, a verb with its complements, were dropped on 2026-10-24:
//! italics are kept for whole clauses, and a fragment such as "call the same task" read as noise.) Terms are ranked KeyBERT-style
//! (<https://maartengr.github.io/KeyBERT/>): cosine similarity to the paragraph (the mean of its sentences), ordered by
//! maximal marginal relevance (Carbonell and Goldstein 1998, <https://doi.org/10.1145/290941.291025>). Similarity alone
//! favours the paragraph's most general words ("study", "assumptions"), so each candidate's score also counts how
//! specific its words are, by how rarely English uses them (wordfreq's Zipf scale: "study" 5.3, "worldview" 3.3,
//! "interpretivism" not in the list of words used once per million), and how often the paragraph repeats it.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use serde_json::{json, Value};

use crate::embed::Embedder;
use crate::text::Doc;

const MAX_WORDS: usize = 4; // longest term
pub const MIN_WORDS_IN_PARAGRAPH: usize = 8;
const LEADING: &[&str] = &["DET", "PRON", "ADP", "CCONJ", "SCONJ", "PART", "NUM", "PUNCT", "SYM", "AUX", "ADV"]; // not X: "bona fide answer"
const HEAD: &[&str] = &["NOUN", "PROPN"];
const CHUNK_DEPS: &[&str] = &["nsubj", "nsubj:pass", "obj", "iobj", "obl", "obl:npmod", "obl:tmod", "nmod", "nmod:npmod", "nmod:tmod", "appos", "root"];
const PREPOSITION_LIKE: &[&str] = &["owing", "according", "including", "regarding", "concerning", "following", "given", "based", "compared", "due"];
/// Words of quantity that end no term ("the study tests none": a misparse makes "none" a noun).
const QUANTITY: &[&str] = &["none", "all", "each", "both", "either", "neither", "some", "any", "many", "few", "several", "most", "much",
    "more", "less", "other", "others", "one", "ones"];

/// English word frequencies on the Zipf scale, for words used at least once per million words (data/wordfreq-en.txt).
static ZIPF: LazyLock<HashMap<&'static str, f32>> = LazyLock::new(|| {
    include_str!("../data/wordfreq-en.txt").lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once(' ')).filter_map(|(w, z)| Some((w, z.parse().ok()?))).collect()
});
/// Below the list's floor: a word used less than once per million words.
const RARE: f32 = 2.5;
/// The score added per Zipf point a candidate's words are rarer than the first value, and taken per point they are
/// commoner (more steeply: a word such as "study" or "data" is almost never what a paragraph is about on its own).
const SPECIFIC: (f32, f32, f32) = (5.0, 0.08, 0.3);
/// The score added for each repetition of a candidate in the paragraph, up to two.
const REPEATED: f32 = 0.01;

/// How common a word is (Zipf); a possessive or plural is looked up as its stem when only that is listed.
fn zipf(w: &str) -> f32 {
    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
    let w = w.strip_suffix("'s").or_else(|| w.strip_suffix("\u{2019}s")).unwrap_or(w);
    ZIPF.get(w).or_else(|| w.strip_suffix('s').and_then(|s| ZIPF.get(s))).copied().unwrap_or(RARE)
}

/// What a candidate (the bytes s..e) adds to its similarity for being specific, from the mean Zipf value of its nouns:
/// an adjective such as "commonest" or "main" says how the thing is ranked, not what it is.
fn specificity(d: &Doc, s: usize, e: usize) -> f32 {
    let z: Vec<f32> = d.toks.iter().filter(|t| t.s >= s && t.e <= e && HEAD.contains(&t.upos.as_str())).map(|t| zipf(&t.lower)).collect();
    if z.is_empty() {
        return 0.0;
    }
    let (common, below, above) = SPECIFIC;
    let d = common - z.iter().sum::<f32>() / z.len() as f32;
    if d >= 0.0 { below * d } else { above * d }
}

/// Adjectives that take "of", which the parser reads as a noun after one ("a writer unsure of the terms").
const ADJ_OF: &[&str] = &["aware", "unaware", "sure", "unsure", "certain", "uncertain", "afraid", "capable", "incapable", "fond", "full",
    "proud", "ashamed", "conscious", "wary", "guilty", "mindful", "tolerant", "intolerant", "free", "short", "devoid", "indicative", "typical"];

/// Fixed phrases, mostly Latin, whose second word starts no term ("fide presentations").
const FIXED: &[(&str, &str)] = &[("bona", "fide"), ("ad", "hoc"), ("de", "facto"), ("a", "priori"), ("a", "posteriori"), ("in", "situ"),
    ("in", "vitro"), ("in", "vivo"), ("per", "se"), ("post", "hoc"), ("vice", "versa"), ("et", "al"), ("status", "quo")];

/// Candidate (lower case, spaces collapsed) -> its byte spans in the text; ordered, so results are reproducible.
pub type Spans = BTreeMap<String, Vec<(usize, usize)>>;

fn add(out: &mut Spans, d: &Doc, s: usize, e: usize) {
    if d.clean(s, e) {
        let spans = out.entry(d.masked[s..e].to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")).or_default();
        if !spans.contains(&(s, e)) {
            spans.push((s, e));
        }
    }
}

/// Noun chunks: a noun with an argument or modifier role, from the left edge of its subtree to itself (spaCy's English
/// rule, <https://github.com/explosion/spaCy/blob/master/spacy/lang/en/syntax_iterators.py>, on UD labels).
fn noun_chunks(d: &Doc) -> Vec<(usize, usize)> {
    let head_of_chunk = |i: usize| {
        let mut h = i;
        while d.toks[h].dep == "conj" && d.toks[h].head.is_some_and(|x| x != h) {
            h = d.toks[h].head.unwrap();
        }
        ["NOUN", "PROPN", "PRON"].contains(&d.toks[i].upos.as_str()) && CHUNK_DEPS.contains(&d.toks[h].dep.as_str())
    };
    // A noun inside the left part of a later chunk ("Sensor" of "Sensor-based methods", which UDPipe labels obl:npmod)
    // belongs to that chunk, not one of its own.
    let inside_later = |i: usize| {
        let (mut a, mut guard) = (d.toks[i].head, 0);
        while let Some(h) = a.filter(|_| guard < d.toks.len()) {
            if h > i && d.left[h] <= i && head_of_chunk(h) {
                return true;
            }
            (a, guard) = (d.toks[h].head, guard + 1);
        }
        false
    };
    let mut out = Vec::new();
    let mut prev_end: Option<usize> = None;
    for i in 0..d.toks.len() {
        if head_of_chunk(i) && !inside_later(i) && !prev_end.is_some_and(|p| d.left[i] <= p) {
            out.push((d.left[i], i));
            prev_end = Some(i);
        }
    }
    out
}

pub fn terms(d: &Doc) -> Spans {
    let mut out = Spans::new();
    let bracketed = d.bracketed();
    // the authors the paragraph cites, by the letters that start its citation keys ("Biggam puts the risk bluntly …
    // [@biggam2022]"): a name, not a term
    let cited: Vec<String> = d.text.split('@').skip(1).map(|k| k.chars().take_while(char::is_ascii_alphabetic).collect::<String>().to_lowercase()).filter(|k| k.len() > 1).collect();
    for (a, b) in noun_chunks(d) {
        let mut toks: Vec<usize> = (a..=b).collect();
        // also words that work as prepositions whatever their tag ("including searches": a verb labelled case)
        let leading = |t: usize| LEADING.contains(&d.toks[t].upos.as_str()) || d.toks[t].stop() || ["case", "mark"].contains(&d.toks[t].dep.as_str())
            || PREPOSITION_LIKE.contains(&d.toks[t].lower.as_str());
        while toks.first().is_some_and(|&t| leading(t)) {
            toks.remove(0);
        }
        while toks.last().is_some_and(|&t| d.toks[t].punct() || d.toks[t].stop()) {
            toks.pop();
        }
        if toks.last().is_none_or(|&t| !HEAD.contains(&d.toks[t].upos.as_str()) || ADJ_OF.contains(&d.toks[t].lower.as_str())) {
            continue; // a term ends with its noun
        }
        for i in 0..toks.len() {
            let tail = &toks[i..];
            let (first, prev) = (&d.toks[tail[0]], i.checked_sub(1).map(|p| toks[p]));
            let words = tail.iter().filter(|&&t| !d.toks[t].punct()).count();
            if words > MAX_WORDS || !tail.iter().any(|&t| HEAD.contains(&d.toks[t].upos.as_str())) || LEADING.contains(&first.upos.as_str()) || first.stop()
                || tail.iter().any(|&t| d.toks[t].punct() && !["-", "/", "'", "\u{2019}"].contains(&d.toks[t].form.as_str())) // a list, a quotation
                || prev.is_some_and(|p| !d.space_after(p)) // inside "feature-based", "PAD/OCC"
                || prev.is_some_and(|p| d.toks[p].form.starts_with(char::is_uppercase) && first.form.starts_with(char::is_uppercase)) // inside a name
                || prev.is_some_and(|p| FIXED.contains(&(d.toks[p].lower.as_str(), first.lower.as_str()))) // inside "bona fide"
                || tail.iter().any(|&t| cited.contains(&d.toks[t].lower))
                || (words == 1 && (first.form.chars().count() < 3 || first.num()))
                || QUANTITY.contains(&d.toks[*tail.last().unwrap()].lower.as_str())
                // a verb read as a noun: "FAR counts attacks", "groups artefacts" (English rarely puts a plural first)
                || tail[..tail.len() - 1].iter().any(|&t| {
                    let w = &d.toks[t].lower;
                    d.toks[t].upos == "NOUN" && w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is")
                })
                || tail.iter().any(|&t| bracketed[t]) // a reference's or an aside's words: "(Literature Search and Screening)"
                // a clause, not a name: "domain is the set", "differently by different scientists" (only "of" joins a term)
                || tail.iter().any(|&t| d.toks[t].upos == "AUX" || d.toks[t].lemma == "be" || (d.toks[t].upos == "ADP" && d.toks[t].lower != "of"))
                // a verb read as the last noun: "the standard counts the share", "the claim requires a control"
                || { let l = *tail.last().unwrap(); let w = &d.toks[l].lower;
                     w.ends_with('s') && !w.ends_with("ss") && d.toks.get(l + 1).is_some_and(|x| ["the", "a", "an", "each", "every", "its", "their", "this", "these"].contains(&x.lower.as_str())) }
            {
                continue;
            }
            add(&mut out, d, first.s, d.toks[*tail.last().unwrap()].e);
        }
    }
    out
}

/// Clauses: sentences, cut before a clause with its own subject that hangs off the root or a verb of the root, and
/// after a semicolon, never inside brackets; a link at either end is left out.
pub fn clauses(d: &Doc) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for sent in d.sentences() {
        let mut inside = vec![false; d.toks.len()];
        let mut depth = 0;
        for i in sent.clone() {
            match d.toks[i].form.as_str() {
                "(" | "[" => depth += 1,
                ")" | "]" => depth = (depth - 1i32).max(0),
                _ => inside[i] = depth > 0,
            }
        }
        let root = sent.clone().find(|&i| d.toks[i].dep == "root");
        let mut cuts = vec![sent.start];
        for i in sent.clone().filter(|&i| !inside[i]) {
            let t = &d.toks[i];
            let under_root = root.is_some_and(|r| t.head == Some(r) || t.head.and_then(|h| d.toks[h].head) == Some(r));
            if ["conj", "advcl", "parataxis"].contains(&t.dep.as_str()) && under_root && !inside[d.left[i]]
                && d.children[i].iter().any(|&c| ["nsubj", "nsubj:pass", "expl"].contains(&d.toks[c].dep.as_str()))
            {
                cuts.push(d.left[i]);
            }
            if t.form == ";" {
                cuts.push(i + 1);
            }
        }
        cuts.retain(|c| sent.contains(c));
        cuts.sort_unstable();
        cuts.dedup();
        cuts.push(sent.end);
        for w in cuts.windows(2) {
            let mut toks: Vec<usize> = (w[0]..w[1]).collect();
            while toks.first().is_some_and(|&t| d.toks[t].punct() || d.toks[t].dep == "cc" || d.in_link(t)) {
                toks.remove(0);
            }
            while toks.last().is_some_and(|&t| [",", ";", ":"].contains(&d.toks[t].form.as_str()) || d.toks[t].dep == "cc" || d.in_link(t)
                || (d.toks[t].punct() && toks.len() > 1 && d.in_link(toks[toks.len() - 2])))
            {
                toks.pop();
            }
            if toks.iter().filter(|&&t| d.toks[t].alpha()).count() >= 4 {
                let (s, mut e) = (d.toks[toks[0]].s, d.toks[*toks.last().unwrap()].e);
                let seg = &d.masked[s..e]; // a blanked citation before the full stop
                let core = seg.trim_end_matches(['.', ',', ';', ':', '!', '?']);
                if core.len() < seg.len() && core.ends_with(char::is_whitespace) {
                    e = s + core.trim_end().len();
                }
                out.push((s, e));
            }
        }
    }
    out
}

/// Maximal marginal relevance; a candidate inside, or containing, a chosen one adds nothing and is skipped.
fn mmr(score: &[f32], vecs: &[Vec<f32>], keys: &[&String], top_k: usize, diversity: f32) -> Vec<usize> {
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let lam = 1.0 - diversity;
    let (mut chosen, mut redundancy, mut alive) = (Vec::new(), vec![0f32; keys.len()], vec![true; keys.len()]);
    while chosen.len() < top_k {
        let Some(j) = (0..keys.len()).filter(|&i| alive[i]).max_by(|&a, &b| {
            (lam * score[a] - (1.0 - lam) * redundancy[a]).total_cmp(&(lam * score[b] - (1.0 - lam) * redundancy[b]))
        }) else { break };
        chosen.push(j);
        alive[j] = false;
        for k in 0..keys.len() {
            redundancy[k] = redundancy[k].max(dot(&vecs[k], &vecs[j]));
            let contains = |a: &str, b: &str| format!(" {a} ").contains(&format!(" {b} "));
            if alive[k] && (contains(keys[j], keys[k]) || contains(keys[k], keys[j])) {
                alive[k] = false;
            }
        }
    }
    chosen
}

/// Key terms of a paragraph, ranked, as the protocol's spans, after its quotations and definitions (meaning.rs).
pub fn rank(d: &Doc, emb: &Embedder, top_k: usize, diversity: f32) -> candle_core::Result<Vec<Value>> {
    let mut out = crate::meaning::spans(d);
    if d.toks.iter().filter(|t| t.alpha()).count() < MIN_WORDS_IN_PARAGRAPH {
        return Ok(named_first(d, out));
    }
    let kinds = [("term", terms(d), top_k)];
    let sentences: Vec<String> = d.sentences().into_iter().filter(|r| r.clone().any(|i| d.toks[i].alpha())).map(|r| d.visible(d.toks[r.start].s, d.toks[r.end - 1].e)).collect();
    let keys: Vec<&String> = kinds.iter().flat_map(|(_, c, _)| c.keys()).collect();
    if keys.is_empty() || sentences.is_empty() {
        return Ok(named_first(d, out));
    }
    let (s, k): (Vec<&str>, Vec<&str>) = (sentences.iter().map(String::as_str).collect(), keys.iter().map(|k| k.as_str()).collect());
    let emb = emb.encode_groups(&[&s, &k])?;
    let mut centre = vec![0f32; emb[0].len()];
    for v in &emb[..sentences.len()] {
        centre.iter_mut().zip(v).for_each(|(c, x)| *c += x);
    }
    let norm = centre.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    centre.iter_mut().for_each(|c| *c /= norm);
    let mut at = sentences.len();
    for (kind, cands, k) in &kinds {
        let names: Vec<&String> = cands.keys().collect();
        let vecs = &emb[at..at + names.len()];
        at += names.len();
        let score: Vec<f32> = vecs.iter().zip(&names).map(|(v, name)| {
            let similarity: f32 = v.iter().zip(&centre).map(|(a, b)| a * b).sum();
            let (s, e) = cands[*name][0];
            similarity + specificity(d, s, e) + REPEATED * (cands[*name].len() - 1).min(2) as f32
        }).collect();
        let mut done: Vec<&String> = Vec::new();
        for j in mmr(&score, vecs, &names, *k, diversity) {
            // a chosen term brings the other items of its list, at its score, so that a list is drawn whole ("illumination
            // uniformity, mean luminance and sharpness")
            for name in std::iter::once(names[j]).chain(list_mates(d, cands, names[j])) {
                if done.contains(&name) {
                    continue;
                }
                done.push(name);
                let spans = &cands[name];
                let (s, e) = spans[0];
                out.push(json!({ "k": kind, "t": &d.text[s..e], "w": (score[j] * 1e4).round() / 1e4, "v": d.visible(s, e),
                    "at": spans.iter().map(|&(a, b)| [d.utf16(a), d.utf16(b)]).collect::<Vec<_>>() }));
            }
        }
    }
    Ok(named_first(d, out))
}

/// The other items of the list a term is in: "A, B and C", "A, B, or C", "A or B", found by the words between them (the
/// parser often misreads a list: it took "mean" in "…, mean luminance and sharpness" for the sentence's verb). Each item
/// is the longest candidate that ends where the item ends; the run must hold "and" or "or", so an aside between commas
/// ("an adversarial attack, a change to the pixels …") is not a list.
fn list_mates<'a>(d: &Doc, cands: &'a Spans, term: &str) -> Vec<&'a String> {
    const SEPS: &[&str] = &[", and ", ", or ", " and ", " or ", ", "];
    const ENDS: &[char] = &[',', ';', '.', ':', '(', ')', '[', ']', '?', '!'];
    let text = &d.masked;
    // the longest candidate that lies in a..b and ends at b
    let ending_at = |a: usize, b: usize| {
        cands.iter().filter(|(_, spans)| spans.iter().any(|&(s, e)| s >= a && e == b)).max_by_key(|(k, _)| k.len()).map(|(k, _)| k)
    };
    let Some(&(s, e)) = cands.get(term).and_then(|spans| spans.first()) else { return Vec::new() };
    let (mut out, mut joined) = (Vec::new(), false);
    // rightwards: a separator, then an item up to the next separator or punctuation (at most 60 bytes)
    let mut pos = e;
    while let Some(sep) = SEPS.iter().find(|sep| text[pos..].starts_with(**sep)) {
        let a = pos + sep.len();
        let rest = &text[a..text.len().min(a + 60)];
        let rest = &rest[..(0..=rest.len()).rev().find(|&i| rest.is_char_boundary(i)).unwrap_or(0)];
        let cut = [rest.find(ENDS), rest.find(" and "), rest.find(" or ")].into_iter().flatten().min().unwrap_or(rest.len());
        let b = a + rest[..cut].trim_end().len();
        let Some(item) = ending_at(a, b) else { break };
        joined |= sep.contains("and") || sep.contains("or");
        out.push(item);
        pos = b;
        if sep.contains("and") || sep.contains("or") {
            break; // the last item
        }
    }
    // leftwards: an item that ends just before a separator
    let mut pos = s;
    while let Some(sep) = SEPS.iter().find(|sep| text[..pos].ends_with(**sep)) {
        let b = pos - sep.len();
        let before = &text[b.saturating_sub(60)..b];
        let before = &before[(0..before.len()).find(|&i| before.is_char_boundary(i)).unwrap_or(0)..];
        let start = [before.rfind(ENDS).map(|i| i + 1), before.rfind(" and ").map(|i| i + 5), before.rfind(" or ").map(|i| i + 4)].into_iter().flatten().max().unwrap_or(0);
        let a = b - before.len() + start;
        let Some(item) = ending_at(a, b) else { break };
        joined |= sep.contains("and") || sep.contains("or");
        out.push(item);
        pos = cands[item].iter().find(|&&(x, y)| x >= a && y == b).map_or(a, |&(x, _)| x);
    }
    if joined { out.into_iter().filter(|k| k.as_str() != term).collect() } else { Vec::new() }
}

/// Named terms (the name a naming sentence gives, a quoted word: meaning.rs; an acronym) weigh NAMED, so the plugin draws
/// them at any sensitivity without counting them; the same words ranked again are dropped.
fn named_first(d: &Doc, mut out: Vec<Value>) -> Vec<Value> {
    let is_term = |v: &Value| v["k"] == "term";
    out.extend(acronyms(d));
    let named: Vec<String> = out.iter().filter(|v| v.get("named").is_some()).filter_map(|v| v["v"].as_str().map(str::to_lowercase)).collect();
    out.retain(|v| !(is_term(v) && v.get("named").is_none() && v["v"].as_str().is_some_and(|s| named.contains(&s.to_lowercase()))));
    for v in out.iter_mut().filter(|v| v.get("named").is_some()) {
        v["w"] = json!(NAMED);
        v.as_object_mut().map(|o| o.remove("named"));
    }
    out
}

/// The weight of a named term (pick.rs in the plugin: drawn always, above any ranked term's score).
const NAMED: f32 = 2.0;
/// Capitals that name an organisation or a format rather than a term of the subject.
const NOT_TERMS: &[&str] = &["ISO", "IEC", "BSI", "IEEE", "UK", "US", "USA", "EU", "UN", "PDF", "URL", "II", "III", "IV", "VI"];

/// Acronyms (ROC, AUC, NID): two or more capitals with digits or a lower-case "s" ("NIDs") allowed, outside links,
/// citations and code, each with every place it occurs. In technical writing they are the terms a paragraph is built on.
fn acronyms(d: &Doc) -> Vec<Value> {
    let mut found: BTreeMap<&str, Vec<(usize, usize)>> = BTreeMap::new();
    for (i, t) in d.toks.iter().enumerate() {
        let w = t.form.strip_suffix('s').filter(|w| w.len() >= 2).unwrap_or(&t.form);
        let caps = w.chars().filter(char::is_ascii_uppercase).count();
        // part of a longer name ("COCO-Stuff", "MS-COCO", "NER/POS"): a letter, hyphen or slash against it
        let joins = |x: &crate::text::Tok| x.alpha() || ["-", "/", "\u{2010}", "\u{2011}"].contains(&x.form.as_str());
        let joined = (i > 0 && !d.space_after(i - 1) && joins(&d.toks[i - 1])) || (i + 1 < d.toks.len() && !d.space_after(i) && joins(&d.toks[i + 1]));
        if caps >= 2 && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) && !NOT_TERMS.contains(&w) && !joined
            && !d.in_link(i) && d.clean(t.s, t.e)
        {
            found.entry(w).or_default().push((t.s, t.e));
        }
    }
    found.into_iter().map(|(w, spans)| {
        let (s, e) = spans[0];
        json!({ "k": "term", "t": &d.text[s..e], "w": NAMED, "v": w, "named": true,
            "at": spans.iter().map(|&(a, b)| [d.utf16(a), d.utf16(b)]).collect::<Vec<_>>() })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_is_drawn_whole() {
        let Some(parser) = crate::text::tests::parser() else { return };
        let text = "The standard separates the quality components that depend on the network and the hosting environment, among them packet loss, latency and jitter, from those that depend on the user.";
        let d = Doc::parse(text, &parser);
        let c = terms(&d);
        let mut mates: Vec<&str> = list_mates(&d, &c, "packet loss").into_iter().map(String::as_str).collect();
        mates.sort();
        assert_eq!(mates, ["jitter", "latency"], "{:?}", c.keys());
        let text = "This is why an adversarial attack, a change to the pixels of a digital image, is not a network intrusion.";
        let d = Doc::parse(text, &parser);
        assert!(list_mates(&d, &terms(&d), "adversarial attack").is_empty());
    }
}
