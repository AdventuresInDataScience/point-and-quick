//! main.rs — window and loop, asset streaming, walking, audio, and drawing the room.
//! The game rules live in logic.rs, the interface (input, verbs, inventory, speech, dialogue
//! options) in ui.rs, and the spec's data types and file reading in spec.rs.
//! Run:  cargo run --release -- path/to/game.zip
mod logic;
mod spec;
#[cfg(test)]
mod tests;
mod ui;

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
const TEXT: f32 = 16.0; // speech text size

/// (scale, x offset, y offset): game pixels -> window pixels. The game is drawn at its own
/// resolution and letterboxed into whatever size the window is.
type View = (f32, f32, f32);

/// A loaded location: GPU textures (background, object sprites, walk-behind cut-outs), CPU
/// masks (object mask for hit-testing, depth for scaling), CPU copies of the object sprites
/// (clicks hit their visible pixels; they're small) and the walk grid.
pub struct Room {
    pub tex: HashMap<String, Texture2D>,
    pub masks: HashMap<String, Image>,
    pub sprites: HashMap<String, Image>,
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
    pub press: Option<ui::Press>,                 // ring style: a left press not yet let go
    pub inv_open: bool,                           // ring style: the inventory is showing
    pub pause: Option<ui::Pause>,                 // the menu (Esc) is open: the game is paused
    pub saves: std::path::PathBuf,                // this game's saves folder ("" in the tests: no saving)
    pub autosave: bool,                           // a room was entered: autosave once it has settled
    pub tex: HashMap<String, Texture2D>,          // character sheets and icons, loaded once
    pub rooms: HashMap<String, Room>,             // the current room and its map neighbours
    pub pending: Vec<String>,                     // rooms the loader thread is working on
    pub tx: Sender<(String, Vec<(String, String)>)>,
    pub rx: Receiver<(String, HashMap<String, Image>)>,
    pub audio: Option<Audio>,                     // None without a sound device (and in the tests)
    pub music_file: String,                       // the track that should be playing ("" = none)
    pub music_fade: f32,                          // its fade-in, in seconds
    pub volume: f64,
    pub t: f32,
    pub ended: bool,
}

/// Sound output: kira's manager, the playing music, and separate music and sfx buses so each has
/// its own volume. Files are read and decoded on a thread of their own (see audio_loader).
pub struct Audio {
    pub manager: AudioManager<DefaultBackend>,
    pub music: Option<StreamingSoundHandle<FromFileError>>,
    pub music_bus: TrackHandle,
    pub sfx_bus: TrackHandle,
    pub sounds: HashMap<String, StaticSoundData>, // decoded effects, kept for replays (voice lines aren't)
    pub tx: Sender<(String, bool)>,               // (file, is music) for the audio thread
    pub rx: Receiver<Loaded>,
}

/// What the audio thread sends back: a music file's bytes (kira decodes music as it streams), or
/// a fully decoded effect.
pub enum Loaded {
    Music(String, Vec<u8>),
    Effect(String, StaticSoundData),
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

// GAME_KEY: Option<(masked key, mask)>, written by build.rs from the GAME_KEY environment variable.
include!(concat!(env!("OUT_DIR"), "/key.rs"));

/// The game key is compiled in:  GAME_KEY=<hex from the editor's export> cargo build --release
fn compiled_key() -> Vec<u8> {
    let (masked, mask) = GAME_KEY.expect("this game is encrypted: build the engine with GAME_KEY=<key from the editor>");
    // black_box stops the compiler undoing the mask at build time and storing the plain key
    let mask = std::hint::black_box(mask);
    masked.iter().zip(mask.iter()).map(|(a, b)| a ^ b).collect()
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
    let (mut tex, mut sprites) = (HashMap::new(), HashMap::new());
    for (name, img) in imgs {
        let t = Texture2D::from_image(&img);
        t.set_filter(FilterMode::Nearest);
        if name != "bg" && !name.starts_with("behind") {
            sprites.insert(name.clone(), img); // object sprites are named by their path
        }
        tex.insert(name, t);
    }
    let (walk, gw, gh) = bake_walk(&masks, size);
    g.rooms.insert(loc, Room { tex, masks, sprites, walk, gw, gh });
}

/// The walk grid for a room of the given size: a cell is walkable where the walkway mask is
/// light at its centre. Returns (cells, width, height).
fn bake_walk(masks: &HashMap<String, Image>, size: Vec2) -> (Vec<bool>, usize, usize) {
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
    (walk, gw, gh)
}

/// Character sheets, inventory icons and verb icons are small and needed everywhere: load them
/// once. Sprites keep hard pixel edges; verb icons are smoothed, since they're scaled to fit.
fn load_globals(g: &mut G) {
    let mut paths = vec![];
    for c in g.spec.characters.values() {
        for a in c.animations.values() {
            paths.push((a.sheet.clone(), FilterMode::Nearest));
        }
    }
    for o in g.spec.objects.values() {
        if !o.icon.is_empty() {
            paths.push((o.icon.clone(), FilterMode::Nearest));
        }
    }
    for icon in g.spec.game.interface.icons.values() {
        paths.push((icon.clone(), FilterMode::Linear));
    }
    for (p, filter) in paths {
        let img = Image::from_file_with_format(&spec::read(&g.zip, &p, &g.key), None).expect(&p);
        let t = Texture2D::from_image(&img);
        t.set_filter(filter);
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
    sync_npcs(g);
}

/// Keep the characters on screen in step with logic::npcs_here, every frame: an NPC whose placement
/// condition stops holding leaves, and one whose condition starts holding appears at its mark.
/// check.py's solver treats placement conditions as live in the same way.
fn sync_npcs(g: &mut G) {
    let here = logic::npcs_here(g);
    let player = g.spec.game.player.clone();
    g.pos.retain(|c, _| *c == player || here.iter().any(|(h, _, _)| h == c));
    let pos = &g.pos;
    g.path.retain(|c, _| pos.contains_key(c));
    for (c, mark, facing) in here {
        if !g.pos.contains_key(&c) {
            let p = mark_pos(g, &mark);
            g.pos.insert(c.clone(), p);
            g.dir.insert(c, facing_dir(&facing));
        }
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
        if h.color.to_lowercase() == hex && logic::shown(g, id) {
            return id.clone();
        }
    }
    String::new()
}

/// Whether an object's sprite, drawn with its top-left at pos, has a visible pixel at p.
fn opaque(room: &Room, sprite: &str, pos: [f32; 2], p: Vec2) -> bool {
    let Some(img) = room.sprites.get(sprite) else { return false };
    let (x, y) = (p.x - pos[0], p.y - pos[1]);
    x >= 0.0 && y >= 0.0 && x < img.width as f32 && y < img.height as f32 && img.get_pixel(x as u32, y as u32).a > 0.5
}

/// What's drawn over the room's background, back to front, as (kind, id, sprite): kind is
/// "object", "character" or "behind" (a walk-behind, id behind<i>). The back layer comes first
/// (objects in their listed order), then the depth layer sorted by bottom edge (objects, characters
/// and walk-behinds; the listed order breaks ties), then the front layer (in listed order).
/// Objects that aren't here, or have no sprite in their state, aren't in it.
fn scene(g: &G, room: &Room) -> Vec<(&'static str, String, String)> {
    let loc = &g.spec.locations[&g.st.location];
    let mut items = vec![]; // (layer, bottom edge, listed order, kind, id, sprite)
    for (i, (id, hs)) in loc.hotspots.iter().enumerate() {
        let state = g.st.objects.get(id).cloned().unwrap_or_default();
        let sprite = g.spec.objects[id].states.get(&state).cloned().unwrap_or_default();
        let Some(img) = room.sprites.get(&sprite) else { continue };
        if logic::shown(g, id) {
            let layer = match hs.layer.as_str() { "back" => 0, "front" => 2, _ => 1 };
            items.push((layer, hs.pos[1] + img.height as f32, i, "object", id.clone(), sprite));
        }
    }
    let n = loc.hotspots.len(); // characters, then walk-behinds, after objects on a tie
    for (id, p) in &g.pos {
        items.push((1, p.y, n, "character", id.clone(), String::new()));
    }
    for (i, b) in loc.walkbehinds.iter().enumerate() {
        items.push((1, b.baseline, n + 1, "behind", format!("behind{i}"), String::new()));
    }
    items.sort_by(|a, b| {
        let depth = if a.0 == 1 && b.0 == 1 { a.1.total_cmp(&b.1) } else { std::cmp::Ordering::Equal };
        a.0.cmp(&b.0).then(depth).then(a.2.cmp(&b.2)).then(a.4.cmp(&b.4))
    });
    items.into_iter().map(|(_, _, _, kind, id, sprite)| (kind, id, sprite)).collect()
}

/// What's under a point in the scene. Front to back as drawn: a character by its box, an object
/// by its sprite's visible pixels or its colour in the object mask. After that, an object painted
/// into the background (one with no sprite drawn), by its colour. The player is never hit, and
/// neither is an object that isn't here.
fn target_at(g: &G, p: Vec2) -> String {
    let Some(room) = g.rooms.get(&g.st.location) else { return String::new() };
    let painted = object_at(g, p); // the object whose colour is under p, if it's here
    let loc = &g.spec.locations[&g.st.location];
    for (kind, id, sprite) in scene(g, room).into_iter().rev() {
        let hit = match kind {
            "character" => id != g.spec.game.player && char_rect(g, &id).contains(p),
            "object" => id == painted || opaque(room, &sprite, loc.hotspots[&id].pos, p),
            _ => false, // walk-behinds are scenery
        };
        if hit {
            return id;
        }
    }
    painted
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

// ---------------- audio ----------------

fn fade(secs: f32) -> Tween {
    Tween { duration: Duration::from_secs_f32(secs.max(0.01)), ..Default::default() }
}

/// Open the sound device and start the audio thread. Without a device the game runs silent.
fn start_audio(zip: &str, key: &Option<Vec<u8>>) -> Option<Audio> {
    let mut manager = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("no sound: {e}");
            return None;
        }
    };
    let music_bus = manager.add_sub_track(TrackBuilder::new()).ok()?;
    let sfx_bus = manager.add_sub_track(TrackBuilder::new()).ok()?;
    let (tx, jobs) = channel();
    let (done, rx) = channel();
    let (zip, key) = (zip.to_string(), key.clone());
    std::thread::spawn(move || audio_loader(zip, key, jobs, done));
    Some(Audio { manager, music: None, music_bus, sfx_bus, sounds: HashMap::new(), tx, rx })
}

/// Background thread: read and decrypt sound files, and decode effects in full, so the frame loop
/// never stalls on audio. Music goes back as bytes, since kira decodes it while it streams.
fn audio_loader(zip: String, key: Option<Vec<u8>>, jobs: Receiver<(String, bool)>, done: Sender<Loaded>) {
    for (file, music) in jobs {
        let bytes = spec::read(&zip, &file, &key);
        let loaded = if music {
            Loaded::Music(file, bytes)
        } else {
            match StaticSoundData::from_cursor(Cursor::new(bytes), StaticSoundSettings::default()) {
                Ok(data) => Loaded::Effect(file, data),
                Err(e) => {
                    eprintln!("can't decode {file}: {e}");
                    continue;
                }
            }
        };
        if done.send(loaded).is_err() {
            return;
        }
    }
}

/// Crossfade to a music track: the old one fades out now, and the new one fades in once the audio
/// thread has read it (see pump_audio). "" stops the music.
pub fn play_music(g: &mut G, file: &str, secs: f32) {
    if file == g.music_file {
        return;
    }
    g.music_file = file.to_string();
    g.music_fade = secs;
    let Some(a) = g.audio.as_mut() else { return };
    if let Some(m) = a.music.as_mut() {
        m.stop(fade(secs)).ok();
    }
    a.music = None;
    if logic::has(&g.spec.header.files, file) {
        a.tx.send((file.to_string(), true)).ok();
    }
}

/// Play a sound effect or voice line, if the bundle has it. An effect plays at once if it has been
/// decoded before; otherwise it plays as soon as the audio thread has decoded it.
pub fn play_sound(g: &mut G, file: &str) {
    let Some(a) = g.audio.as_mut() else { return };
    if let Some(data) = a.sounds.get(file) {
        a.manager.play(data.clone()).ok();
    } else if logic::has(&g.spec.header.files, file) {
        a.tx.send((file.to_string(), false)).ok();
    }
}

/// Play whatever the audio thread has finished: effects right away (and kept, except voice lines,
/// which are said once each), and music if it's still the track that should be playing.
fn pump_audio(g: &mut G) {
    let Some(a) = g.audio.as_mut() else { return };
    while let Ok(loaded) = a.rx.try_recv() {
        match loaded {
            Loaded::Effect(file, data) => {
                let data = data.with_modified_settings(|s| s.output_destination(&a.sfx_bus));
                a.manager.play(data.clone()).ok();
                if !file.starts_with("voice/") {
                    a.sounds.insert(file, data);
                }
            }
            Loaded::Music(file, bytes) => {
                if file != g.music_file {
                    continue; // the room changed again while it was loading
                }
                let settings = StreamingSoundSettings::new().loop_region(..).fade_in_tween(fade(g.music_fade)).output_destination(&a.music_bus);
                match StreamingSoundData::from_cursor(Cursor::new(bytes), settings) {
                    Ok(data) => a.music = a.manager.play(data).ok(),
                    Err(e) => eprintln!("can't play {file}: {e}"),
                }
            }
        }
    }
}

// ---------------- acting on clicks (ui.rs decides what was clicked) ----------------

fn reset_verb(g: &mut G) {
    g.verb = "walk".to_string();
    g.item.clear();
}

/// Use a verb on a target in the scene (with the picked item, if any). Looking happens on the spot;
/// anything else walks up to the target first (its hotspot mark, or beside a character), then acts.
fn click_target(g: &mut G, verb: &str, target: &str) {
    if verb == "look" {
        logic::interact(g, "look", target, "");
        reset_verb(g);
        return;
    }
    let player = g.spec.game.player.clone();
    let dest = match g.spec.locations[&g.st.location].hotspots.get(target) {
        Some(hs) => mark_pos(g, &hs.mark),
        None => {
            let (c, p) = (g.pos[target], g.pos[&player]);
            let side = if p.x < c.x { -36.0 } else { 36.0 };
            vec2(c.x + side * depth_scale(g, c), c.y)
        }
    };
    walk_to(g, &player, dest);
    g.arrive = Some((verb.to_string(), target.to_string(), g.item.clone()));
    reset_verb(g);
}

// ---------------- saving and loading ----------------

/// Saving waits until the game is waiting for the player: no line, cutscene, conversation or room
/// change under way. (A save holds the state, not what was in the middle of happening.)
pub fn can_save(g: &G) -> bool {
    !logic::busy(g) && g.queue.is_empty() && g.menu.is_none()
}

/// Save to a slot ("auto", "quick", "1".."6"). False if it couldn't be written (or, in the tests,
/// when there's no saves folder).
pub fn save_slot(g: &mut G, slot: &str) -> bool {
    if g.saves.as_os_str().is_empty() {
        return false;
    }
    match spec::write_save(&g.saves, slot, &g.st, &g.key) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("can't save to {}: {e}", g.saves.display());
            false
        }
    }
}

/// Load a slot, dropping everything that was under way (including a room change). False if the
/// slot is empty or unreadable.
pub fn load_slot(g: &mut G, slot: &str) -> bool {
    let Some((st, _)) = spec::read_save(&g.saves, slot, &g.spec, &g.key) else { return false };
    g.st = st;
    g.queue.clear();
    g.menu = None;
    g.speech = None;
    g.arrive = None;
    g.ended = false;
    g.fade_edge.clear();
    g.wait = 0.0;
    g.cut_walk = false;
    g.press = None;
    g.inv_open = false;
    g.pause = None;
    g.autosave = false;
    reset_verb(g);
    room_changed(g);
    true
}

/// A line from the engine itself (like "Saved."), said by the player.
fn notice(g: &mut G, text: String) {
    g.speech = Some((g.spec.game.player.clone(), text, 1.5));
}

/// Esc opens and closes the menu (or goes back a page in it). F5 quicksaves and F9 quickloads.
/// [ and ] change the music volume.
fn keys(g: &mut G) {
    if is_key_pressed(KeyCode::Escape) {
        ui::toggle_menu(g);
    }
    let step = if is_key_pressed(KeyCode::RightBracket) { 0.1 } else if is_key_pressed(KeyCode::LeftBracket) { -0.1 } else { 0.0 };
    if step != 0.0 {
        g.volume = (g.volume + step).clamp(0.0, 1.0);
        if let Some(a) = g.audio.as_mut() {
            a.music_bus.set_volume(Volume::Amplitude(g.volume), Tween::default()).ok();
        }
    }
    if g.pause.is_some() {
        return;
    }
    if is_key_pressed(KeyCode::F5) {
        let text = if !can_save(g) {
            ui::text_or(g, "ui_cant_save", "You can't save in the middle of something.")
        } else if save_slot(g, "quick") {
            ui::text_or(g, "ui_saved", "Saved.")
        } else {
            ui::text_or(g, "ui_save_failed", "The game couldn't be saved.")
        };
        notice(g, text);
    }
    if is_key_pressed(KeyCode::F9) && !load_slot(g, "quick") {
        let text = ui::text_or(g, "ui_no_quicksave", "There's no quicksave yet.");
        notice(g, text);
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
    sync_npcs(g);
    // fade out, switch rooms at black, fade back in once the new room has loaded
    if !g.fade_edge.is_empty() {
        g.fade = (g.fade + dt * 3.0).min(1.0);
        if g.fade >= 1.0 {
            let e = std::mem::take(&mut g.fade_edge);
            logic::take_edge(g, &e);
            g.autosave = true; // once the new room has settled
        }
    } else if g.rooms.contains_key(&g.st.location) {
        g.fade = (g.fade - dt * 3.0).max(0.0);
    }
    // autosave after entering a room, once its enter rule (any cutscene) has played out
    if g.autosave && can_save(g) {
        g.autosave = false;
        save_slot(g, "auto");
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

/// The room: its background, then everything in scene() back to front.
fn draw_room(g: &G, room: &Room, v: View) {
    let (w, h) = res(g);
    let full = Rect::new(0.0, 0.0, w, h);
    tex_at(&room.tex["bg"], full, None, false, v);
    let loc = &g.spec.locations[&g.st.location];
    for (kind, id, sprite) in scene(g, room) {
        match kind {
            "character" => {
                if let Some((t, src, flip)) = frame(g, &id) {
                    tex_at(&t, char_rect(g, &id), Some(src), flip, v);
                }
            }
            "object" => {
                if let Some(t) = room.tex.get(&sprite) {
                    let pos = loc.hotspots[&id].pos;
                    tex_at(t, Rect::new(pos[0], pos[1], t.width(), t.height()), None, false, v);
                }
            }
            _ => {
                if let Some(t) = room.tex.get(&id) {
                    tex_at(t, full, None, false, v);
                }
            }
        }
    }
}

/// The frame: the room, then the interface (see ui.rs), then any fade and the ending.
fn draw(g: &G, mouse: ui::Mouse, v: View) {
    let (w, h) = res(g);
    match g.rooms.get(&g.st.location) {
        Some(room) => draw_room(g, room, v),
        None => text_at("Loading...", w / 2.0 - 30.0, h / 2.0, TEXT, WHITE, v),
    }
    ui::draw_ui(g, mouse, v);
    let all = Rect::new(0.0, 0.0, w, h + ui::ui_h(g));
    if g.fade > 0.0 {
        box_at(all, Color::new(0.0, 0.0, 0.0, g.fade), v);
    }
    if g.ended {
        box_at(all, Color::new(0.0, 0.0, 0.0, 0.6), v);
        let end = g.spec.strings.get("str_the_end").cloned().unwrap_or("The End".to_string());
        text_at(&end, w / 2.0 - text_w(&end, 32.0) / 2.0, h / 2.0, 32.0, WHITE, v);
    }
    ui::draw_menu(g, mouse, v); // the menu button, or the open menu over everything
}

// ---------------- main ----------------

fn conf() -> Conf {
    Conf { window_title: "Point & Click".to_string(), window_width: 1280, window_height: 848, high_dpi: true, ..Default::default() }
}

/// Read the game zip and set up everything that doesn't need a window. `live` also starts the
/// asset loader thread and audio; the tests run without them.
fn boot(zip: &str, live: bool) -> G {
    let header: Header = serde_json::from_slice(&spec::read(zip, "header.json", &None)).expect("header.json");
    if header.spec_version != spec::SPEC_VERSION {
        panic!("this engine runs spec {}, the game is spec {}", spec::SPEC_VERSION, header.spec_version);
    }
    let key = if header.encrypted { Some(compiled_key()) } else { None };
    let spec = spec::load(zip, &key, header);
    let st = spec::new_state(&spec);

    // asset loader thread: jobs go in as (room, files), decoded images come back
    let (tx, jobs) = channel();
    let (done, rx) = channel();
    let (mut audio, mut saves) = (None, std::path::PathBuf::new());
    if live {
        let (zip2, key2) = (zip.to_string(), key.clone());
        std::thread::spawn(move || loader(zip2, key2, jobs, done));
        audio = start_audio(zip, &key);
        saves = spec::saves_dir(&spec.game.id);
        // a save from before save slots (./saves/<id>.sav, next to the game) becomes the quicksave
        let old = std::path::Path::new("saves").join(format!("{}.sav", spec.game.id));
        if old.is_file() && !saves.join("quick.sav").exists() && std::fs::create_dir_all(&saves).is_ok() {
            std::fs::copy(&old, saves.join("quick.sav")).ok();
        }
    }
    G {
        spec, st, zip: zip.to_string(), key,
        queue: vec![], speech: None, menu: None, wait: 0.0, cut_walk: false, fade: 1.0, fade_edge: String::new(),
        pos: HashMap::new(), path: HashMap::new(), dir: HashMap::new(),
        verb: "walk".to_string(), item: String::new(), arrive: None, press: None, inv_open: false,
        pause: None, saves, autosave: false,
        tex: HashMap::new(), rooms: HashMap::new(), pending: vec![], tx, rx,
        audio, music_file: String::new(), music_fade: 1.0, volume: 1.0,
        t: 0.0, ended: false,
    }
}

#[macroquad::main(conf)]
async fn main() {
    let zip = std::env::args().nth(1).unwrap_or("game.zip".to_string());
    let mut g = boot(&zip, true);
    load_globals(&mut g);
    let (loc, mark) = (g.st.location.clone(), g.st.mark.clone());
    logic::enter(&mut g, &loc, &mark);
    let mut auto = ui::Autoplay::from_env(); // a dev tool: a scripted mouse, and screenshots

    loop {
        let dt = get_frame_time().min(0.1);
        pump(&mut g);
        pump_audio(&mut g);
        let (w, h) = res(&g);
        let full_h = h + ui::ui_h(&g);
        let s = (screen_width() / w).min(screen_height() / full_h);
        let v = (s, (screen_width() - w * s) / 2.0, (screen_height() - full_h * s) / 2.0);
        let mouse = match auto.as_mut() {
            Some(a) => a.step(dt),
            None => ui::Mouse::read(v),
        };
        if g.pause.is_none() {
            g.t += dt; // the menu pauses the game: no time passes, nothing moves
            update(&mut g, dt);
        }
        keys(&mut g);
        ui::input(&mut g, mouse);
        clear_background(BLACK);
        draw(&g, mouse, v);
        if let Some(file) = auto.as_mut().and_then(|a| a.shot.take()) {
            get_screen_data().export_png(&file);
        }
        next_frame().await;
    }
}
