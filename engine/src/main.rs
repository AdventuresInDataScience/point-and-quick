//! main.rs — window and loop, asset streaming, walking, audio, input and drawing.
//! The game rules live in logic.rs; the spec's data types and file reading in spec.rs.
//! Run:  cargo run --release -- path/to/game.zip
mod logic;
mod spec;

use kira::manager::{backend::DefaultBackend, AudioManager, AudioManagerSettings};
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle, StreamingSoundSettings};
use kira::sound::FromFileError;
use kira::track::{TrackBuilder, TrackHandle};
use kira::tween::Tween;
use kira::Volume;
use macroquad::prelude::*;
use spec::{Action, Header, Spec, State};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

const CELL: f32 = 4.0; // walk-grid cell size, in game pixels
const UI_H: f32 = 64.0; // height of the verb/inventory panel under the scene
const TEXT: f32 = 16.0; // speech text size

/// (scale, x offset, y offset): game pixels -> window pixels. The game is drawn at its own
/// resolution and letterboxed into whatever size the window is.
type View = (f32, f32, f32);

/// A loaded location: GPU textures (background, object sprites, walk-behind cut-outs), CPU
/// masks (object mask for hit-testing, depth for scaling) and the walk grid.
pub struct Room {
    pub tex: HashMap<String, Texture2D>,
    pub masks: HashMap<String, Image>,
    pub walk: Vec<bool>,
    pub gw: usize,
    pub gh: usize,
}

/// Everything the running game knows. Functions take &mut G and refer to things by ID string.
pub struct G {
    pub spec: Spec,
    pub st: State,                                // the save-able game state
    pub zip: String,
    pub key: Option<Vec<u8>>,
    pub queue: Vec<Action>,                       // actions waiting to run, in order
    pub speech: Option<(String, String, f32)>,    // (speaker, text, seconds left)
    pub menu: Option<(String, String)>,           // open conversation (character, node)
    pub wait: f32,
    pub cut_walk: bool,                           // a scripted walk is holding up the queue
    pub fade: f32,                                // 0 clear .. 1 black
    pub fade_edge: String,                        // map edge to take once faded out
    pub pos: HashMap<String, Vec2>,               // feet of each character in this room
    pub path: HashMap<String, Vec<Vec2>>,         // waypoints left per walking character
    pub dir: HashMap<String, usize>,              // facing: 0 e, 1 se, 2 s, 3 sw, 4 w, 5 nw, 6 n, 7 ne
    pub verb: String,
    pub item: String,                             // item picked for "use X with ..."
    pub arrive: Option<(String, String, String)>, // (verb, target, item) to do on reaching the target
    pub tex: HashMap<String, Texture2D>,          // character sheets and icons, loaded once
    pub rooms: HashMap<String, Room>,             // the current room and its map neighbours
    pub pending: Vec<String>,                     // rooms the loader thread is working on
    pub tx: Sender<(String, Vec<(String, String)>)>,
    pub rx: Receiver<(String, HashMap<String, Image>)>,
    pub audio: AudioManager<DefaultBackend>,
    pub music: Option<StreamingSoundHandle<FromFileError>>,
    pub music_file: String,
    pub music_bus: TrackHandle,
    pub sfx_bus: TrackHandle,
    pub volume: f64,
    pub t: f32,
    pub ended: bool,
}

// ---------------- small lookups ----------------

fn res(g: &G) -> (f32, f32) {
    let r = g.spec.game.resolution;
    if r[0] > 0.0 { (r[0], r[1]) } else { (640.0, 360.0) }
}

/// Player-facing text for a string ID.
pub fn text(g: &G, id: &str) -> String {
    g.spec.strings.get(id).cloned().unwrap_or(id.to_string())
}

/// Display name of an object or character.
fn name(g: &G, id: &str) -> String {
    let mut n = g.spec.objects.get(id).map(|o| o.name.clone()).unwrap_or_default();
    if let Some(c) = g.spec.characters.get(id) {
        n = c.name.clone();
    }
    if n.is_empty() { id.to_string() } else { text(g, &n) }
}

fn verb_label(g: &G, verb: &str) -> String {
    let key = format!("verb_{verb}");
    if g.spec.strings.contains_key(&key) { text(g, &key) } else { verb.to_string() }
}

fn hex_color(c: &str) -> Color {
    let n = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0xffffff);
    Color::from_rgba((n >> 16) as u8, (n >> 8) as u8, n as u8, 255)
}

pub fn walking(g: &G, id: &str) -> bool {
    g.path.get(id).map(|p| !p.is_empty()).unwrap_or(false)
}

pub fn mark_pos(g: &G, mark: &str) -> Vec2 {
    let m = g.spec.locations[&g.st.location].marks.get(mark).copied().unwrap_or([320.0, 300.0]);
    vec2(m[0], m[1])
}

fn facing_dir(f: &str) -> usize {
    match f {
        "e" | "right" => 0,
        "se" => 1,
        "sw" => 3,
        "w" | "left" => 4,
        "nw" => 5,
        "n" | "up" => 6,
        "ne" => 7,
        _ => 2, // s, down, or unset: face the viewer
    }
}

/// Movement direction -> one of the 8 facing rows (y points down, so +y is south).
fn dir_of(d: Vec2) -> usize {
    ((d.y.atan2(d.x).to_degrees() / 45.0).round() as i32).rem_euclid(8) as usize
}

/// The game key is compiled in:  GAME_KEY=<hex from the editor's export> cargo build --release
fn compiled_key() -> Vec<u8> {
    let hex = option_env!("GAME_KEY").expect("this game is encrypted: build the engine with GAME_KEY=<key from the editor>");
    let mut key = vec![];
    for i in (0..hex.len()).step_by(2) {
        key.push(u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
    }
    key
}

// ---------------- assets ----------------

/// Background thread: read, decrypt and decode a room's images, and cut walk-behinds out of
/// the background. Only the GPU upload is left for the main thread (see pump), because
/// macroquad's async loading doesn't really run in the background on desktop.
fn loader(zip: String, key: Option<Vec<u8>>, jobs: Receiver<(String, Vec<(String, String)>)>, done: Sender<(String, HashMap<String, Image>)>) {
    for (loc, files) in jobs {
        let mut imgs = HashMap::new();
        for (name, path) in files {
            let bytes = spec::read(&zip, &path, &key);
            imgs.insert(name, Image::from_file_with_format(&bytes, None).expect(&path));
        }
        let behinds: Vec<String> = imgs.keys().filter(|k| k.starts_with("behind")).cloned().collect();
        for name in behinds {
            // keep the background's pixels where the mask is white; transparent elsewhere
            let mut cut = imgs["bg"].clone();
            let mask = &imgs[&name];
            for i in 0..cut.bytes.len() / 4 {
                if mask.bytes[i * 4] < 128 {
                    cut.bytes[i * 4 + 3] = 0;
                }
            }
            imgs.insert(name, cut);
        }
        done.send((loc, imgs)).ok();
    }
}

/// The images a room needs, as (name, path): background, masks, walk-behinds, object sprites.
fn room_files(g: &G, loc: &str) -> Vec<(String, String)> {
    let l = &g.spec.locations[loc];
    let mut files = vec![("bg".to_string(), l.background.clone())];
    for (name, path) in [("walk", &l.walkway), ("objects", &l.objects_mask), ("depth", &l.depth)] {
        if !path.is_empty() {
            files.push((name.to_string(), path.clone()));
        }
    }
    for (i, b) in l.walkbehinds.iter().enumerate() {
        files.push((format!("behind{i}"), b.mask.clone()));
    }
    for id in l.hotspots.keys() {
        for sprite in g.spec.objects[id].states.values() {
            if !sprite.is_empty() {
                files.push((sprite.clone(), sprite.clone()));
            }
        }
    }
    files
}

/// Upload at most one finished room per frame, and bake its walk grid from the walkway mask.
fn pump(g: &mut G) {
    let Ok((loc, mut imgs)) = g.rx.try_recv() else { return };
    g.pending.retain(|p| *p != loc);
    let mut masks = HashMap::new();
    for name in ["walk", "objects", "depth"] {
        if let Some(img) = imgs.remove(name) {
            masks.insert(name.to_string(), img);
        }
    }
    let size = imgs.get("bg").map(|i| vec2(i.width as f32, i.height as f32)).unwrap_or(vec2(640.0, 360.0));
    let mut tex = HashMap::new();
    for (name, img) in imgs {
        let t = Texture2D::from_image(&img);
        t.set_filter(FilterMode::Nearest);
        tex.insert(name, t);
    }
    let (gw, gh) = ((size.x / CELL) as usize, (size.y / CELL) as usize);
    let mut walk = vec![true; gw * gh]; // no walkway mask: walk anywhere
    if let Some(m) = masks.get("walk") {
        for y in 0..gh {
            for x in 0..gw {
                let px = m.get_pixel((x as f32 * CELL + CELL / 2.0) as u32, (y as f32 * CELL + CELL / 2.0) as u32);
                walk[y * gw + x] = px.r > 0.5;
            }
        }
    }
    g.rooms.insert(loc, Room { tex, masks, walk, gw, gh });
}

/// Character sheets and inventory icons are small and needed everywhere: load them once.
fn load_globals(g: &mut G) {
    let mut paths = vec![];
    for c in g.spec.characters.values() {
        for a in c.animations.values() {
            paths.push(a.sheet.clone());
        }
    }
    for o in g.spec.objects.values() {
        if !o.icon.is_empty() {
            paths.push(o.icon.clone());
        }
    }
    for p in paths {
        let img = Image::from_file_with_format(&spec::read(&g.zip, &p, &g.key), None).expect(&p);
        let t = Texture2D::from_image(&img);
        t.set_filter(FilterMode::Nearest);
        g.tex.insert(p, t);
    }
}

/// After entering a room: keep it and its map neighbours loaded (asking the loader for any
/// that are missing), drop every other room, start its music and put the characters down.
pub fn room_changed(g: &mut G) {
    let here = g.st.location.clone();
    let mut need = vec![here.clone()];
    for e in &g.spec.edges {
        if e.from == here && !need.contains(&e.to) {
            need.push(e.to.clone());
        }
    }
    g.rooms.retain(|k, _| need.contains(k));
    for loc in need {
        if !g.rooms.contains_key(&loc) && !g.pending.contains(&loc) {
            let files = room_files(g, &loc);
            g.tx.send((loc.clone(), files)).ok();
            g.pending.push(loc);
        }
    }
    let music = if g.st.music.is_empty() { g.spec.locations[&here].music.clone() } else { g.st.music.clone() };
    play_music(g, &music, 1.0);
    place_all(g);
}

/// Put the player at their mark and every NPC here at theirs.
fn place_all(g: &mut G) {
    g.pos.clear();
    g.path.clear();
    let player = g.spec.game.player.clone();
    let p = mark_pos(g, &g.st.mark);
    g.pos.insert(player, p);
    for (c, mark, facing) in logic::npcs_here(g) {
        let p = mark_pos(g, &mark);
        g.pos.insert(c.clone(), p);
        g.dir.insert(c, facing_dir(&facing));
    }
}

/// Move an NPC (its state first). Staying in this room it walks; arriving it appears; leaving it vanishes.
pub fn move_char(g: &mut G, c: &str, location: &str, mark: &str) {
    let was_here = g.pos.contains_key(c);
    let p = g.st.characters.entry(c.to_string()).or_default();
    p.location = location.to_string();
    p.mark = mark.to_string();
    if location != g.st.location {
        g.pos.remove(c);
        g.path.remove(c);
        return;
    }
    let target = mark_pos(g, mark);
    if was_here { walk_to(g, c, target) } else { g.pos.insert(c.to_string(), target); }
}

// ---------------- walking ----------------

fn cell(room: &Room, p: Vec2) -> usize {
    let x = ((p.x / CELL).max(0.0) as usize).min(room.gw - 1);
    let y = ((p.y / CELL).max(0.0) as usize).min(room.gh - 1);
    y * room.gw + x
}

fn center(room: &Room, i: usize) -> Vec2 {
    vec2((i % room.gw) as f32 * CELL + CELL / 2.0, (i / room.gw) as f32 * CELL + CELL / 2.0)
}

fn clear_line(room: &Room, a: Vec2, b: Vec2) -> bool {
    let steps = (a.distance(b) / (CELL / 2.0)) as usize + 1;
    for s in 0..=steps {
        if !room.walk[cell(room, a.lerp(b, s as f32 / steps as f32))] {
            return false;
        }
    }
    true
}

/// A walk path on the grid: breadth-first search (8 neighbours) to the walkable cell nearest
/// the target, then string-pulling so characters walk in straight lines where they can.
fn find_path(room: &Room, from: Vec2, to: Vec2) -> Vec<Vec2> {
    let mut goal = cell(room, to);
    if !room.walk[goal] {
        let mut best = f32::MAX;
        for i in 0..room.walk.len() {
            let d = center(room, i).distance(to);
            if room.walk[i] && d < best {
                best = d;
                goal = i;
            }
        }
    }
    let start = cell(room, from);
    let mut prev = vec![usize::MAX; room.walk.len()]; // where each cell was reached from
    prev[start] = start;
    let mut frontier = vec![start];
    let mut head = 0;
    while head < frontier.len() && prev[goal] == usize::MAX {
        let i = frontier[head];
        head += 1;
        let (x, y) = ((i % room.gw) as i32, (i / room.gw) as i32);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= room.gw as i32 || ny >= room.gh as i32 {
                continue;
            }
            let n = ny as usize * room.gw + nx as usize;
            if room.walk[n] && prev[n] == usize::MAX {
                prev[n] = i;
                frontier.push(n);
            }
        }
    }
    if prev[goal] == usize::MAX {
        return vec![]; // can't get there: stay put
    }
    let mut cells = vec![goal];
    while *cells.last().unwrap() != start {
        let c = *cells.last().unwrap();
        cells.push(prev[c]);
    }
    cells.reverse();
    let mut pts: Vec<Vec2> = cells.iter().map(|c| center(room, *c)).collect();
    *pts.last_mut().unwrap() = if room.walk[cell(room, to)] { to } else { center(room, goal) };
    // string-pulling: from where we are, jump to the furthest point we can see in a straight line
    let mut out = vec![];
    let mut cur = from;
    let mut i = 0;
    while i < pts.len() {
        let mut j = pts.len() - 1;
        while j > i && !clear_line(room, cur, pts[j]) {
            j -= 1;
        }
        cur = pts[j];
        out.push(cur);
        i = j + 1;
    }
    out
}

pub fn walk_to(g: &mut G, who: &str, target: Vec2) {
    let Some(room) = g.rooms.get(&g.st.location) else { return };
    let from = g.pos.get(who).copied().unwrap_or(target);
    let path = find_path(room, from, target);
    g.path.insert(who.to_string(), path);
}

/// Move walkers along their paths; further away (darker depth) is slower.
fn step_walkers(g: &mut G, dt: f32) {
    let ids: Vec<String> = g.path.keys().cloned().collect();
    for id in ids {
        let Some(&target) = g.path[&id].first() else { continue };
        let pos = g.pos.get(&id).copied().unwrap_or(target);
        let speed = g.spec.characters.get(&id).map(|c| c.speed).unwrap_or(120.0) * depth_scale(g, pos);
        let d = target - pos;
        if d.length() <= speed * dt {
            g.pos.insert(id.clone(), target);
            g.path.get_mut(&id).unwrap().remove(0);
        } else {
            g.pos.insert(id.clone(), pos + d.normalize() * speed * dt);
            g.dir.insert(id.clone(), dir_of(d));
        }
    }
}

/// Character scale at a point: the depth mask's brightness, between the sheet's far and near scales.
fn depth_scale(g: &G, p: Vec2) -> f32 {
    let s = g.spec.locations[&g.st.location].scale;
    let v = match g.rooms.get(&g.st.location).and_then(|r| r.masks.get("depth")) {
        Some(img) => img.get_pixel((p.x.max(0.0) as u32).min(img.width as u32 - 1), (p.y.max(0.0) as u32).min(img.height as u32 - 1)).r,
        None => 1.0,
    };
    s[0] + (s[1] - s[0]) * v
}

// ---------------- hit-testing ----------------

/// The hotspot under a point: its colour in the object mask, if the object isn't gone.
fn object_at(g: &G, p: Vec2) -> String {
    let Some(img) = g.rooms.get(&g.st.location).and_then(|r| r.masks.get("objects")) else { return String::new() };
    if p.x < 0.0 || p.y < 0.0 || p.x >= img.width as f32 || p.y >= img.height as f32 {
        return String::new();
    }
    let i = (p.y as usize * img.width as usize + p.x as usize) * 4;
    let hex = format!("#{:02x}{:02x}{:02x}", img.bytes[i], img.bytes[i + 1], img.bytes[i + 2]);
    for (id, h) in &g.spec.locations[&g.st.location].hotspots {
        if h.color.to_lowercase() == hex && logic::active(g, id) {
            return id.clone();
        }
    }
    String::new()
}

/// Which sheet and frame to draw for a character now: walk, talk, its state's set, or idle.
/// Returns (texture, source rect in the sheet, mirror horizontally).
fn frame(g: &G, id: &str) -> Option<(Texture2D, Rect, bool)> {
    let c = g.spec.characters.get(id)?;
    let base = g.st.characters.get(id).map(|p| p.state.clone()).unwrap_or_default();
    let talking = g.speech.as_ref().map(|s| s.0 == id).unwrap_or(false);
    let want = if walking(g, id) { "walk".to_string() } else if talking { "talk".to_string() } else if !base.is_empty() { base } else { "idle".to_string() };
    let a = c.animations.get(&want).or(c.animations.get("idle"))?;
    let t = g.tex.get(&a.sheet)?.clone();
    let (fw, fh) = (t.width() / a.cols as f32, t.height() / a.rows as f32);
    let f = (g.t * a.fps) as u32 % a.cols;
    let d = g.dir.get(id).copied().unwrap_or(2);
    let (row, flip) = if a.rows >= 8 { (d as u32, false) } else { (0, (3..=5).contains(&d)) };
    Some((t, Rect::new(f as f32 * fw, row as f32 * fh, fw, fh), flip))
}

/// A character's on-screen box, from the feet up, at depth scale.
fn char_rect(g: &G, id: &str) -> Rect {
    let p = g.pos.get(id).copied().unwrap_or_default();
    let (w, h) = frame(g, id).map(|f| (f.1.w, f.1.h)).unwrap_or((32.0, 64.0));
    let s = depth_scale(g, p);
    Rect::new(p.x - w * s / 2.0, p.y - h * s, w * s, h * s)
}

/// What's under the mouse in the scene: an NPC first, then a hotspot.
fn target_at(g: &G, p: Vec2) -> String {
    for (c, _, _) in logic::npcs_here(g) {
        if g.pos.contains_key(&c) && char_rect(g, &c).contains(p) {
            return c;
        }
    }
    object_at(g, p)
}

// panel layout, in game pixels below the scene
fn verb_rect(g: &G, i: usize) -> Rect {
    Rect::new(8.0 + (i % 3) as f32 * 76.0, res(g).1 + 18.0 + (i / 3) as f32 * 20.0, 72.0, 18.0)
}
fn inv_rect(g: &G, i: usize) -> Rect {
    Rect::new(250.0 + i as f32 * 40.0, res(g).1 + 20.0, 36.0, 36.0)
}
fn option_rect(g: &G, i: usize, n: usize) -> Rect {
    let (w, h) = res(g);
    let top = (h + 4.0).min(h + UI_H - 4.0 - n as f32 * 15.0); // long menus grow up over the scene
    Rect::new(8.0, top + i as f32 * 15.0, w - 16.0, 15.0)
}

// ---------------- audio ----------------

/// Crossfade to a music track, streamed from the zip. "" stops the music.
pub fn play_music(g: &mut G, file: &str, fade: f32) {
    if file == g.music_file {
        return;
    }
    let tween = Tween { duration: Duration::from_secs_f32(fade.max(0.01)), ..Default::default() };
    if let Some(m) = g.music.as_mut() {
        m.stop(tween).ok(); // fades out while the new track fades in
    }
    g.music = None;
    g.music_file = file.to_string();
    if file.is_empty() {
        return;
    }
    let bytes = spec::read(&g.zip, file, &g.key);
    let settings = StreamingSoundSettings::new().loop_region(..).fade_in_tween(tween).output_destination(&g.music_bus);
    let data = StreamingSoundData::from_cursor(Cursor::new(bytes), settings).expect(file);
    g.music = g.audio.play(data).ok();
}

/// Play a sound effect or voice line, if the bundle has it.
pub fn play_sound(g: &mut G, file: &str) {
    if !g.spec.header.files.iter().any(|f| f == file) {
        return;
    }
    let bytes = spec::read(&g.zip, file, &g.key);
    let settings = StaticSoundSettings::new().output_destination(&g.sfx_bus);
    let data = StaticSoundData::from_cursor(Cursor::new(bytes), settings).expect(file);
    g.audio.play(data).ok();
}

// ---------------- input ----------------

fn reset_verb(g: &mut G) {
    g.verb = "walk".to_string();
    g.item.clear();
}

/// Mouse: left uses the selected verb, right looks. Panel clicks pick verbs and inventory items.
fn input(g: &mut G, mouse: Vec2) {
    let (left, right) = (is_mouse_button_pressed(MouseButton::Left), is_mouse_button_pressed(MouseButton::Right));
    if !(left || right) || g.ended {
        return;
    }
    if g.speech.is_some() {
        g.speech = None; // a click skips the current line
        return;
    }
    if logic::busy(g) || !g.queue.is_empty() {
        return; // a cutscene is playing
    }
    let player = g.spec.game.player.clone();
    let h = res(g).1;
    if g.menu.is_some() {
        let opts = logic::options(g);
        for (i, o) in opts.iter().enumerate() {
            if option_rect(g, i, opts.len()).contains(mouse) {
                logic::choose(g, o);
                return;
            }
        }
        return;
    }
    if mouse.y >= h {
        for (i, v) in g.spec.game.verbs.clone().iter().enumerate() {
            if verb_rect(g, i).contains(mouse) {
                g.verb = v.clone();
                g.item.clear();
                return;
            }
        }
        for (i, it) in g.st.inventory.clone().iter().enumerate() {
            if !inv_rect(g, i).contains(mouse) {
                continue;
            }
            let verb = if right { "look".to_string() } else { g.verb.clone() };
            if verb == "use" && g.item.is_empty() {
                g.item = it.clone(); // "Use Key with ..." now waits for a target
                return;
            }
            let item = g.item.clone();
            logic::interact(g, &verb, it, &item);
            reset_verb(g);
            return;
        }
        return;
    }
    let target = target_at(g, mouse);
    if target.is_empty() {
        walk_to(g, &player, mouse);
        g.arrive = None;
        return;
    }
    if right || g.verb == "look" {
        logic::interact(g, "look", &target, "");
        reset_verb(g);
        return;
    }
    // walk up to the target first: an object's hotspot mark, or beside a character
    let dest = match g.spec.locations[&g.st.location].hotspots.get(&target) {
        Some(hs) => mark_pos(g, &hs.mark),
        None => {
            let (c, p) = (g.pos[&target], g.pos[&player]);
            let side = if p.x < c.x { -36.0 } else { 36.0 };
            vec2(c.x + side * depth_scale(g, c), c.y)
        }
    };
    walk_to(g, &player, dest);
    g.arrive = Some((g.verb.clone(), target, g.item.clone()));
    reset_verb(g);
}

/// F5 save, F9 load, [ and ] music volume, Esc quit.
fn keys(g: &mut G) {
    if is_key_pressed(KeyCode::Escape) {
        std::process::exit(0);
    }
    if is_key_pressed(KeyCode::F5) && !logic::busy(g) && g.queue.is_empty() && g.menu.is_none() {
        spec::save(&g.st, &g.spec.game.id, &g.key);
        g.speech = Some((g.spec.game.player.clone(), "Saved.".to_string(), 1.0));
    }
    if is_key_pressed(KeyCode::F9) {
        if let Some(st) = spec::load_save(&g.spec, &g.key) {
            g.st = st;
            g.queue.clear();
            g.menu = None;
            g.speech = None;
            g.arrive = None;
            g.ended = false;
            room_changed(g);
        }
    }
    let step = if is_key_pressed(KeyCode::RightBracket) { 0.1 } else if is_key_pressed(KeyCode::LeftBracket) { -0.1 } else { 0.0 };
    if step != 0.0 {
        g.volume = (g.volume + step).clamp(0.0, 1.0);
        g.music_bus.set_volume(Volume::Amplitude(g.volume), Tween::default()).ok();
    }
}

// ---------------- update ----------------

/// Timers, walking, arrivals, the action queue, dialogue menus, room fades and the ending.
fn update(g: &mut G, dt: f32) {
    let player = g.spec.game.player.clone();
    let done = match g.speech.as_mut() {
        Some(s) => {
            s.2 -= dt;
            s.2 <= 0.0
        }
        None => false,
    };
    if done {
        g.speech = None;
    }
    g.wait = (g.wait - dt).max(0.0);
    step_walkers(g, dt);
    if g.cut_walk && !walking(g, &player) {
        g.cut_walk = false;
    }
    if !walking(g, &player) {
        if let Some((verb, target, item)) = g.arrive.take() {
            logic::arrive(g, &verb, &target, &item);
        }
    }
    logic::run_queue(g);
    logic::update_menu(g);
    // fade out, switch rooms at black, fade back in once the new room has loaded
    if !g.fade_edge.is_empty() {
        g.fade = (g.fade + dt * 3.0).min(1.0);
        if g.fade >= 1.0 {
            let e = std::mem::take(&mut g.fade_edge);
            logic::take_edge(g, &e);
        }
    } else if g.rooms.contains_key(&g.st.location) {
        g.fade = (g.fade - dt * 3.0).max(0.0);
    }
    let ending = g.spec.game.ending.clone();
    if !ending.is_empty() && g.st.flags.get(&ending) == Some(&true) && !logic::busy(g) && g.queue.is_empty() {
        g.ended = true;
    }
}

// ---------------- drawing ----------------

fn tex_at(t: &Texture2D, r: Rect, src: Option<Rect>, flip: bool, v: View) {
    let params = DrawTextureParams { dest_size: Some(vec2(r.w * v.0, r.h * v.0)), source: src, flip_x: flip, ..Default::default() };
    draw_texture_ex(t, v.1 + r.x * v.0, v.2 + r.y * v.0, WHITE, params);
}

fn box_at(r: Rect, c: Color, v: View) {
    draw_rectangle(v.1 + r.x * v.0, v.2 + r.y * v.0, r.w * v.0, r.h * v.0, c);
}

/// Text with a 1px shadow; y is the baseline.
fn text_at(s: &str, x: f32, y: f32, size: f32, c: Color, v: View) {
    draw_text(s, v.1 + (x + 1.0) * v.0, v.2 + (y + 1.0) * v.0, size * v.0, BLACK);
    draw_text(s, v.1 + x * v.0, v.2 + y * v.0, size * v.0, c);
}

fn text_w(s: &str, size: f32) -> f32 {
    measure_text(s, None, size as u16, 1.0).width
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let n = lines.last().unwrap().len();
        if n > 0 && n + word.len() + 1 > width {
            lines.push(String::new());
        }
        let last = lines.last_mut().unwrap();
        if !last.is_empty() {
            last.push(' ');
        }
        last.push_str(word);
    }
    lines
}

/// The room: background, then objects, characters and walk-behinds back to front by baseline.
fn draw_room(g: &G, room: &Room, v: View) {
    let (w, h) = res(g);
    let full = Rect::new(0.0, 0.0, w, h);
    tex_at(&room.tex["bg"], full, None, false, v);
    let loc = &g.spec.locations[&g.st.location];
    let mut layers: Vec<(f32, String, String)> = vec![]; // (baseline y, kind, texture or character)
    for (id, hs) in &loc.hotspots {
        let state = g.st.objects.get(id).cloned().unwrap_or_default();
        let sprite = g.spec.objects[id].states.get(&state).cloned().unwrap_or_default();
        if let Some(t) = room.tex.get(&sprite) {
            layers.push((hs.pos[1] + t.height(), format!("object {id}"), sprite));
        }
    }
    for (id, p) in &g.pos {
        layers.push((p.y, "character".to_string(), id.clone()));
    }
    for (i, b) in loc.walkbehinds.iter().enumerate() {
        layers.push((b.baseline, "behind".to_string(), format!("behind{i}")));
    }
    layers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (_, kind, id) in layers {
        if kind == "character" {
            if let Some((t, src, flip)) = frame(g, &id) {
                tex_at(&t, char_rect(g, &id), Some(src), flip, v);
            }
        } else if let Some(obj) = kind.strip_prefix("object ") {
            let (t, pos) = (&room.tex[&id], loc.hotspots[obj].pos);
            tex_at(t, Rect::new(pos[0], pos[1], t.width(), t.height()), None, false, v);
        } else {
            tex_at(&room.tex[&id], full, None, false, v);
        }
    }
}

/// The current line, wrapped, above the speaker's head in their colour.
fn draw_speech(g: &G, v: View) {
    let Some((who, line, _)) = &g.speech else { return };
    let w = res(g).0;
    let r = if g.pos.contains_key(who) { char_rect(g, who) } else { Rect::new(w / 2.0, 40.0, 0.0, 0.0) };
    let color = g.spec.characters.get(who).map(|c| hex_color(&c.text_color)).unwrap_or(WHITE);
    let lines = wrap(line, 42);
    for (i, l) in lines.iter().enumerate() {
        let tw = text_w(l, TEXT);
        let x = (r.x + r.w / 2.0 - tw / 2.0).clamp(4.0, (w - tw - 4.0).max(4.0));
        let y = (r.y - 6.0 - (lines.len() - 1 - i) as f32 * TEXT).max(TEXT);
        text_at(l, x, y, TEXT, color, v);
    }
}

/// The panel: dialogue options when a conversation is open, else the sentence line, verbs and inventory.
fn draw_panel(g: &G, mouse: Vec2, v: View) {
    let (w, h) = res(g);
    let bg = Color::from_rgba(24, 22, 34, 255);
    box_at(Rect::new(0.0, h, w, UI_H), bg, v);
    if g.menu.is_some() {
        if logic::busy(g) || !g.queue.is_empty() {
            return;
        }
        let opts = logic::options(g);
        for (i, o) in opts.iter().enumerate() {
            let r = option_rect(g, i, opts.len());
            box_at(r, bg, v);
            text_at(&text(g, &o.line), r.x, r.y + 12.0, 14.0, if r.contains(mouse) { YELLOW } else { WHITE }, v);
        }
        return;
    }
    let mut hover = if mouse.y < h { target_at(g, mouse) } else { String::new() };
    for (i, it) in g.st.inventory.iter().enumerate() {
        if inv_rect(g, i).contains(mouse) {
            hover = it.clone();
        }
    }
    let mut sentence = verb_label(g, &g.verb);
    if !g.item.is_empty() {
        sentence += &format!(" {} with", name(g, &g.item));
    }
    if !hover.is_empty() {
        sentence += &format!(" {}", name(g, &hover));
    }
    text_at(&sentence, 8.0, h + 13.0, 14.0, Color::from_rgba(150, 200, 255, 255), v);
    for (i, verb) in g.spec.game.verbs.iter().enumerate() {
        let r = verb_rect(g, i);
        let lit = *verb == g.verb || r.contains(mouse);
        text_at(&verb_label(g, verb), r.x, r.y + 13.0, 14.0, if lit { YELLOW } else { WHITE }, v);
    }
    for (i, it) in g.st.inventory.iter().enumerate() {
        let r = inv_rect(g, i);
        box_at(r, if *it == g.item { Color::from_rgba(90, 80, 40, 255) } else { Color::from_rgba(44, 42, 60, 255) }, v);
        let icon = g.spec.objects.get(it).map(|o| o.icon.clone()).unwrap_or_default();
        if let Some(t) = g.tex.get(&icon) {
            let s = (32.0 / t.width()).min(32.0 / t.height());
            let (iw, ih) = (t.width() * s, t.height() * s);
            tex_at(t, Rect::new(r.x + 18.0 - iw / 2.0, r.y + 18.0 - ih / 2.0, iw, ih), None, false, v);
        }
    }
}

fn draw(g: &G, mouse: Vec2, v: View) {
    let (w, h) = res(g);
    match g.rooms.get(&g.st.location) {
        Some(room) => draw_room(g, room, v),
        None => text_at("Loading...", w / 2.0 - 30.0, h / 2.0, TEXT, WHITE, v),
    }
    draw_speech(g, v);
    draw_panel(g, mouse, v);
    if g.fade > 0.0 {
        box_at(Rect::new(0.0, 0.0, w, h + UI_H), Color::new(0.0, 0.0, 0.0, g.fade), v);
    }
    if g.ended {
        box_at(Rect::new(0.0, 0.0, w, h + UI_H), Color::new(0.0, 0.0, 0.0, 0.6), v);
        let end = g.spec.strings.get("str_the_end").cloned().unwrap_or("The End".to_string());
        text_at(&end, w / 2.0 - text_w(&end, 32.0) / 2.0, h / 2.0, 32.0, WHITE, v);
    }
}

// ---------------- main ----------------

fn conf() -> Conf {
    Conf { window_title: "Point & Click".to_string(), window_width: 1280, window_height: 848, high_dpi: true, ..Default::default() }
}

#[macroquad::main(conf)]
async fn main() {
    let zip = std::env::args().nth(1).unwrap_or("game.zip".to_string());
    let header: Header = serde_json::from_slice(&spec::read(&zip, "header.json", &None)).expect("header.json");
    if header.spec_version != spec::SPEC_VERSION {
        panic!("this engine runs spec {}, the game is spec {}", spec::SPEC_VERSION, header.spec_version);
    }
    let key = if header.encrypted { Some(compiled_key()) } else { None };
    let spec = spec::load(&zip, &key, header);
    let st = spec::new_state(&spec);

    // asset loader thread: jobs go in as (room, files), decoded images come back
    let (tx, jobs) = channel();
    let (done, rx) = channel();
    let (zip2, key2) = (zip.clone(), key.clone());
    std::thread::spawn(move || loader(zip2, key2, jobs, done));

    // audio: separate music and sfx buses, so each has its own volume
    let mut audio = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()).expect("no audio device");
    let music_bus = audio.add_sub_track(TrackBuilder::new()).unwrap();
    let sfx_bus = audio.add_sub_track(TrackBuilder::new()).unwrap();

    let mut g = G {
        spec, st, zip, key,
        queue: vec![], speech: None, menu: None, wait: 0.0, cut_walk: false, fade: 1.0, fade_edge: String::new(),
        pos: HashMap::new(), path: HashMap::new(), dir: HashMap::new(),
        verb: "walk".to_string(), item: String::new(), arrive: None,
        tex: HashMap::new(), rooms: HashMap::new(), pending: vec![], tx, rx,
        audio, music: None, music_file: String::new(), music_bus, sfx_bus, volume: 1.0,
        t: 0.0, ended: false,
    };
    load_globals(&mut g);
    let (loc, mark) = (g.st.location.clone(), g.st.mark.clone());
    logic::enter(&mut g, &loc, &mark);

    loop {
        let dt = get_frame_time().min(0.1);
        g.t += dt;
        pump(&mut g);
        let (w, h) = res(&g);
        let s = (screen_width() / w).min(screen_height() / (h + UI_H));
        let v = (s, (screen_width() - w * s) / 2.0, (screen_height() - (h + UI_H) * s) / 2.0);
        let (mx, my) = mouse_position();
        let mouse = vec2((mx - v.1) / s, (my - v.2) / s);
        update(&mut g, dt);
        keys(&mut g);
        input(&mut g, mouse);
        clear_background(BLACK);
        draw(&g, mouse, v);
        next_frame().await;
    }
}
