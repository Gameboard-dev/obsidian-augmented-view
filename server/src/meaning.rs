//! Quotations and definitions in a paragraph, by rule (no model, so no GPU): what the plugin draws in italics (quoted
//! words) and underlined (what defines a term), both together for a quoted definition.
//!
//! A quotation is the text between double quotation marks, straight or curly, outside code, links and citations. A
//! definition is, on the parse (Universal Dependencies labels, <https://universaldependencies.org/u/dep/>):
//! - a quotation in a sentence that says it defines or names something ("in ISO's definition", "defined as",
//!   "names", "means", "refers to", "known as", "called"), or that follows the term it defines: after a comma or a
//!   bracketed abbreviation ("network intrusion detector (NID), "…""), after a form of "be" ("is a "…"", "is
//!   one "…""), or after a relative pronoun ("an impostor, who "…"");
//! - an apposition that glosses a noun ("an adversarial attack, a change to the pixels of a digital image");
//! - a predicate noun that classifies its subject and says how ("X is a Y that …", "X is a Y of …");
//! - what follows "defined as", "means", "denotes" or "refers to".

use crate::text::Doc;

/// Words (lemmas) that say a sentence defines or names something.
const CUES: &[&str] = &["define", "definition", "mean", "meaning", "denote", "refer", "term", "call", "name", "know"];

/// The parts of [s, e) outside markup that was blanked (links, citations, locators), each trimmed; none without a
/// letter or digit.
pub fn clean_parts(d: &Doc, s: usize, e: usize) -> Vec<(usize, usize)> {
    let (m, t) = (d.masked.as_bytes(), d.text.as_bytes());
    let keep = |i: usize| (m[i] == t[i] || b"*_=~\\".contains(&t[i])) && !d.links.iter().any(|&(a, b)| a <= i && i < b);
    let (mut out, mut i) = (Vec::new(), s);
    while i < e {
        while i < e && !keep(i) {
            i += 1;
        }
        let a = i;
        while i < e && keep(i) {
            i += 1;
        }
        let seg = &d.text[a..i];
        let lead = seg.len() - seg.trim_start().len();
        let core = seg.trim().trim_end_matches(['.', ',', ';', ':']).trim_end();
        if core.chars().any(char::is_alphanumeric) {
            out.push((a + lead, a + lead + core.len()));
        }
    }
    out
}

/// Quotations: (the opening mark's offset, the quoted text's byte span), in order.
pub fn quotes(d: &Doc) -> Vec<(usize, (usize, usize))> {
    let (mut out, mut open): (Vec<_>, Option<usize>) = (Vec::new(), None);
    for (i, c) in d.text.char_indices() {
        if d.masked.as_bytes()[i] != d.text.as_bytes()[i] {
            continue; // inside code, a link or a citation
        }
        let closes = match c {
            '\u{201c}' => false,
            '\u{201d}' => true,
            '"' => open.is_some(),
            _ => continue,
        };
        match (closes, open) {
            (true, Some(o)) => {
                let (s, e) = (o + d.text[o..].chars().next().map_or(1, char::len_utf8), i);
                if s < e && e - s <= 1200 {
                    out.push((o, (s, e)));
                }
                open = None;
            }
            (false, _) => open = Some(i),
            (true, None) => {}
        }
    }
    out
}

/// The tokens of the sentence holding byte `at`, and the index of the first token at or after it.
fn sentence_at(d: &Doc, at: usize) -> Option<(std::ops::Range<usize>, usize)> {
    let first = d.toks.iter().position(|t| t.s >= at)?;
    let r = d.sentences().into_iter().find(|r| r.contains(&first))?;
    Some((r, first))
}

fn is_noun(d: &Doc, t: usize) -> bool {
    ["NOUN", "PROPN"].contains(&d.toks[t].upos.as_str())
}

/// Whether noun `t` names a term rather than points back to something said: not "those three steps", "this rule",
/// "its first step" (a demonstrative, a number or a possessive before it).
fn a_term(d: &Doc, t: usize) -> bool {
    is_noun(d, t) && !d.children[t].iter().any(|&c| {
        let x = &d.toks[c];
        ["this", "that", "these", "those"].contains(&x.lower.as_str()) || x.upos == "NUM" || x.dep == "nmod:poss"
    })
}

/// Whether the quotation opening at byte `open` (its first token at or after `open`) defines something.
fn quote_defines(d: &Doc, clauses: &[(usize, usize)], open: usize, end: usize) -> bool {
    let Some((sent, first)) = sentence_at(d, open) else { return false };
    let mut i = first;
    while i > sent.start && d.toks[i - 1].s >= open {
        i -= 1;
    }
    let before: Vec<usize> = (sent.start..i).rev().filter(|&t| !d.in_link(t)).collect();
    let tok = |k: usize| before.get(k).map(|&t| &d.toks[t]);
    // A defining word in the quotation's own clause, unless the quotation is introduced as something else ("by",
    // "because", "that" …; "as" is fine: "defined as").
    let functional = |t: &crate::text::Tok| t.upos == "SCONJ" || (t.upos == "ADP" && t.lower != "as");
    let opens = d.toks.get(first + 1).filter(|t| t.s < end); // the quotation's own first word: "by normalizing …"
    let introduced = tok(0).is_some_and(functional) || opens.is_some_and(functional);
    let clause = clauses.iter().find(|&&(s, e)| s <= open && open < e).copied().unwrap_or((d.toks[sent.start].s, end));
    let in_clause = |t: &usize| d.toks[*t].s >= clause.0 && d.toks[*t].e <= clause.1 && (d.toks[*t].e <= open || d.toks[*t].s > end);
    if !introduced && sent.clone().filter(in_clause).any(|t| CUES.contains(&d.toks[t].lemma.as_str())) {
        return true;
    }
    // Then, walking back from the quotation's opening mark: a comma or colon (after a bracketed group) after a noun
    // (apposition); "be" (with "a", "an", "the" or "one" after it); a relative pronoun after a noun.
    match tok(0) {
        Some(t) if t.lemma == "be" => true,
        Some(t) if ["a", "an", "the", "one"].contains(&t.lower.as_str()) => tok(1).is_some_and(|u| u.lemma == "be"),
        Some(t) if t.upos == "PRON" && ["who", "which", "that"].contains(&t.lower.as_str()) => before.get(1).is_some_and(|&u| is_noun(d, u) || d.toks[u].form == ","),
        Some(t) if [",", ":"].contains(&t.form.as_str()) => {
            let mut k = 1;
            if tok(1).is_some_and(|u| u.form == ")") {
                while tok(k).is_some_and(|u| u.form != "(") {
                    k += 1;
                }
                k += 1;
            }
            before.get(k).is_some_and(|&u| is_noun(d, u))
        }
        _ => false,
    }
}

/// Whether verb `v`'s subject names a term: a name or a bare noun ("NetFlow here means", "Robustness refers to"), a
/// word for a term ("the term X", "this abbreviation"), or something quoted or linked; not "the claim", "it", "this".
fn names_a_term(d: &Doc, v: usize) -> bool {
    let Some(subj) = d.children[v].iter().copied().find(|&c| ["nsubj", "nsubj:pass"].contains(&d.toks[c].dep.as_str())) else { return false };
    let s = &d.toks[subj];
    let determiner = d.children[subj].iter().any(|&c| d.toks[c].upos == "DET");
    let quoted_or_linked = (d.left[subj]..=d.right[subj]).any(|t| d.in_link(t) || ["\"", "\u{201c}", "*"].contains(&d.toks[t].form.as_str()));
    s.upos == "PROPN" || (s.upos == "NOUN" && !determiner) || quoted_or_linked
        || ["term", "word", "phrase", "expression", "abbreviation", "acronym", "name"].contains(&s.lemma.as_str())
}

/// "Choosing the range of values that the codes cover is called calibration": "is" (are, was, were) and a naming verb
/// ("called", "termed", "named", "dubbed", "known as"). Each as (the definition: the clause up to "is"; the name: the
/// words after the verb, up to punctuation, at most six). By the words alone: the parser often misreads such a
/// subject (here it took "codes cover" for a noun). Not when the subject is a lone pronoun ("This is called …").
pub fn namings(d: &Doc) -> Vec<((usize, usize), (usize, usize))> {
    const NAMING: &[&str] = &["called", "termed", "named", "dubbed", "known"];
    const STOPS: &[&str] = &[",", ";", ":", ".", "(", ")", "?", "!"];
    let mut out = Vec::new();
    for t in 1..d.toks.len().saturating_sub(1) {
        if !["is", "are", "was", "were"].contains(&d.toks[t].lower.as_str()) || !NAMING.contains(&d.toks[t + 1].lower.as_str()) {
            continue;
        }
        let sent = d.toks[t].sent;
        // the clause's start: after the last stop in the sentence (or a conjunction after a comma), before "is"
        let mut first = t;
        while first > 0 && d.toks[first - 1].sent == sent && ![";", ":", "(", ")"].contains(&d.toks[first - 1].form.as_str())
            && !(d.toks[first - 1].form == "," && first >= 2 && ["and", "but", "while"].contains(&d.toks.get(first).map_or("", |x| x.lower.as_str())))
        {
            first -= 1;
        }
        while first < t && (d.toks[first].punct() || ["and", "but", "while", "so"].contains(&d.toks[first].lower.as_str())) {
            first += 1;
        }
        let mut n = t + 2;
        if d.toks.get(n).is_some_and(|x| x.lower == "as") {
            n += 1; // "known as X"
        } else if d.toks[t + 1].lower == "known" || d.toks.get(n).is_some_and(|x| x.upos == "ADP" || ["for", "upon", "about", "on", "into", "off", "out", "up"].contains(&x.lower.as_str())) {
            continue; // "what is known about …", "is called for": no naming
        }
        let mut last = n;
        while last + 1 < d.toks.len() && d.toks[last + 1].sent == sent && !STOPS.contains(&d.toks[last + 1].form.as_str()) && last + 1 < n + 6 {
            last += 1;
        }
        let lone_pronoun = t - first == 1 && d.toks[first].upos == "PRON";
        if first < t && n < d.toks.len() && !STOPS.contains(&d.toks[n].form.as_str()) && !lone_pronoun {
            out.push(((d.toks[first].s, d.toks[t - 1].e), (d.toks[n].s, d.toks[last].e)));
        }
    }
    out
}

/// Definitions: byte spans to underline (overlapping ones merged).
pub fn definitions(d: &Doc, quoted: &[(usize, (usize, usize))]) -> Vec<(usize, usize)> {
    let clauses = crate::rules::clauses(d);
    let mut out: Vec<(usize, usize)> = quoted.iter().filter(|(o, (_, e))| quote_defines(d, &clauses, *o, *e)).map(|&(_, span)| span).collect();
    let in_quote = |s: usize, e: usize| quoted.iter().any(|&(o, (_, qe))| s <= qe && o < e);
    let has = |t: usize, f: &dyn Fn(&crate::text::Tok) -> bool| d.children[t].iter().any(|&c| f(&d.toks[c]));
    let subtree = |t: usize| (d.toks[d.left[t]].s, d.toks[d.right[t]].e);
    for (t, tok) in d.toks.iter().enumerate() {
        let indefinite = |x: &crate::text::Tok| x.upos == "DET" && ["a", "an"].contains(&x.lower.as_str());
        let definite = |x: &crate::text::Tok| x.upos == "DET" && x.lower == "the" && x.dep == "det";
        let subject = d.children[t].iter().copied().find(|&c| ["nsubj", "nsubj:pass"].contains(&d.toks[c].dep.as_str()));
        // A gloss: "a/an …" between commas after a noun, found by position (the parser often labels it an object),
        // and not an item of a list ("a printed photograph, a video …, or a mask"), told by the words around it (the
        // parser labels some glosses list items too).
        let (mut l, mut r) = (d.left[t], d.right[t]);
        while l < t && d.toks[l].punct() {
            l += 1; // the parser may attach the comma before a gloss to it
        }
        while r > t && d.toks[r].punct() {
            r -= 1;
        }
        let gloss = l >= 2 && indefinite(&d.toks[l]) && d.toks[l - 1].form == "," && is_noun(d, l - 2)
            && d.toks.get(r + 1).is_some_and(|x| [",", ".", ";", ")"].contains(&x.form.as_str()))
            && !d.toks.get(r + 2).is_some_and(|x| ["and", "or"].contains(&x.lower.as_str()))
            && !d.children[t].iter().any(|&c| d.toks[c].dep == "conj");
        let span = if tok.upos == "NOUN" && gloss {
            Some((d.toks[l].s, d.toks[r].e)) // "an adversarial attack, a change to the pixels of a digital image"
        } else if tok.upos == "NOUN" && has(t, &|x| x.dep == "cop") && subject.is_some_and(|c| a_term(d, c))
            && (has(t, &indefinite) || (has(t, &definite) && subject.is_some_and(|c| !d.children[c].iter().any(|&g| d.toks[g].upos == "DET"))))
            && has(t, &|x| ["acl", "acl:relcl", "nmod"].contains(&x.dep.as_str()))
        {
            // "X is a Y that/of …", and "X is the Y …" when X is a bare term ("Positivism is the philosophy beneath the
            // scientific method"; not "the result is the average …"): from the article to the end of the predicate. Not
            // "Those three steps are a useful checklist for …": the subject must be a term.
            let a = d.children[t].iter().copied().find(|&c| indefinite(&d.toks[c]) || definite(&d.toks[c])).unwrap_or(t);
            Some((d.toks[a].s, d.toks[d.right[t]].e))
        } else if ["define", "mean", "denote", "refer"].contains(&tok.lemma.as_str()) && tok.upos == "VERB" && (tok.lemma == "define" || names_a_term(d, t)) {
            // what follows "defined as" ("X is defined as Y", "we define X as Y"; not "define the basic ideas"), or "X
            // means / denotes / refers to" when X is a term (not "the claim refers to …")
            // ("X is defined by Y" too: "Each protocol is defined by which session … is held out")
            let as_ = |c: usize| tok.lemma != "define" || (d.left[c]..=d.right[c]).any(|g| ["as", "by"].contains(&d.toks[g].lower.as_str()) && ["case", "mark"].contains(&d.toks[g].dep.as_str()));
            d.children[t].iter().copied().filter(|&c| c > t && ["obj", "obl", "xcomp", "advcl"].contains(&d.toks[c].dep.as_str()) && as_(c)).map(subtree).next()
        } else {
            None
        };
        if let Some((s, e)) = span.filter(|&(s, e)| !in_quote(s, e)) {
            out.extend(clean_parts(d, s, e));
        }
    }
    // "Each protocol is defined by which session … is held out, and …": "is (are, was, were) defined as / by", by the
    // words alone (the parser can hang "by" on a later verb), defined by the rest of the clause: up to a stop, or a
    // comma before "and" or "but"
    for t in 1..d.toks.len().saturating_sub(2) {
        if d.toks[t].lower != "defined" || !["is", "are", "was", "were"].contains(&d.toks[t - 1].lower.as_str()) || !["as", "by"].contains(&d.toks[t + 1].lower.as_str()) {
            continue;
        }
        let start = t + 2;
        let end = (start..d.toks.len()).find(|&u| {
            [";", ".", ":", "(", "?", "!"].contains(&d.toks[u].form.as_str()) || d.toks[u].sent != d.toks[t].sent
                || (d.toks[u].form == "," && d.toks.get(u + 1).is_some_and(|x| ["and", "but", "while", "whereas"].contains(&x.lower.as_str())))
        }).unwrap_or(d.toks.len());
        if end > start && !in_quote(d.toks[start].s, d.toks[end - 1].e) {
            out.extend(clean_parts(d, d.toks[start].s, d.toks[end - 1].e));
        }
    }
    for ((s, e), _) in namings(d) {
        if !in_quote(s, e) {
            out.extend(clean_parts(d, s, e));
        }
    }
    // "FAR counts attacks accepted, and FRR counts real accesses rejected": an abbreviation, then "counts", "measures",
    // "is", "means" or "denotes", defines it with the rest of its clause. By the words alone, as the parser often reads
    // such a verb as a noun.
    for t in 0..d.toks.len().saturating_sub(2) {
        let abbr = &d.toks[t].form;
        let is_abbr = abbr.chars().count() >= 2 && abbr.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) && abbr.chars().any(|c| c.is_ascii_uppercase());
        // and a linked term (a glossary entry) that "means", "denotes" or "refers to", perhaps after "here" ("NetFlow here
        // means detecting physical attacks …"), defined by the rest of its sentence up to a semicolon or colon
        let linked = d.in_link(t) && !d.in_link(t + 1);
        let v = if ["here", "thus", "simply", "usually", "also", "therefore"].contains(&d.toks[t + 1].lower.as_str()) { t + 2 } else { t + 1 };
        let verb = d.toks.get(v).map_or("", |x| x.lower.as_str());
        let start = if verb == "refers" && d.toks.get(v + 1).is_some_and(|x| x.lower == "to") { v + 2 } else { v + 1 };
        let (defines, stops): (bool, &[&str]) = if is_abbr && v == t + 1 && ["counts", "measures", "is", "means", "denotes", "gives"].contains(&verb) {
            (true, &[",", ";", ".", "(", ":"])
        } else if (is_abbr || linked) && ["means", "denotes", "refers"].contains(&verb) {
            (true, &[";", ".", "(", ":"])
        } else {
            (false, &[])
        };
        if !defines || start >= d.toks.len() {
            continue;
        }
        let end = (start..d.toks.len()).find(|&u| stops.contains(&d.toks[u].form.as_str()) || d.toks[u].sent != d.toks[t].sent).unwrap_or(d.toks.len());
        if end > start && !in_quote(d.toks[start].s, d.toks[end - 1].e) {
            out.extend(clean_parts(d, d.toks[start].s, d.toks[end - 1].e));
        }
    }
    out.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in out {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Option<(Vec<String>, Vec<String>)> {
        let d = Doc::parse(text, &crate::text::tests::parser()?);
        let q = quotes(&d);
        let quoted = q.iter().flat_map(|&(_, (s, e))| clean_parts(&d, s, e)).map(|(s, e)| text[s..e].to_string()).collect();
        Some((quoted, definitions(&d, &q).into_iter().map(|(s, e)| text[s..e].to_string()).collect()))
    }

    #[test]
    fn quotations_and_the_definitions_among_them() {
        let text = "A network intrusion is, in the standard's definition, a \u{201c}connection to the protected subsystem\u{201d} (Clause 3.5, p. 2). \
            Intrusions \"can have a number of goals\" [@iso27001-1]. The tool used is a network intrusion detector (NID), \"a tool used in an intrusion\". \
            The standard insists that \"error rates of one set cannot be assumed\", with \"recipe\" the usual word for the method.";
        let Some((quoted, defs)) = found(text) else { return };
        assert_eq!(quoted, ["connection to the protected subsystem", "can have a number of goals", "a tool used in an intrusion", "error rates of one set cannot be assumed", "recipe"]);
        assert_eq!(defs, ["connection to the protected subsystem", "a tool used in an intrusion"]);
    }

    #[test]
    fn definitions_without_quotations() {
        let text = "This is why an adversarial attack, a change to the pixels of a digital image, is not a network intrusion. \
            A detector is a classifier that separates normal traffic from attacks. The series concerns attacks at the device, not \
            attacks on the network (Clause 1, p. 1), which is why an [[Glossary#Adversarial attack|adversarial]] attack, a \
            change to the pixels of an image, is not one. Typical tools are a port scanner, a script replayed on a \
            server, or a proxy.";
        let Some((_, defs)) = found(text) else { return };
        assert_eq!(defs, ["a change to the pixels of a digital image", "a classifier that separates normal traffic from attacks", "a \
            change to the pixels of an image"]);
        let text = "FAR counts attacks accepted, and FRR counts real accesses rejected; both depend on the threshold.";
        let Some((_, defs)) = found(text) else { return };
        assert_eq!(defs, ["attacks accepted", "real accesses rejected"]);
        let d = Doc::parse(text, &crate::text::tests::parser().unwrap());
        assert!(crate::rules::terms(&d).keys().all(|k| !k.contains("counts")), "{:?}", crate::rules::terms(&d).keys());
        let text = "Choosing the range of values that the codes cover is called calibration. A gap of this kind is known as a blind spot.";
        let Some((_, defs)) = found(text) else { return };
        assert_eq!(defs, ["Choosing the range of values that the codes cover", "A gap of this kind"]);
        let d = Doc::parse(text, &crate::text::tests::parser().unwrap());
        let names: Vec<&str> = namings(&d).iter().map(|&(_, (s, e))| &text[s..e]).collect();
        assert_eq!(names, ["calibration", "a blind spot"]);
        // advice to define, and a demonstrative subject with "a … for", define nothing; "defined as" does
        let text = "They advise a proposal to name its worldview, define the basic ideas of that worldview, and say how it shaped the research. \
            Those three steps are a useful checklist for the methodology's opening section. A worldview is defined as a set of beliefs that guide action.";
        let Some((_, defs)) = found(text) else { return };
        assert_eq!(defs, ["as a set of beliefs that guide action"]);
        let text = "Each protocol is defined by which session, router, server or firewall is held out, and the sessions are described by their setting.";
        let Some((_, defs)) = found(text) else { return };
        assert_eq!(defs, ["which session, router, server or firewall is held out"]);
    }

    #[test]
    fn short_quotations_are_terms() {
        let text = "Students who swap \"positivism\" for \"quantitative\" lose marks, so a writer unsure of the terms should ask.";
        let Some(parser) = crate::text::tests::parser() else { return };
        let d = Doc::parse(text, &parser);
        let terms: Vec<String> = spans(&d).iter().filter(|v| v["k"] == "term").map(|v| v["v"].as_str().unwrap().to_string()).collect();
        assert_eq!(terms, ["positivism", "quantitative"]);
        assert!(!crate::rules::terms(&d).keys().any(|k| k.contains("unsure")), "{:?}", crate::rules::terms(&d).keys());
        // a cited author and the inside of "bona fide" are not terms
        let text = "Biggam puts the risk bluntly [@biggam2022, p. 143]: each volunteer gave bona fide answers to the survey.";
        let d = Doc::parse(text, &parser);
        let terms = crate::rules::terms(&d);
        assert!(terms.contains_key("bona fide answers") && !terms.keys().any(|k| k.starts_with("fide") || k.contains("biggam")), "{:?}", terms.keys());
    }

    /// Prints what a file's paragraphs give (AV_PARAS=<file>, one paragraph a line): `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn show_file() {
        let Ok(path) = std::env::var("AV_PARAS") else { return };
        for p in std::fs::read_to_string(path).unwrap().lines().filter(|l| !l.trim().is_empty()) {
            let p = p.trim_start_matches('\u{feff}');
            if let Some((q, d)) = found(p) {
                println!("QUOTES {q:#?}\nDEFINITIONS {d:#?}\n");
            }
            if std::env::var("AV_PARSE").is_ok() {
                let d = Doc::parse(p, &crate::text::tests::parser().unwrap());
                for (i, t) in d.toks.iter().enumerate() {
                    println!("{i:3} {:<14} {:<12} {:<6} {:<10} {:?}", t.form, t.lemma, t.upos, t.dep, t.head);
                }
            }
        }
    }
}

/// The paragraph's quotations and definitions as the protocol's spans (kinds "quote" and "definition").
pub fn spans(d: &Doc) -> Vec<serde_json::Value> {
    let quoted = quotes(d);
    let span = |k: &str, (s, e): (usize, usize)| {
        serde_json::json!({ "k": k, "t": &d.text[s..e], "w": 1.0, "v": d.visible(s, e), "at": [[d.utf16(s), d.utf16(e)]] })
    };
    let mut out: Vec<serde_json::Value> = quoted.iter().flat_map(|&(_, (s, e))| clean_parts(d, s, e)).map(|p| span("quote", p)).collect();
    out.extend(definitions(d, &quoted).into_iter().map(|p| span("definition", p)));
    // the name a naming sentence gives is a key term ("calibration"); rules::rank gives it the best term's score
    // and so is a word or two quoted to be talked about ("swap "positivism" for "quantitative"", ""recipe" the usual
    // word"): a quotation of up to three nouns and adjectives (not a quoted verdict such as "not conformant")
    let mentioned = quoted.iter().map(|&(_, q)| q).filter(|&(s, e)| {
        let toks: Vec<&crate::text::Tok> = d.toks.iter().filter(|t| t.s >= s && t.e <= e && t.alpha()).collect();
        (1..=3).contains(&toks.len()) && toks.iter().all(|t| ["NOUN", "PROPN", "ADJ", "X"].contains(&t.upos.as_str()))
    });
    out.extend(namings(d).into_iter().map(|(_, n)| n).chain(mentioned).flat_map(|n| clean_parts(d, n.0, n.1)).map(|p| {
        let mut v = span("term", p);
        v["named"] = serde_json::json!(true);
        v
    }));
    out
}
