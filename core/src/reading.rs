//! Reading view shows rendered text, not the note's source, so the chosen spans are found in a rendered block by their
//! visible text (the server's `v`: markup blanked, spaces collapsed). A long span that a rendered citation or link
//! interrupts is found by its first and last few words instead. The result is a list of segments, each with the kinds
//! over it as a bit mask, so overlapping kinds become one wrapper with several classes.

pub use view_marks::locate::{plain_words, segments};
use view_marks::locate::Norm;

use crate::pick::Span;

/// The segments to wrap in a rendered block: (start16, end16, mask of kinds), sorted, not overlapping. `found`: ranges
/// the worker found in the block itself, with their kinds (the user's own terms, bold::block_ranges; citations,
/// rules.rs); `rules`: the kinds that cut a
/// sentence's colour (pick::cut).
pub fn block_segments(block: &str, spans: &[&Span], chosen: &[(usize, bool)], found: &[(u32, u32, u8)], rules: u32) -> Vec<(u32, u32, u32)> {
    let b = Norm::new(block);
    let mut ranges: Vec<(u32, u32, u8)> = found.to_vec();
    for &(i, every) in chosen {
        let sp = spans[i];
        ranges.extend(b.find_all(&sp.v, every).into_iter().map(|(a, e)| (a, e, sp.k)));
    }
    crate::pick::tidy(&mut ranges);
    crate::pick::cut(&mut ranges, rules);
    segments(&ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pick::{MAIN, STRESS as PHRASE, TERM};

    fn sp(k: u8, v: &str) -> Span {
        Span { k, w: 1.0, at: vec![], v: v.into() }
    }

    #[test]
    fn overlapping_kinds_become_masked_segments() {
        let block = "Plain bold part and link text end.";
        let spans = [sp(MAIN, "Plain bold part and link text"), sp(TERM, "bold part"), sp(PHRASE, "part and link")];
        let got = block_segments(block, &spans.iter().collect::<Vec<_>>(), &[(0, false), (1, false), (2, false)], &[], 0);
        let m = |ks: &[u8]| ks.iter().fold(0u32, |m, k| m | (1 << k));
        assert_eq!(got, vec![(0, 6, m(&[MAIN])), (6, 11, m(&[MAIN, TERM])), (11, 15, m(&[MAIN, TERM, PHRASE])),
            (15, 24, m(&[MAIN, PHRASE])), (24, 29, m(&[MAIN]))]);
    }

    #[test]
    fn found_despite_spacing_case_and_an_interrupting_citation() {
        let block = "Oates  calls it\ninaccurate (Oates, 2006, p. 287) to equate positivism with quantitative research.";
        let got = block_segments(block, &[&sp(TERM, "Calls it inaccurate")], &[(0, false)], &[], 0);
        assert_eq!(got.len(), 1);
        assert_eq!(&block[got[0].0 as usize..got[0].1 as usize], "calls it\ninaccurate");
        let long = "Oates calls it inaccurate to equate positivism with quantitative research";
        let got = block_segments(block, &[&sp(MAIN, long)], &[(0, false)], &[], 0);
        assert_eq!(&block[got[0].0 as usize..got[0].1 as usize], &block[..block.len() - 1]);
    }

    #[test]
    fn every_occurrence_and_utf16() {
        let block = "😀 theory and theory again";
        let got = block_segments(block, &[&sp(TERM, "theory")], &[(0, true)], &[], 0);
        assert_eq!(got.iter().map(|g| (g.0, g.1)).collect::<Vec<_>>(), vec![(3, 9), (14, 20)]); // the emoji is 2 units
    }
}
