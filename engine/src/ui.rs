//! ui.rs — the interface: how the player gives commands, and everything on screen that isn't the
//! room. game.yaml's `interface` picks the style, and the style decides which other settings apply:
//!   bar   the classic verb bar and inventory under the scene. `labels` shows the verbs as words,
//!         icons, or icons with captions; the icon styles make the bar much shallower.
//!   ring  the scene fills the window. Click to walk; hold on something for a ring of verbs and let
//!         go on one; right-click for the inventory. Walk isn't on the ring: it's the plain click.
//! Both styles end in the same calls (click_target, logic::click_item, logic::choose), so they play
//! by the same rules. Spoken lines go at the top of the screen or over the speaker (`speech`), and
//! dialogue options sit at the bottom in either style.
use crate::{box_at, logic, name, res, text, text_at, text_w, verb_label, View, G};
use macroquad::prelude::*;
use std::f32::consts::PI;

const HOLD: f32 = 0.25; // seconds a press is held before the ring opens...
const DRAG: f32 = 6.0; // ...or how far it moves first, in game pixels
const SLOT_R: f32 = 13.0; // radius of one verb on the ring
const ICON: f32 = 24.0; // verb icons are drawn this size, in game pixels
const INV_COLS: usize = 8; // slots per row in the ring style's inventory
const LABEL: Color = Color::new(0.59, 0.78, 1.0, 1.0); // sentence and hover labels
const DIM: Color = Color::new(0.78, 0.76, 0.72, 1.0); // verbs that aren't lit

fn panel() -> Color {
    Color::from_rgba(24, 22, 34, 255)
}

fn slot() -> Color {
    Color::from_rgba(44, 42, 60, 255)
}

/// The mouse this frame, in game pixels.
#[derive(Clone, Copy, Default)]
pub struct Mouse {
    pub pos: Vec2,
    pub pressed: bool,  // left button went down this frame
    pub released: bool, // left button came up this frame
    pub right: bool,    // right button went down this frame
}

impl Mouse {
    /// The real mouse, through the view's transform.
    pub fn read(v: View) -> Mouse {
        let (x, y) = mouse_position();
        Mouse {
            pos: vec2((x - v.1) / v.0, (y - v.2) / v.0),
            pressed: is_mouse_button_pressed(MouseButton::Left),
            released: is_mouse_button_released(MouseButton::Left),
            right: is_mouse_button_pressed(MouseButton::Right),
        }
    }
}

/// A left press that hasn't been let go (ring style): where and when it began, what was under it
/// (a target in the room, an inventory item, or "" for bare ground), and whether the ring is open.
pub struct Press {
    pub at: Vec2,
    pub t: f32,
    pub target: String,
    pub item: bool,
    pub ring: bool,
}

fn ring_style(g: &G) -> bool {
    g.spec.game.interface.style == "ring"
}

/// How the bar shows verbs: "words", "icons" or "captioned" ("" for the ring, which has no bar).
fn labels(g: &G) -> &str {
    if ring_style(g) {
        return "";
    }
    match g.spec.game.interface.labels.as_str() {
        l @ ("icons" | "captioned") => l,
        _ => "words",
    }
}

// ---------------- layout ----------------

/// Height of the panel under the scene: none for the ring, and shallower for icons than words.
pub fn ui_h(g: &G) -> f32 {
    match labels(g) {
        "" => 0.0,
        "icons" => 34.0,
        "captioned" => 46.0,
        _ => 64.0,
    }
}

/// Verb i's button in the bar.
pub fn verb_rect(g: &G, i: usize) -> Rect {
    let h = res(g).1;
    match labels(g) {
        "icons" => Rect::new(6.0 + i as f32 * 30.0, h + 5.0, 28.0, 24.0),
        "captioned" => {
            let x: f32 = g.spec.game.verbs[..i].iter().map(|v| caption_w(g, v)).sum();
            Rect::new(6.0 + x, h + 4.0, caption_w(g, &g.spec.game.verbs[i]) - 2.0, 38.0)
        }
        _ => Rect::new(8.0 + (i % 3) as f32 * 76.0, h + 18.0 + (i / 3) as f32 * 20.0, 72.0, 18.0),
    }
}

/// A captioned verb's width: about its caption's, worked out without the font so layout and
/// hit-testing don't need a window.
fn caption_w(g: &G, verb: &str) -> f32 {
    (verb_label(g, verb).chars().count() as f32 * 5.5 + 8.0).max(30.0)
}

/// Inventory slot i: in the bar after the verbs, or in the ring style's inventory panel.
pub fn inv_rect(g: &G, i: usize) -> Rect {
    let h = res(g).1;
    match labels(g) {
        "" => {
            let p = inv_panel(g);
            Rect::new(p.x + 6.0 + (i % INV_COLS) as f32 * 40.0, p.y + 6.0 + (i / INV_COLS) as f32 * 40.0, 36.0, 36.0)
        }
        "words" => Rect::new(250.0 + i as f32 * 40.0, h + 20.0, 36.0, 36.0),
        _ => {
            let n = g.spec.game.verbs.len();
            let end = if n == 0 { 0.0 } else { verb_rect(g, n - 1).right() };
            let s = ui_h(g) - 6.0;
            Rect::new(end + 18.0 + i as f32 * (s + 4.0), h + 3.0, s, s)
        }
    }
}

/// The ring style's inventory: a panel in the middle of the scene, opened with a right-click.
pub fn inv_panel(g: &G) -> Rect {
    let (w, h) = res(g);
    let n = g.st.inventory.len().max(1);
    let (cols, rows) = (n.min(INV_COLS), n.div_ceil(INV_COLS));
    let (pw, ph) = (cols as f32 * 40.0 + 8.0, rows as f32 * 40.0 + 8.0);
    Rect::new((w - pw) / 2.0, (h - ph) / 2.0, pw, ph)
}

/// The inventory item under a point, or "".
fn item_at(g: &G, p: Vec2) -> String {
    if ring_style(g) && !g.inv_open {
        return String::new();
    }
    for (i, it) in g.st.inventory.iter().enumerate() {
        if inv_rect(g, i).contains(p) {
            return it.clone();
        }
    }
    String::new()
}

/// What the mouse is over: an inventory item, or a character or hotspot in the room.
fn hovered(g: &G, p: Vec2) -> String {
    if ring_style(g) && g.inv_open {
        return item_at(g, p);
    }
    if p.y < res(g).1 { crate::target_at(g, p) } else { item_at(g, p) }
}

/// Dialogue option i of n: bottom-aligned at the foot of the screen, growing up over the scene.
pub fn option_rect(g: &G, i: usize, n: usize) -> Rect {
    let (w, h) = res(g);
    let bottom = h + ui_h(g) - 4.0;
    Rect::new(8.0, bottom - (n - i) as f32 * 15.0, w - 16.0, 15.0)
}

/// The verbs on the ring: all of the game's except walk, which is the plain click.
pub fn ring_verbs(g: &G) -> Vec<String> {
    g.spec.game.verbs.iter().filter(|v| *v != "walk").cloned().collect()
}

/// The ring's radius: big enough that n verbs don't overlap.
fn ring_r(n: usize) -> f32 {
    (n as f32 * (2.0 * SLOT_R + 6.0) / (2.0 * PI)).max(28.0)
}

/// Where a ring opened at `at` is centred: moved in from the edges so all of it is on screen.
pub fn ring_centre(g: &G, at: Vec2) -> Vec2 {
    let (w, h) = res(g);
    let m = ring_r(ring_verbs(g).len()) + SLOT_R + 4.0;
    vec2(at.x.clamp(m, (w - m).max(m)), at.y.clamp(m, (h - m).max(m)))
}

/// The centre of verb i of n on a ring centred at c, clockwise from the top.
pub fn ring_slot(c: Vec2, i: usize, n: usize) -> Vec2 {
    let a = -PI / 2.0 + i as f32 * 2.0 * PI / n.max(1) as f32;
    c + vec2(a.cos(), a.sin()) * ring_r(n)
}

/// The verb on the ring under a point, or "".
fn ring_verb_at(g: &G, c: Vec2, p: Vec2) -> String {
    let verbs = ring_verbs(g);
    for (i, v) in verbs.iter().enumerate() {
        if ring_slot(c, i, verbs.len()).distance(p) <= SLOT_R + 3.0 {
            return v.clone();
        }
    }
    String::new()
}

// ---------------- input ----------------

/// This frame's mouse: the menu takes it while it's open (and the ☰ button opens it); otherwise
/// it's handled the way the game's style does it.
pub fn input(g: &mut G, m: Mouse) {
    if g.pause.is_some() {
        return input_menu(g, m);
    }
    if m.pressed && menu_button(g).contains(m.pos) {
        g.press = None;
        return toggle_menu(g);
    }
    if g.ended {
        return;
    }
    if ring_style(g) { input_ring(g, m) } else { input_bar(g, m) }
}

/// What a click means in either style before anything else. Returns true if the click was used
/// up: skipping the current line, waiting out a cutscene, or picking a dialogue option.
fn common(g: &mut G, m: &Mouse) -> bool {
    if g.speech.is_some() {
        g.speech = None;
        return true;
    }
    if logic::busy(g) || !g.queue.is_empty() {
        return true;
    }
    if g.menu.is_some() {
        let opts = logic::options(g);
        for (i, o) in opts.iter().enumerate() {
            if option_rect(g, i, opts.len()).contains(m.pos) {
                logic::choose(g, o);
                break;
            }
        }
        return true;
    }
    false
}

/// Bar: left-click uses the selected verb (walk unless another is picked), right-click looks.
/// Clicking a verb selects it; clicking an inventory item follows logic::click_item.
fn input_bar(g: &mut G, m: Mouse) {
    if !(m.pressed || m.right) || common(g, &m) {
        return;
    }
    if m.pos.y >= res(g).1 {
        for i in 0..g.spec.game.verbs.len() {
            if verb_rect(g, i).contains(m.pos) {
                g.verb = g.spec.game.verbs[i].clone();
                g.item.clear();
                return;
            }
        }
        let it = item_at(g, m.pos);
        if !it.is_empty() {
            let verb = if m.right { "look".to_string() } else { g.verb.clone() };
            logic::click_item(g, &verb, &it);
        }
        return;
    }
    let target = crate::target_at(g, m.pos);
    if target.is_empty() {
        let player = g.spec.game.player.clone();
        crate::walk_to(g, &player, m.pos);
        g.arrive = None;
        return;
    }
    let verb = if m.right { "look".to_string() } else { g.verb.clone() };
    crate::click_target(g, &verb, &target);
}

/// Ring: click to walk (or, with an item picked, to use it on what's clicked); hold on something
/// for its verbs and let go on one. Right-click opens and closes the inventory, or puts a picked
/// item back. In the inventory, a click picks an item up (or uses the picked one on it) and a
/// hold gives the item's own verbs; a click outside the panel puts the inventory away.
fn input_ring(g: &mut G, m: Mouse) {
    if (m.pressed || m.right) && common(g, &m) {
        g.press = None;
        return;
    }
    if logic::busy(g) || !g.queue.is_empty() || g.menu.is_some() {
        g.press = None;
        return;
    }
    if m.right {
        g.press = None;
        if g.item.is_empty() { g.inv_open = !g.inv_open } else { crate::reset_verb(g) }
        return;
    }
    if m.pressed {
        let (target, item) = if g.inv_open { (item_at(g, m.pos), true) } else { (crate::target_at(g, m.pos), false) };
        if g.inv_open && target.is_empty() {
            if !inv_panel(g).contains(m.pos) {
                g.inv_open = false;
            }
            return;
        }
        g.press = Some(Press { at: m.pos, t: g.t, target, item, ring: false });
    }
    // the ring opens once a press on something has lasted HOLD or moved DRAG (not with an item picked)
    let (picked, t) = (!g.item.is_empty(), g.t);
    if let Some(p) = g.press.as_mut() {
        if !p.target.is_empty() && !picked && (t - p.t >= HOLD || m.pos.distance(p.at) > DRAG) {
            p.ring = true;
        }
    }
    if !m.released {
        return;
    }
    let Some(p) = g.press.take() else { return };
    if p.ring {
        let verb = ring_verb_at(g, ring_centre(g, p.at), m.pos); // let go off the ring: nothing happens
        if verb.is_empty() {
        } else if p.item {
            logic::click_item(g, &verb, &p.target);
        } else {
            crate::click_target(g, &verb, &p.target);
        }
    } else if p.item {
        logic::click_item(g, "use", &p.target); // pick it up, or use the picked item on it
    } else if p.target.is_empty() {
        let player = g.spec.game.player.clone();
        crate::walk_to(g, &player, p.at);
        g.arrive = None;
    } else {
        crate::click_target(g, if picked { "use" } else { "walk" }, &p.target);
    }
}

// ---------------- drawing ----------------

/// Everything on screen that isn't the room.
pub fn draw_ui(g: &G, m: Mouse, v: View) {
    let (w, h) = res(g);
    if !ring_style(g) {
        box_at(Rect::new(0.0, h, w, ui_h(g)), panel(), v);
    }
    draw_speech(g, v);
    let cutscene = logic::busy(g) || !g.queue.is_empty();
    if g.menu.is_some() {
        if !cutscene {
            draw_options(g, m, v);
        }
        return;
    }
    if ring_style(g) { draw_ring_style(g, m, v, cutscene) } else { draw_bar(g, m, v) }
}

/// The current line in the speaker's colour: centred at the top of the screen like a subtitle,
/// or over the speaker's head.
fn draw_speech(g: &G, v: View) {
    let Some((who, line, _)) = &g.speech else { return };
    let (w, size) = (res(g).0, crate::TEXT);
    let color = g.spec.characters.get(who).map(|c| crate::hex_color(&c.text_color)).unwrap_or(WHITE);
    if g.spec.game.interface.speech == "over_speaker" {
        let r = if g.pos.contains_key(who) { crate::char_rect(g, who) } else { Rect::new(w / 2.0, 40.0, 0.0, 0.0) };
        let lines = crate::wrap(line, 42);
        for (i, l) in lines.iter().enumerate() {
            let tw = text_w(l, size);
            let x = (r.x + r.w / 2.0 - tw / 2.0).clamp(4.0, (w - tw - 4.0).max(4.0));
            let y = (r.y - 6.0 - (lines.len() - 1 - i) as f32 * size).max(size);
            text_at(l, x, y, size, color, v);
        }
        return;
    }
    let lines = crate::wrap(line, 64);
    let tw = lines.iter().map(|l| text_w(l, size)).fold(0.0, f32::max);
    box_at(Rect::new((w - tw) / 2.0 - 10.0, 6.0, tw + 20.0, lines.len() as f32 * size + 10.0), Color::new(0.0, 0.0, 0.0, 0.5), v);
    for (i, l) in lines.iter().enumerate() {
        text_at(l, (w - text_w(l, size)) / 2.0, 8.0 + (i + 1) as f32 * size, size, color, v);
    }
}

/// The open conversation's options at the bottom; the one under the mouse is lit.
fn draw_options(g: &G, m: Mouse, v: View) {
    let opts = logic::options(g);
    if opts.is_empty() {
        return;
    }
    let (w, h) = res(g);
    let top = option_rect(g, 0, opts.len()).y - 4.0;
    if top < h {
        box_at(Rect::new(0.0, top, w, h + ui_h(g) - top), Color::new(0.094, 0.086, 0.133, 0.88), v);
    }
    for (i, o) in opts.iter().enumerate() {
        let r = option_rect(g, i, opts.len());
        text_at(&text(g, &o.line), r.x, r.y + 12.0, 14.0, if r.contains(m.pos) { YELLOW } else { WHITE }, v);
    }
}

/// What a click would do, as a sentence: "Walk to Door", "Use Key with Door". The ring style has
/// no selected verb, so there it just names what's under the mouse.
fn sentence(g: &G, m: Mouse) -> String {
    let hover = hovered(g, m.pos);
    if !g.item.is_empty() && hover == g.item {
        return format!("{} {}", verb_label(g, "use"), name(g, &hover)); // clicking it uses it alone
    }
    let mut s = if !g.item.is_empty() {
        format!("{} {} with", verb_label(g, "use"), name(g, &g.item))
    } else if ring_style(g) {
        String::new()
    } else {
        verb_label(g, &g.verb)
    };
    if !hover.is_empty() {
        if !s.is_empty() {
            s.push(' ');
        }
        s += &name(g, &hover);
    }
    s
}

/// A short label just above the mouse, kept on screen.
fn cursor_label(g: &G, s: &str, p: Vec2, v: View) {
    if s.is_empty() {
        return;
    }
    let w = res(g).0;
    let tw = text_w(s, 14.0);
    text_at(s, (p.x - tw / 2.0).clamp(4.0, (w - tw - 4.0).max(4.0)), (p.y - 10.0).max(14.0), 14.0, LABEL, v);
}

/// Draw a texture as large as fits in r, keeping its shape, centred, tinted.
fn fit(t: &Texture2D, r: Rect, tint: Color, v: View) {
    let s = (r.w / t.width()).min(r.h / t.height());
    let (w, h) = (t.width() * s, t.height() * s);
    let params = DrawTextureParams { dest_size: Some(vec2(w * v.0, h * v.0)), ..Default::default() };
    draw_texture_ex(t, v.1 + (r.x + (r.w - w) / 2.0) * v.0, v.2 + (r.y + (r.h - h) / 2.0) * v.0, tint, params);
}

fn disc(c: Vec2, r: f32, color: Color, v: View) {
    draw_circle(v.1 + c.x * v.0, v.2 + c.y * v.0, r * v.0, color);
}

/// A verb's icon fitted in r, or its name when it has no icon.
fn verb_icon(g: &G, verb: &str, r: Rect, tint: Color, v: View) {
    match g.spec.game.interface.icons.get(verb).and_then(|f| g.tex.get(f)) {
        Some(t) => fit(t, r, tint, v),
        None => {
            let s = verb_label(g, verb);
            text_at(&s, r.x + (r.w - text_w(&s, 10.0)) / 2.0, r.y + r.h / 2.0 + 3.0, 10.0, tint, v);
        }
    }
}

/// Bar: verbs (words, icons or captioned icons), the inventory, and the sentence being built: in
/// the bar for words, by the cursor for icons (where hovering a bare icon names its verb).
fn draw_bar(g: &G, m: Mouse, v: View) {
    let h = res(g).1;
    let words = labels(g) == "words";
    let mut tip = String::new();
    for (i, verb) in g.spec.game.verbs.iter().enumerate() {
        let r = verb_rect(g, i);
        let (on, hover) = (*verb == g.verb, r.contains(m.pos));
        if words {
            text_at(&verb_label(g, verb), r.x, r.y + 13.0, 14.0, if on || hover { YELLOW } else { WHITE }, v);
            continue;
        }
        if hover {
            box_at(r, Color::from_rgba(52, 48, 74, 255), v);
            tip = verb_label(g, verb);
        }
        let tint = if on { YELLOW } else if hover { WHITE } else { DIM };
        let captioned = labels(g) == "captioned";
        if !captioned || g.spec.game.interface.icons.contains_key(verb) {
            verb_icon(g, verb, Rect::new(r.x + (r.w - ICON) / 2.0, r.y, ICON, ICON), tint, v);
        }
        if captioned {
            let s = verb_label(g, verb);
            text_at(&s, r.x + (r.w - text_w(&s, 10.0)) / 2.0, r.y + ICON + 10.0, 10.0, tint, v);
        }
    }
    for (i, it) in g.st.inventory.iter().enumerate() {
        draw_item(g, it, inv_rect(g, i), v);
    }
    let s = sentence(g, m);
    if words {
        text_at(&s, 8.0, h + 13.0, 14.0, LABEL, v);
    } else if !tip.is_empty() && labels(g) == "icons" {
        cursor_label(g, &tip, m.pos, v);
    } else if !hovered(g, m.pos).is_empty() || !g.item.is_empty() {
        cursor_label(g, &s, m.pos, v);
    }
}

/// An inventory slot with the item's icon, lit while the item is picked for "use with".
fn draw_item(g: &G, it: &str, r: Rect, v: View) {
    box_at(r, if it == g.item { Color::from_rgba(90, 80, 40, 255) } else { slot() }, v);
    let icon = g.spec.objects.get(it).map(|o| o.icon.clone()).unwrap_or_default();
    if let Some(t) = g.tex.get(&icon) {
        fit(t, Rect::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0), WHITE, v);
    }
}

/// Ring: the inventory if it's open, the ring while a press holds it open; otherwise a label
/// naming what's under the mouse, with the picked item's icon by the cursor.
fn draw_ring_style(g: &G, m: Mouse, v: View, cutscene: bool) {
    if g.inv_open {
        let p = inv_panel(g);
        box_at(p, Color::from_rgba(24, 22, 34, 235), v);
        draw_rectangle_lines(v.1 + p.x * v.0, v.2 + p.y * v.0, p.w * v.0, p.h * v.0, 2.0, Color::from_rgba(90, 84, 120, 255));
        if g.st.inventory.is_empty() {
            box_at(inv_rect(g, 0), slot(), v);
        }
        for (i, it) in g.st.inventory.iter().enumerate() {
            draw_item(g, it, inv_rect(g, i), v);
        }
    }
    if let Some(p) = g.press.as_ref().filter(|p| p.ring) {
        draw_ring(g, p, m, v);
        return;
    }
    if cutscene {
        return;
    }
    if !hovered(g, m.pos).is_empty() || !g.item.is_empty() {
        cursor_label(g, &sentence(g, m), m.pos, v);
    }
    if let Some(t) = g.spec.objects.get(&g.item).and_then(|o| g.tex.get(&o.icon)) {
        fit(t, Rect::new(m.pos.x + 6.0, m.pos.y + 6.0, 20.0, 20.0), WHITE, v);
    }
}

/// The verb ring around where the press began, with the verb under the mouse lit and what letting
/// go would do written under it ("Look at Door").
fn draw_ring(g: &G, p: &Press, m: Mouse, v: View) {
    let (w, h) = res(g);
    let verbs = ring_verbs(g);
    let (c, r) = (ring_centre(g, p.at), ring_r(verbs.len()));
    disc(c, r + SLOT_R + 5.0, Color::new(0.06, 0.05, 0.09, 0.55), v);
    let hot = ring_verb_at(g, c, m.pos);
    for (i, verb) in verbs.iter().enumerate() {
        let s = ring_slot(c, i, verbs.len());
        let lit = *verb == hot;
        disc(s, SLOT_R, if lit { Color::from_rgba(96, 84, 150, 255) } else { Color::from_rgba(40, 36, 58, 255) }, v);
        verb_icon(g, verb, Rect::new(s.x - 10.0, s.y - 10.0, 20.0, 20.0), if lit { WHITE } else { DIM }, v);
    }
    let what = if hot.is_empty() { name(g, &p.target) } else { format!("{} {}", verb_label(g, &hot), name(g, &p.target)) };
    let tw = text_w(&what, 14.0);
    let below = c.y + r + SLOT_R + 18.0;
    let y = if below <= h - 2.0 { below } else { c.y - r - SLOT_R - 8.0 };
    text_at(&what, (c.x - tw / 2.0).clamp(4.0, (w - tw - 4.0).max(4.0)), y, 14.0, LABEL, v);
}

// ---------------- the menu (Esc, or the ☰ button in the corner) ----------------

/// The open menu: which page is showing ("main", "save" or "load"), a note under it (like "Saved
/// to slot 2"), and the save slots as last read: (slot, Some((room name, saved at)) or None).
/// The game is paused while it's open.
pub struct Pause {
    pub page: &'static str,
    pub note: String,
    pub slots: Vec<(String, Option<(String, u64)>)>,
}

/// The numbered slots; there's also "auto" (written on entering a room) and "quick" (F5, F9).
const SLOTS: [&str; 6] = ["1", "2", "3", "4", "5", "6"];

/// Text the engine shows on its own account (ui_resume, ui_saved…): the game's string if it has
/// one, so it can be translated, else the English.
pub fn text_or(g: &G, id: &str, english: &str) -> String {
    g.spec.strings.get(id).cloned().unwrap_or_else(|| english.to_string())
}

/// Esc: open the menu; on one of its inner pages, go back; on its main page, close it.
pub fn toggle_menu(g: &mut G) {
    match g.pause.as_mut() {
        None => {
            g.press = None;
            g.pause = Some(Pause { page: "main", note: String::new(), slots: vec![] });
        }
        Some(p) if p.page != "main" => {
            p.page = "main";
            p.note.clear();
        }
        Some(_) => g.pause = None,
    }
}

/// The ☰ button that opens the menu, in the top-right corner of the scene.
pub fn menu_button(g: &G) -> Rect {
    Rect::new(res(g).0 - 22.0, 4.0, 18.0, 16.0)
}

fn room_name(g: &G, loc: &str) -> String {
    match g.spec.locations.get(loc) {
        Some(l) if !l.name.is_empty() => text(g, &l.name),
        _ => loc.to_string(),
    }
}

/// Read every slot's room and save time, for the menu to show.
fn refresh_slots(g: &mut G) {
    let slots = ["auto", "quick"].iter().chain(SLOTS.iter()).map(|s| {
        let info = crate::spec::read_save(&g.saves, s, &g.spec, &g.key).map(|(st, at)| (room_name(g, &st.location), at));
        (s.to_string(), info)
    });
    let slots = slots.collect();
    if let Some(p) = g.pause.as_mut() {
        p.slots = slots;
    }
}

fn slot_info<'a>(g: &'a G, slot: &str) -> Option<&'a (String, u64)> {
    g.pause.as_ref()?.slots.iter().find(|(s, _)| s == slot)?.1.as_ref()
}

/// The buttons on the menu page showing, top to bottom: (rect, action). Actions are "resume",
/// "save", "load", "quit", "back", "save:<slot>" and "load:<slot>".
pub fn menu_buttons(g: &G) -> Vec<(Rect, String)> {
    let Some(p) = &g.pause else { return vec![] };
    let actions: Vec<String> = match p.page {
        "save" => SLOTS.iter().map(|s| format!("save:{s}")).chain(["back".to_string()]).collect(),
        "load" => ["auto", "quick"].iter().chain(SLOTS.iter()).map(|s| format!("load:{s}")).chain(["back".to_string()]).collect(),
        _ => ["resume", "save", "load", "quit"].iter().map(|s| s.to_string()).collect(),
    };
    let (w, h) = (res(g).0, res(g).1 + ui_h(g));
    let (bw, bh, gap) = (if p.page == "main" { 170.0 } else { 320.0 }, 20.0, 4.0);
    let top = (h - actions.len() as f32 * (bh + gap)) / 2.0 + 8.0;
    actions.into_iter().enumerate().map(|(i, a)| (Rect::new((w - bw) / 2.0, top + i as f32 * (bh + gap), bw, bh), a)).collect()
}

/// Whether a menu action can be taken now: saving waits until the game is waiting for the player,
/// and an empty slot can't be loaded.
fn enabled(g: &G, action: &str) -> bool {
    match action.split_once(':') {
        Some(("load", slot)) => slot_info(g, slot).is_some(),
        Some(_) => crate::can_save(g),
        None => action != "save" || crate::can_save(g),
    }
}

/// Clicks in the menu. A right-click goes back a page, like Esc.
fn input_menu(g: &mut G, m: Mouse) {
    if m.right {
        return toggle_menu(g);
    }
    if !m.pressed {
        return;
    }
    let Some((_, action)) = menu_buttons(g).into_iter().find(|(r, _)| r.contains(m.pos)) else { return };
    if !enabled(g, &action) {
        return;
    }
    match action.split_once(':') {
        Some(("save", slot)) => {
            let saved = crate::save_slot(g, slot);
            refresh_slots(g);
            let note = if saved {
                format!("{} {slot}.", text_or(g, "ui_saved_to", "Saved to slot"))
            } else {
                text_or(g, "ui_save_failed", "The game couldn't be saved.")
            };
            if let Some(p) = g.pause.as_mut() {
                p.note = note;
            }
        }
        Some((_, slot)) => {
            crate::load_slot(g, slot); // closes the menu
        }
        None => match action.as_str() {
            "resume" => g.pause = None,
            "quit" => std::process::exit(0),
            "back" => toggle_menu(g),
            page => {
                refresh_slots(g);
                if let Some(p) = g.pause.as_mut() {
                    p.page = if page == "save" { "save" } else { "load" };
                    p.note.clear();
                }
            }
        },
    }
}

/// How long ago a save was made, roughly.
fn ago(at: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let s = now.saturating_sub(at);
    match s {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86399 => format!("{} h ago", s / 3600),
        86400..=172799 => "yesterday".to_string(),
        _ => format!("{} days ago", s / 86400),
    }
}

/// A menu button's text: (label, detail on the right, like "Street - 5 min ago").
fn menu_label(g: &G, action: &str) -> (String, String) {
    let Some((_, slot)) = action.split_once(':') else {
        let label = match action {
            "resume" => text_or(g, "ui_resume", "Resume"),
            "save" => text_or(g, "ui_save", "Save game"),
            "load" => text_or(g, "ui_load", "Load game"),
            "quit" => text_or(g, "ui_quit", "Quit"),
            _ => text_or(g, "ui_back", "Back"),
        };
        return (label, String::new());
    };
    let name = match slot {
        "auto" => text_or(g, "ui_autosave", "Autosave"),
        "quick" => text_or(g, "ui_quicksave", "Quicksave"),
        n => format!("{} {n}", text_or(g, "ui_slot", "Slot")),
    };
    let detail = match slot_info(g, slot) {
        Some((room, at)) => format!("{room} - {}", ago(*at)),
        None => text_or(g, "ui_empty", "empty"),
    };
    (name, detail)
}

/// While playing, the ☰ button; with the menu open, the menu over the dimmed, paused game.
pub fn draw_menu(g: &G, m: Mouse, v: View) {
    let Some(p) = &g.pause else {
        let r = menu_button(g);
        let lit = r.contains(m.pos);
        box_at(r, Color::new(0.0, 0.0, 0.0, if lit { 0.6 } else { 0.35 }), v);
        for i in 0..3 {
            box_at(Rect::new(r.x + 4.0, r.y + 4.0 + i as f32 * 3.5, r.w - 8.0, 1.5), if lit { WHITE } else { DIM }, v);
        }
        return;
    };
    let (w, h) = (res(g).0, res(g).1 + ui_h(g));
    box_at(Rect::new(0.0, 0.0, w, h), Color::new(0.0, 0.0, 0.0, 0.6), v);
    let buttons = menu_buttons(g);
    let (first, last) = (buttons[0].0, buttons[buttons.len() - 1].0);
    let frame = Rect::new(first.x - 12.0, first.y - 30.0, first.w + 24.0, last.bottom() - first.y + 42.0);
    box_at(frame, panel(), v);
    draw_rectangle_lines(v.1 + frame.x * v.0, v.2 + frame.y * v.0, frame.w * v.0, frame.h * v.0, 2.0, Color::from_rgba(90, 84, 120, 255));
    let title = match p.page {
        "save" => text_or(g, "ui_save_title", "Save game"),
        "load" => text_or(g, "ui_load_title", "Load game"),
        _ => text_or(g, "ui_paused", "Paused"),
    };
    text_at(&title, (w - text_w(&title, 16.0)) / 2.0, first.y - 10.0, 16.0, WHITE, v);
    for (r, action) in &buttons {
        let (on, hover) = (enabled(g, action), r.contains(m.pos));
        box_at(*r, if on && hover { Color::from_rgba(70, 64, 100, 255) } else { slot() }, v);
        let colour = if !on { Color::new(0.5, 0.5, 0.55, 1.0) } else if hover { YELLOW } else { WHITE };
        let (label, detail) = menu_label(g, action);
        text_at(&label, r.x + 8.0, r.y + 14.0, 14.0, colour, v);
        if !detail.is_empty() {
            text_at(&detail, r.right() - 8.0 - text_w(&detail, 12.0), r.y + 14.0, 12.0, if on { DIM } else { colour }, v);
        }
    }
    let note = if !p.note.is_empty() {
        p.note.clone()
    } else if p.page == "main" && !crate::can_save(g) {
        text_or(g, "ui_cant_save", "You can't save in the middle of something.")
    } else {
        String::new()
    };
    if !note.is_empty() {
        text_at(&note, (w - text_w(&note, 13.0)) / 2.0, frame.bottom() + 16.0, 13.0, LABEL, v);
    }
}

// ---------------- autoplay (a dev tool) ----------------

/// PNC_AUTOPLAY=<file> drives the mouse from a script instead of the real one, to take
/// screenshots and check the interface by eye. One command per line; "#" starts a comment:
///   move X Y   put the mouse at game pixel X, Y     down / up   press or let go the left button
///   click      down, then up on the next frame      right       right-click
///   wait S     let S seconds pass                   shot FILE   save a screenshot (PNG)
///   quit       close the game
pub struct Autoplay {
    lines: Vec<String>,
    wait: f32,
    pos: Vec2,
    down: bool,
    was_down: bool,
    pub shot: Option<String>,
}

impl Autoplay {
    pub fn from_env() -> Option<Autoplay> {
        let path = std::env::var("PNC_AUTOPLAY").ok()?;
        let script = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("PNC_AUTOPLAY: can't read {path}: {e}"));
        let lines = script.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect();
        Some(Autoplay { lines, wait: 0.0, pos: Vec2::ZERO, down: false, was_down: false, shot: None })
    }

    /// Run the script up to its next wait (a button change or a shot also ends the frame's
    /// commands), and return this frame's mouse.
    pub fn step(&mut self, dt: f32) -> Mouse {
        let mut right = false;
        self.wait -= dt;
        while self.wait <= 0.0 && self.shot.is_none() && !self.lines.is_empty() {
            let line = self.lines.remove(0);
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.as_slice() {
                ["move", x, y] => self.pos = vec2(x.parse().unwrap(), y.parse().unwrap()),
                ["down"] | ["up"] => {
                    self.down = w[0] == "down";
                    break;
                }
                ["click"] => {
                    self.down = true;
                    self.lines.insert(0, "up".to_string());
                    break;
                }
                ["right"] => {
                    right = true;
                    break;
                }
                ["wait", s] => self.wait = s.parse().unwrap(),
                ["shot", f] => self.shot = Some(f.to_string()),
                ["quit"] => std::process::exit(0),
                _ => panic!("PNC_AUTOPLAY: can't read '{line}'"),
            }
        }
        let m = Mouse { pos: self.pos, pressed: self.down && !self.was_down, released: !self.down && self.was_down, right };
        self.was_down = self.down;
        m
    }
}
