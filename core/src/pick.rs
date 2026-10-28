//! Which of a paragraph's ranked spans to draw at a given sensitivity. The server ranks; this only chooses how many of
//! each kind, so moving the slider never asks the server again:
//!   term    every named one (an acronym, a quoted word, the name a naming sentence gives: weight NAMED), then ranked
//!           ones up to one term in all per 20 words at full sensitivity (at least one), within 0.04 + 0.16 s of the
//!           paragraph's best ranked term
//!   stress  contrast words, in the model's order: about half its list at 50%, all of it at 100%
//!   main    the one main point, whenever the sensitivity is above zero (italic, as a critical point)
//!   point   none more below 25%, then one, two, three as the sensitivity rises (in the model's order), but no more than
//!           one per 80 words, so that a short paragraph is not all italic
//!   quote, definition   every one, whenever the sensitivity is above zero (found by rule, not ranked)
//!   colour  a sentence's colour (the server's /colours), when the model gave it a probability of at least 1 − s: every
//!           one at 100%, the surer half or so at 50% (README, "Colours")

use serde::{Deserialize, Serialize};

pub use crate::msg::Settings;

pub const TERM: u8 = 0;
pub const STRESS: u8 = 1; // contrast words
pub const POINT: u8 = 2;
pub const MAIN: u8 = 3;
pub const QUOTE: u8 = 4;
pub const DEFINITION: u8 = 5;
/// A citation, found in the worker (rules.rs), never by the server.
pub const CITATION: u8 = 6;
/// A sentence's colour: `COLOUR + i` for the colour code's colour `i`.
pub const COLOUR: u8 = 8;
pub const MAX_COLOURS: usize = 24;
/// Kinds fit a `u32` mask (reading.rs).
pub const KINDS: usize = COLOUR as usize + MAX_COLOURS;
/// The weight the server gives a named term, above any ranked term's score.
pub const NAMED: f32 = 2.0;
/// Words per critical point beyond the main one.
const WORDS_PER_POINT: u32 = 80;

/// One ranked span of a paragraph, as the server returns it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub k: u8,
    /// Score (terms), distance below the best point (points; main 0) or place in the model's list (contrast words).
    pub w: f32,
    /// [start, end) UTF-16 offsets of each occurrence in the paragraph's source text.
    pub at: Vec<(u32, u32)>,
    /// The visible text (markup blanked, spaces collapsed): what Reading view searches for.
    pub v: String,
}

/// The class the editor and Reading view put on a kind (colours.rs styles them).
pub fn class(k: u8) -> String {
    match k {
        TERM => "av-term".into(),
        STRESS => "av-stress".into(),
        POINT | MAIN => "av-point".into(), // the main point is styled as a point
        QUOTE => "av-quote".into(),
        DEFINITION => "av-definition".into(),
        CITATION => "av-citation".into(),
        _ => format!("av-c{}", k.saturating_sub(COLOUR)),
    }
}

/// The chosen spans: (index into `spans`, every occurrence or only the first), in no particular order.
pub fn choose(spans: &[&Span], words: u32, s: &Settings) -> Vec<(usize, bool)> {
    if !s.enabled || s.sensitivity <= 0.0 || spans.is_empty() {
        return Vec::new();
    }
    let sens = s.sensitivity as f64;
    let mut out = Vec::new();
    let ranked = |kind: u8, per: f64, every: bool, out: &mut Vec<(usize, bool)>| {
        let (named, of): (Vec<usize>, Vec<usize>) = (0..spans.len()).filter(|&i| spans[i].k == kind).partition(|&i| spans[i].w >= NAMED);
        let n_named = named.len();
        out.extend(named.into_iter().map(|i| (i, every)));
        let Some(best) = of.iter().map(|&i| spans[i].w).reduce(f32::max) else { return };
        let floor = best as f64 - (0.04 + 0.16 * sens);
        let count = (((sens * words as f64) / per).round() as usize).saturating_sub(n_named).max(1);
        out.extend(of.into_iter().take(count).filter(|&i| spans[i].w as f64 >= floor).map(|i| (i, every)));
    };
    if s.terms {
        ranked(TERM, 20.0, s.every_occurrence, &mut out);
    }
    if s.contrast {
        // the model's list, best first: about half of it at 50%, all of it at 100%, at least one above 0
        let of: Vec<usize> = (0..spans.len()).filter(|&i| spans[i].k == STRESS).collect();
        let n = ((sens * of.len() as f64).round() as usize).max(1);
        out.extend(of.into_iter().take(n).map(|i| (i, s.every_occurrence)));
    }
    let all = |kind: u8, out: &mut Vec<(usize, bool)>| out.extend((0..spans.len()).filter(|&i| spans[i].k == kind).map(|i| (i, false)));
    if s.quotes {
        all(QUOTE, &mut out);
    }
    if s.definitions {
        all(DEFINITION, &mut out);
    }
    if s.points {
        out.extend((0..spans.len()).filter(|&i| spans[i].k >= COLOUR && spans[i].w as f64 >= 1.0 - sens - 1e-6).map(|i| (i, false)));
        all(MAIN, &mut out);
        let mut pts: Vec<usize> = (0..spans.len()).filter(|&i| spans[i].k == POINT).collect();
        pts.sort_by(|&a, &b| spans[a].w.total_cmp(&spans[b].w));
        let n = ((sens * 3.0 + 0.25).floor() as usize).min((words / WORDS_PER_POINT) as usize);
        out.extend(pts.into_iter().take(n).map(|i| (i, false)));
    }
    out
}

/// Keep ranges that do not overlap an earlier one of the same kind; sort by start, longer first.
pub fn tidy(ranges: &mut Vec<(u32, u32, u8)>) {
    ranges.sort_by_key(|&(a, e, k)| (k, a, std::cmp::Reverse(e)));
    let mut end = [0u32; KINDS];
    let mut started = [false; KINDS];
    ranges.retain(|r| (r.2 as usize) < KINDS);
    ranges.retain(|&(a, e, k)| {
        let k = k as usize;
        let ok = e > a && (!started[k] || a >= end[k]);
        if ok {
            started[k] = true;
            end[k] = e;
        }
        ok
    });
    ranges.sort_by_key(|&(a, e, _)| (a, std::cmp::Reverse(e)));
}

/// A rule wins over the model: a sentence's colour is cut where a range of a kind in `rules` (a mask of kinds, those
/// a clause colour of the note's code shows: a quotation, say) lies, so the two never blend.
pub fn cut(ranges: &mut Vec<(u32, u32, u8)>, rules: u32) {
    let over: Vec<(u32, u32)> = ranges.iter().filter(|r| r.2 < COLOUR && rules & (1 << r.2) != 0).map(|r| (r.0, r.1)).collect();
    if over.is_empty() {
        return;
    }
    let mut out = Vec::with_capacity(ranges.len());
    for &(a, e, k) in ranges.iter() {
        if k < COLOUR {
            out.push((a, e, k));
            continue;
        }
        let mut pos = a;
        let mut inside: Vec<(u32, u32)> = over.iter().copied().filter(|&(s, t)| s < e && a < t).collect();
        inside.sort_unstable();
        for (s, t) in inside {
            if s > pos {
                out.push((pos, s, k));
            }
            pos = pos.max(t);
        }
        if pos < e {
            out.push((pos, e, k));
        }
    }
    *ranges = out;
    tidy(ranges);
}

/// The ranges to mark in a paragraph's source text (offsets relative to the paragraph).
pub fn source_ranges(spans: &[&Span], words: u32, len16: u32, s: &Settings) -> Vec<(u32, u32, u8)> {
    let mut out: Vec<(u32, u32, u8)> = Vec::new();
    for (i, every) in choose(spans, words, s) {
        let sp = spans[i];
        let n = if every { sp.at.len() } else { sp.at.len().min(1) };
        out.extend(sp.at[..n].iter().filter(|&&(_, e)| e <= len16).map(|&(a, e)| (a, e, sp.k)));
    }
    tidy(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(k: u8, w: f32, a: u32) -> Span {
        Span { k, w, at: vec![(a, a + 1)], v: String::new() }
    }

    fn spans() -> Vec<Span> {
        let mut v: Vec<Span> = (0..12).map(|i| span(TERM, 0.8 - i as f32 * 0.01, i * 4)).collect();
        v.extend((0..6).map(|i| span(STRESS, i as f32, 60 + i * 4)));
        v.push(Span { k: MAIN, w: 0.0, at: vec![(100, 120)], v: String::new() });
        v.extend([0.0, 1.5, 3.0].iter().enumerate().map(|(i, &w)| Span { k: POINT, w, at: vec![(130 + i as u32 * 10, 135 + i as u32 * 10)], v: String::new() }));
        v
    }

    fn count_in(sens: f32, k: u8, words: u32, v: &[Span]) -> usize {
        let s = Settings { sensitivity: sens, ..Default::default() };
        let refs: Vec<&Span> = v.iter().collect();
        source_ranges(&refs, words, 1000, &s).iter().filter(|r| r.2 == k).count()
    }

    fn count(sens: f32, k: u8) -> usize {
        count_in(sens, k, 100, &spans())
    }

    #[test]
    fn counts_rise_with_sensitivity() {
        assert_eq!([0.2, 0.5, 1.0].map(|s| count(s, TERM)), [1, 3, 5]);
        assert_eq!([0.2, 0.5, 1.0].map(|s| count(s, STRESS)), [1, 3, 6]);
        assert_eq!([0.2, 0.5, 1.0].map(|s| count(s, MAIN)), [1, 1, 1]);
        assert_eq!([0.2, 0.4, 0.6, 1.0].map(|s| count_in(s, POINT, 300, &spans())), [0, 1, 2, 3]);
        assert_eq!([0.6, 1.0].map(|s| count_in(s, POINT, 100, &spans())), [1, 1]); // one per 80 words
        assert_eq!(count_in(1.0, POINT, 70, &spans()), 0);
        assert_eq!(count(0.0, MAIN), 0);
        // named terms are drawn whatever the count, and do not set the floor for the ranked ones
        let mut v = spans();
        v.extend((0..4).map(|i| span(TERM, NAMED, 200 + i * 4)));
        assert_eq!([0.2, 1.0].map(|s| count_in(s, TERM, 100, &v)), [5, 5]); // the four named, then the rest of the five
    }

    #[test]
    fn off_kinds_never_drawn_and_sorted() {
        let s = Settings { terms: false, points: false, ..Default::default() };
        let v = spans();
        let refs: Vec<&Span> = v.iter().collect();
        let r = source_ranges(&refs, 100, 1000, &s);
        assert!(r.iter().all(|x| x.2 != TERM && x.2 != POINT && x.2 != MAIN));
        assert!(r.windows(2).all(|w| w[0].0 <= w[1].0));
    }
}
