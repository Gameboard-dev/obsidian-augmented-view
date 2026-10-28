//! The settings tab's Rules section (docs/colour-codes.md, "Rules"; docs/colour-codes.md, "Rules"): a colour code chosen at its top, each of
//! its colours with its rules (Force or Forbid, words or a pattern, on or off), add and delete, and a box to test the
//! code's rules on a sentence. Each colour row of the colour codes above has a Rules button that opens this section on
//! that colour (scrolled to and flashed). The rules are kept with their colour in data.json and applied in the worker
//! (rules.rs, which documents the precedence); a change never asks the server.
//!
//! Obsidian's Setting components: <https://docs.obsidian.md/Reference/TypeScript+API/Setting> (addDropdown, addText,
//! addToggle, addTextArea, addButton, addExtraButton).

use wasm_bindgen::prelude::*;

use crate::app::{redraw_settings, with, App};
use crate::colours::{Action, ColourCode, Level, Rule, RuleKind};
use crate::js::{self, call, f0, f1, get, new, obj, set, str};

type Edit = Box<dyn FnOnce(&mut Vec<ColourCode>)>;

fn edit(f: Edit, layout: bool) {
    with(|a| a.edit_colours(f, layout));
}

/// The element id of a colour's row in this section, which a Rules button scrolls to.
fn anchor(ci: usize, k: usize) -> String {
    format!("av-rules-{ci}-{k}")
}

/// Open the section on a colour: the settings redrawn with its code chosen, then its row scrolled to and flashed.
pub fn show(ci: usize, k: usize) {
    with(|a| a.rules_code = ci);
    redraw_settings();
    js::timeout(30.0, move || {
        let el = call!(get(&js::global(), "document"), getElementById, anchor(ci, k));
        if el.is_null() || el.is_undefined() {
            return;
        }
        call!(el, scrollIntoView, obj! { block: "center" });
        call!(el, addClass, "av-flash");
        js::timeout(1600.0, move || _ = call!(el, removeClass, "av-flash"));
    });
}

/// The words under a rule: what it does, or why it is not applied.
fn status(rule: &Rule, level: Level) -> String {
    if let Err(e) = crate::rules::compile(rule) {
        return format!("Not applied. {e}");
    }
    if !rule.on {
        return "Off: kept, not applied.".into();
    }
    let what = match rule.kind {
        RuleKind::Words => "contains one of these words (whole words, any case, not in links, code or citations)",
        RuleKind::Pattern => "matches this pattern (regex-lite; (?i) for any case)",
    };
    match (level, rule.action) {
        (Level::Clause, Action::Force) => format!("A sentence that {what} takes this colour, whatever the model chose."),
        (Level::Clause, Action::Forbid) => format!("A sentence that {what} never takes this colour."),
        (Level::Phrase, Action::Force) => match rule.kind {
            RuleKind::Words => "These words are marked in this colour wherever they appear.".into(),
            RuleKind::Pattern => "What the pattern matches is marked in this colour (a whole paragraph's source is searched).".into(),
        },
        (Level::Phrase, Action::Forbid) => "The model's phrases of this colour that overlap a match are not drawn.".into(),
    }
}

fn placeholder(kind: RuleKind) -> &'static str {
    match kind {
        RuleKind::Words => "Words, separated by commas",
        RuleKind::Pattern => "A pattern, e.g. ^(First|Second), no\\b",
    }
}

/// The section, after the colour codes and their links.
pub fn section(a: &App, tab: &JsValue) {
    let el = get(tab, "containerEl");
    let setting = get(&a.obs, "Setting");
    let row = || new(&setting, std::slice::from_ref(&el));
    call!(call!(row(), setName, "Rules"), setHeading);
    call!(row(), setDesc, "Your own rules for a colour, applied before anything is drawn and never sent to the server. For each sentence: a colour whose Forbid rule matches it is never given to it; otherwise the first colour (in the code's order) whose Force rule matches gives the sentence its colour, whatever the model chose and at any sensitivity above zero; otherwise the model's colour stays. Quotations still win over a sentence's colour. A phrase colour's Force rules mark what they match; its Forbid rules drop the model's phrases of that colour that they overlap.");
    let codes = &a.settings.colour_codes;
    if codes.is_empty() {
        return;
    }
    let ci = a.rules_code.min(codes.len() - 1);
    let code = &codes[ci];
    let names: Vec<String> = codes.iter().map(|c| c.name.clone()).collect();
    call!(call!(row(), setName, "Colour code"), addDropdown, f1(move |d| {
        for (i, n) in names.iter().enumerate() {
            call!(d, addOption, i.to_string(), n.clone());
        }
        call!(call!(d, setValue, ci.to_string()), onChange, f1(|v| {
            let i: usize = str(&v).parse().unwrap_or(0);
            with(|a| a.rules_code = i);
            redraw_settings();
            JsValue::UNDEFINED
        }))
    }));
    for (k, c) in code.colours.iter().enumerate() {
        let s = row();
        let on = c.rules.iter().filter(|r| r.on).count();
        let level = c.level;
        let what = if level == Level::Clause { "a clause colour: its rules test each sentence" } else { "a phrase colour: its rules mark or drop phrases" };
        call!(call!(s, setName, c.name.clone()), setDesc, format!("{} rule{}, {on} on; {what}.", c.rules.len(), if c.rules.len() == 1 { "" } else { "s" }));
        let el = get(&s, "settingEl");
        set(&el, "id", anchor(ci, k));
        call!(el, addClass, "av-rules-colour");
        call!(s, addButton, f1(move |b| {
            call!(b, setButtonText, "Add a rule");
            call!(b, onClick, f0(move || edit(Box::new(move |codes| codes[ci].colours[k].rules.push(Rule::default())), true)))
        }));
        for (n, r) in c.rules.iter().enumerate() {
            let s = row();
            call!(get(&s, "settingEl"), addClass, "av-rule-row");
            call!(s, setDesc, status(r, level));
            let desc = get(&s, "descEl");
            let action = if r.action == Action::Forbid { "forbid" } else { "force" };
            call!(s, addDropdown, f1(move |d| {
                call!(call!(d, addOption, "force", "Force"), addOption, "forbid", "Forbid");
                call!(call!(d, setValue, action), onChange, f1(move |v| {
                    let act = if str(&v) == "forbid" { Action::Forbid } else { Action::Force };
                    edit(Box::new(move |codes| codes[ci].colours[k].rules[n].action = act), true);
                    JsValue::UNDEFINED
                }))
            }));
            let kind = if r.kind == RuleKind::Pattern { "pattern" } else { "words" };
            call!(s, addDropdown, f1(move |d| {
                call!(call!(d, addOption, "words", "Words"), addOption, "pattern", "Pattern");
                call!(call!(d, setValue, kind), onChange, f1(move |v| {
                    let kind = if str(&v) == "pattern" { RuleKind::Pattern } else { RuleKind::Words };
                    edit(Box::new(move |codes| codes[ci].colours[k].rules[n].kind = kind), true);
                    JsValue::UNDEFINED
                }))
            }));
            let (text, rule) = (r.text.clone(), r.clone());
            call!(s, addText, f1(move |t| {
                call!(call!(t, setValue, text.clone()), setPlaceholder, placeholder(rule.kind));
                let (desc, rule) = (desc.clone(), rule.clone());
                call!(t, onChange, f1(move |v| {
                    let text = str(&v);
                    // checked as typed; the rule is applied as soon as it is valid
                    set(&desc, "textContent", status(&Rule { text: text.clone(), ..rule.clone() }, level));
                    edit(Box::new(move |codes| codes[ci].colours[k].rules[n].text = text), false);
                    JsValue::UNDEFINED
                }))
            }));
            let on = r.on;
            call!(s, addToggle, f1(move |t| {
                call!(call!(t, setValue, on), setTooltip, "Apply this rule");
                call!(t, onChange, f1(move |v| {
                    let on = v.is_truthy();
                    edit(Box::new(move |codes| codes[ci].colours[k].rules[n].on = on), true);
                    JsValue::UNDEFINED
                }))
            }));
            call!(s, addExtraButton, f1(move |b| {
                call!(call!(b, setIcon, "trash"), setTooltip, "Delete this rule");
                call!(b, onClick, f0(move || edit(Box::new(move |codes| _ = codes[ci].colours[k].rules.remove(n)), true)))
            }));
        }
    }
    // the test: what the code's rules do to a sentence typed here
    let s = row();
    call!(s, setName, "Test on a sentence");
    call!(s, setDesc, crate::rules::explain(code, &a.rules_test));
    call!(get(&s, "settingEl"), addClass, "av-rule-test");
    let desc = get(&s, "descEl");
    let test = a.rules_test.clone();
    call!(s, addTextArea, f1(move |t| {
        call!(call!(t, setValue, test.clone()), setPlaceholder, "Type or paste a sentence");
        let desc = desc.clone();
        call!(t, onChange, f1(move |v| {
            let text = str(&v);
            let said = with(|a| {
                a.rules_test = text.clone();
                a.settings.colour_codes.get(ci).map(|c| crate::rules::explain(c, &text)).unwrap_or_default()
            });
            set(&desc, "textContent", said.unwrap_or_default());
            JsValue::UNDEFINED
        }))
    }));
}
