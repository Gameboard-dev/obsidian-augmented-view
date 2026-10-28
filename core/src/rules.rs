//! Built-in rules (COLOUR-CODES.md, "Rules"), found in the worker in each paragraph's text, never by the server, so
//! they show at once, also on cached paragraphs. So far: citations, Pandoc's bracketed `[@key, p. 3]` and `[@a; @b]`
//! and narrative `@key` (<https://pandoc.org/MANUAL.html#citation-syntax>), outside inline code. Reading view keeps a
//! citation's text (Zotero Cite Links only wraps its key in a link), so they are found in a rendered block the same way.

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
}
