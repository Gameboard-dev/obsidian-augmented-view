//! The floating key (docs/colour-codes.md): a small panel over the workspace showing the open note's colour code, every
//! colour as a chip with its name and what it marks. It is dragged by its title bar and folds to the title bar alone;
//! its place and fold are this device's (msg::Local). From it the note, or its folder, is linked to a code (or to
//! none), and a colour is added to the code. A note with no code shows the picker and a line saying nothing is sent to
//! the server until one is chosen (editor.rs and view.rs send nothing for it).
//!
//! The DOM: <https://developer.mozilla.org/en-US/docs/Web/API/Document/createElement>, pointer capture for the drag
//! (<https://developer.mozilla.org/en-US/docs/Web/API/Element/setPointerCapture>); the open note:
//! <https://docs.obsidian.md/Reference/TypeScript+API/Workspace/getActiveFile>.

use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use crate::app::{with, App};
use crate::colours::{folder_of, Assigned, Colour, Level};
use crate::js::{self, call, f0, f1, get, new, set, str};

/// The key's place, as CSS variables its stylesheet reads (styles.css, `.av-key.is-placed`).
fn place(root: &JsValue, x: f64, y: f64) {
    let style = get(root, "style");
    call!(style, setProperty, "--av-key-x", format!("{x}px"));
    call!(style, setProperty, "--av-key-y", format!("{y}px"));
    call!(get(root, "classList"), add, "is-placed");
}

fn doc() -> JsValue {
    get(&js::global(), "document")
}

fn el(tag: &str, class: &str, text: &str) -> JsValue {
    let e = call!(doc(), createElement, tag);
    set(&e, "className", class);
    if !text.is_empty() {
        set(&e, "textContent", text);
    }
    e
}

fn on(target: &JsValue, event: &str, f: impl FnMut(JsValue) + 'static) {
    let mut f = f;
    call!(target, addEventListener, event, f1(move |e| {
        f(e);
        JsValue::UNDEFINED
    }));
}

fn num(e: &JsValue, k: &str) -> f64 {
    get(e, k).as_f64().unwrap_or(0.0)
}

impl App {
    /// The note open in the active view, if it is a Markdown note.
    pub fn active_note(&self) -> Option<String> {
        let f = call!(get(&get(&self.plugin, "app"), "workspace"), getActiveFile);
        (!f.is_null() && !f.is_undefined() && str(&get(&f, "extension")) == "md").then(|| str(&get(&f, "path")))
    }

    /// The key's frame, made once: the title bar (title, code picker, fold button) and the body that
    /// [`App::refresh_key`] fills.
    pub fn make_key(&mut self) {
        let root = el("div", "av-key", "");
        let head = el("div", "av-key-head", "");
        let title = el("span", "av-key-title", "Colour code");
        let select = el("select", "av-key-code dropdown", "");
        let fold = el("span", "av-key-fold clickable-icon", "");
        set(&fold, "textContent", "\u{25be}");
        call!(fold, setAttribute, "aria-label", "Fold or unfold the key");
        for c in [&title, &select, &fold] {
            call!(head, appendChild, c.clone());
        }
        call!(root, appendChild, head.clone());
        call!(root, appendChild, el("div", "av-key-body", ""));
        if self.local.key_x >= 0.0 && self.local.key_y >= 0.0 {
            place(&root, self.local.key_x, self.local.key_y);
        }
        if self.local.key_folded {
            call!(get(&root, "classList"), add, "is-folded");
        }
        // resizable by its corner (user, 2026-10-27); the size the user leaves is kept on this device
        let style = get(&root, "style");
        if self.local.key_w > 0.0 {
            call!(style, setProperty, "width", format!("{}px", self.local.key_w));
        }
        if self.local.key_h > 0.0 {
            call!(style, setProperty, "height", format!("{}px", self.local.key_h));
        }
        // its text and colour boxes grow and shrink with it (user, 2026-10-27): a scale from its width, 250px = 1
        let scale = |r: &JsValue| {
            let w = num(r, "offsetWidth");
            if w > 0.0 {
                call!(get(r, "style"), setProperty, "--av-key-scale", format!("{:.3}", (w / 250.0).clamp(0.75, 3.0)));
            }
        };
        let r = root.clone();
        let observer = new(&get(&js::global(), "ResizeObserver"), &[f0(move || scale(&r)).into()]);
        call!(observer, observe, root.clone());
        let r = root.clone();
        on(&root, "pointerup", move |_| {
            // a folded key's height is the title bar's, not a size the user chose
            if call!(get(&r, "classList"), contains, "is-folded").is_truthy() {
                return;
            }
            let (w, h) = (num(&r, "offsetWidth"), num(&r, "offsetHeight"));
            with(|a| {
                if (a.local.key_w - w).abs() > 1.0 || (a.local.key_h - h).abs() > 1.0 {
                    (a.local.key_w, a.local.key_h) = (w, h);
                    a.save_local();
                }
            });
        });
        // Dragging by the title bar: the pointer is captured, so the moves come here even off the bar.
        let grab: Rc<Cell<Option<(f64, f64)>>> = Rc::new(Cell::new(None));
        let (g, r) = (grab.clone(), root.clone());
        on(&head, "pointerdown", move |e| {
            if call!(get(&e, "target"), closest, "select, .av-key-fold").is_truthy() {
                return;
            }
            let rect = call!(r, getBoundingClientRect);
            g.set(Some((num(&e, "clientX") - num(&rect, "left"), num(&e, "clientY") - num(&rect, "top"))));
            call!(get(&e, "currentTarget"), setPointerCapture, get(&e, "pointerId"));
        });
        let (g, r) = (grab.clone(), root.clone());
        on(&head, "pointermove", move |e| {
            let Some((dx, dy)) = g.get() else { return };
            let (w, h) = (num(&js::global(), "innerWidth"), num(&js::global(), "innerHeight"));
            let (x, y) = ((num(&e, "clientX") - dx).clamp(0.0, (w - 80.0).max(0.0)), (num(&e, "clientY") - dy).clamp(0.0, (h - 30.0).max(0.0)));
            place(&r, x, y);
        });
        let (g, r) = (grab, root.clone());
        on(&head, "pointerup", move |_| {
            if g.take().is_none() {
                return;
            }
            let rect = call!(r, getBoundingClientRect);
            let (x, y) = (num(&rect, "left"), num(&rect, "top"));
            with(|a| {
                (a.local.key_x, a.local.key_y) = (x, y);
                a.save_local();
            });
        });
        let r = root.clone();
        on(&fold, "click", move |_| {
            let folded = call!(get(&r, "classList"), toggle, "is-folded").is_truthy();
            with(|a| {
                a.local.key_folded = folded;
                a.save_local();
            });
        });
        on(&select, "change", move |e| {
            let value = str(&get(&get(&e, "target"), "value"));
            let folder = with(|a| a.key_folder).unwrap_or(false);
            let code = match value.as_str() {
                "" => return,
                "off" => String::new(),
                id => id.to_string(),
            };
            with(|a| if let Some(path) = a.active_note() { a.set_link(&path, &code, folder) });
        });
        call!(get(&doc(), "body"), appendChild, root.clone());
        self.key = root;
        self.refresh_key();
    }

    /// Fill the key for the open note: the picker's choices and the code's colours, or what to do when it has none.
    pub fn refresh_key(&mut self) {
        let root = self.key.clone();
        if root.is_undefined() {
            return;
        }
        let classes = get(&root, "classList");
        let Some(path) = self.active_note() else {
            call!(classes, add, "is-hidden");
            return;
        };
        call!(classes, remove, "is-hidden");
        let assigned = crate::colours::assigned(&self.settings.colour_codes, &self.settings.links, &path);
        let select = call!(root, querySelector, ".av-key-code");
        call!(select, empty); // Obsidian's Node.empty
        let option = |value: &str, label: &str, selected: bool| {
            let o = el("option", "", label);
            set(&o, "value", value);
            set(&o, "selected", selected);
            call!(select, appendChild, o);
        };
        if assigned == Assigned::Unset {
            option("", "Choose a colour code\u{2026}", true);
        }
        for (i, c) in self.settings.colour_codes.iter().enumerate() {
            option(&c.id, &c.name, assigned == Assigned::Code(i));
        }
        option("off", "None (not marked)", assigned == Assigned::Off);

        let body = call!(root, querySelector, ".av-key-body");
        call!(body, empty);
        let folder = folder_of(&path);
        let from = crate::colours::link_for(&self.settings.links, &path).map(|l| l.path.clone());
        let line = |text: &str| _ = call!(body, appendChild, el("div", "av-key-line", text));
        match (&assigned, &from) {
            (Assigned::Unset, _) => line("No colour code for this note. Choose one above: nothing is sent to the server until you do."),
            (_, Some(p)) if *p == path => line("Linked to this note."),
            (_, Some(p)) => line(&format!("From the folder {p}")),
            _ => {}
        }
        let label = el("label", "av-key-folder", "");
        let check = el("input", "", "");
        set(&check, "type", "checkbox");
        set(&check, "checked", self.key_folder);
        on(&check, "change", |e| _ = with(|a| a.key_folder = get(&get(&e, "target"), "checked").is_truthy()));
        call!(label, appendChild, check);
        let scope = if folder == "/" { "the whole vault".to_string() } else { format!("the folder {folder}") };
        call!(label, appendChild, call!(doc(), createTextNode, format!(" Choosing applies to {scope}")));
        call!(body, appendChild, label);
        let Assigned::Code(ci) = assigned else {
            if assigned == Assigned::Off {
                line("This note is not marked.");
            }
            return;
        };
        let chips = el("div", "av-key-chips", "");
        for (k, c) in self.settings.colour_codes[ci].colours.iter().enumerate() {
            let chip = el("div", if c.hidden { "av-key-chip is-hidden-colour" } else { "av-key-chip" }, "");
            // shown or hidden in the notes (user, 2026-10-28): this replaces the menu's and the settings' switches
            let shown = el("input", "av-key-show", "");
            set(&shown, "type", "checkbox");
            set(&shown, "checked", !c.hidden);
            call!(shown, setAttribute, "aria-label", format!("Show {} in the notes", c.name));
            on(&shown, "change", move |e| {
                let on = get(&get(&e, "target"), "checked").is_truthy();
                with(|a| a.edit_colours(|codes| if let Some(c) = codes.get_mut(ci).and_then(|code| code.colours.get_mut(k)) { c.hidden = !on }, false));
                js::timeout(0.0, || _ = with(App::refresh_key));
            });
            call!(chip, appendChild, shown);
            let swatch = el("span", if c.level == Level::Phrase { "av-key-swatch is-phrase" } else { "av-key-swatch" }, "");
            let (light, deep) = crate::colours::painted(c).unwrap_or_else(|| (c.hex.clone(), c.hex.clone()));
            call!(get(&swatch, "style"), setProperty, "--av-swatch", light);
            call!(get(&swatch, "style"), setProperty, "--av-swatch-dark", deep);
            call!(chip, appendChild, swatch);
            call!(chip, appendChild, el("span", "av-key-name", &c.name));
            let level = if c.level == Level::Phrase { "phrase" } else { "clause" };
            call!(chip, setAttribute, "title", format!("{} ({level}): {}", c.name, c.description));
            call!(chips, appendChild, chip);
        }
        call!(body, appendChild, chips);
        // A new colour: its colour and name; the rest (what it marks, clause or phrase, opacity) in the settings.
        let add = el("div", "av-key-add", "");
        let picker = el("input", "", "");
        set(&picker, "type", "color");
        set(&picker, "value", "#b3e5fc");
        let name = el("input", "", "");
        set(&name, "type", "text");
        set(&name, "placeholder", "New colour");
        let button = el("button", "", "+");
        call!(button, setAttribute, "aria-label", "Add this colour to the code");
        let (p, n) = (picker.clone(), name.clone());
        on(&button, "click", move |_| {
            let label = str(&get(&n, "value")).trim().to_string();
            if label.is_empty() {
                return;
            }
            let colour = Colour { name: label, hex: str(&get(&p, "value")), ..Colour::default() };
            with(|a| a.edit_colours(move |codes: &mut Vec<crate::colours::ColourCode>| codes[ci].colours.push(colour), true));
        });
        for c in [picker, name, button] {
            call!(add, appendChild, c);
        }
        call!(body, appendChild, add);
    }

    pub fn drop_key(&mut self) {
        if !self.key.is_undefined() {
            call!(self.key, remove);
            self.key = JsValue::UNDEFINED;
        }
    }
}
