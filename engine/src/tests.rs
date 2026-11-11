//! Parity tests, engine side: play every scenario in tests/parity/scenarios.yaml through the
//! engine's own logic and check the state it ends in. editor/tests/test_parity.py plays the same
//! scenarios through check.py's solver, so the two can't drift apart unnoticed.
//!
//! Moves go through the same functions ui::input uses (click_item, click_target, choose), and time
//! passes through the real update(). Only the window-side parts are stubbed: rooms "load" with no
//! textures and a fully walkable grid, and there's no audio.
//!
//! The interface tests further down play the demo through ui::input itself, with a scripted mouse
//! over the rooms' real masks, once per interface style.
use super::*;
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    name: String,
    project: String,
    moves: Vec<String>,
    #[serde(default)]
    expect: Expect,
    #[serde(default)]
    blocked: Vec<String>, // moves that mustn't be possible once the moves are made
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Expect {
    location: Option<String>,
    inventory: Option<Vec<String>>,
    flags: HashMap<String, bool>,
    objects: HashMap<String, String>,
    characters: HashMap<String, String>, // character -> location
}

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/parity")
}

/// Zip a project folder the way the editor's dev export does: header.json plus every file, stored.
/// Each bundle gets its own file, since tests run in parallel and delete theirs when done.
fn bundle(project: &Path, name: &str) -> String {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let mut files = vec![];
    collect(project, project, &mut files);
    files.sort();
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let out = std::env::temp_dir().join(format!("pnc-parity-{name}-{}-{n}.zip", std::process::id()));
    let mut z = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let header = serde_json::json!({"spec_version": spec::SPEC_VERSION, "game": name, "encrypted": false, "files": files});
    z.start_file("header.json", opts).unwrap();
    z.write_all(header.to_string().as_bytes()).unwrap();
    for f in &files {
        z.start_file(f.as_str(), opts).unwrap();
        z.write_all(&std::fs::read(project.join(f)).unwrap()).unwrap();
    }
    z.finish().unwrap();
    out.to_string_lossy().to_string()
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.file_name().unwrap().to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            collect(root, &p, out);
        } else {
            out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

/// A room that counts as loaded without a GPU: no textures, and every cell walkable.
fn stub_room() -> Room {
    Room { tex: HashMap::new(), masks: HashMap::new(), sprites: HashMap::new(), walk: vec![true; 160 * 90], gw: 160, gh: 90 }
}

/// Run frames until the game waits for the player: lines said, walks and fades done, queue empty,
/// and either no conversation or one showing its options.
fn settle(g: &mut G) {
    for _ in 0..10_000 {
        let here = g.st.location.clone();
        g.rooms.entry(here).or_insert_with(stub_room);
        update(g, 0.25);
        let idle = !logic::busy(g) && g.queue.is_empty() && g.arrive.is_none() && g.fade_edge.is_empty();
        if idle && (g.menu.is_none() || !logic::options(g).is_empty()) {
            return;
        }
    }
    panic!("the game never settled");
}

/// The target is somewhere the player could click: an NPC standing here, or an object that's here.
/// (That it has a sprite or a painted colour to click on is the Check's job.)
fn clickable(g: &G, target: &str) -> bool {
    let npc = g.pos.contains_key(target) && logic::npcs_here(g).iter().any(|(c, _, _)| c == target);
    npc || logic::shown(g, target)
}

fn carried(g: &G, item: &str) -> bool {
    logic::has(&g.st.inventory, item)
}

/// Whether a move (in the solver's label format, see scenarios.yaml) could be made right now.
/// In a conversation, only its options can.
fn possible(g: &G, mv: &str) -> bool {
    if let Some(line) = mv.strip_prefix("say '").and_then(|m| m.strip_suffix('\'')) {
        return logic::options(g).iter().any(|o| o.line == line);
    }
    if g.menu.is_some() {
        return false;
    }
    if let Some(rest) = mv.strip_suffix(" (inventory)") {
        return carried(g, rest.split_once(' ').unwrap().1);
    }
    if let Some((item, target)) = mv.strip_prefix("use ").and_then(|m| m.split_once(" on ")) {
        return carried(g, item) && (carried(g, target) || clickable(g, target));
    }
    clickable(g, mv.split_once(' ').unwrap().1)
}

/// One move, in the solver's label format, made the way a player would.
fn play(g: &mut G, mv: &str) {
    assert!(possible(g, mv), "'{mv}' isn't possible in {}", g.st.location);
    if let Some(line) = mv.strip_prefix("say '").and_then(|m| m.strip_suffix('\'')) {
        let o = logic::options(g).into_iter().find(|o| o.line == line).unwrap();
        logic::choose(g, &o);
    } else if let Some(rest) = mv.strip_suffix(" (inventory)") {
        let (verb, it) = rest.split_once(' ').unwrap();
        g.verb = verb.to_string();
        logic::click_item(g, verb, it);
        if verb == "use" {
            logic::click_item(g, verb, it); // picked it; clicking it again uses it on its own
        }
    } else if let Some((item, target)) = mv.strip_prefix("use ").and_then(|m| m.split_once(" on ")) {
        g.verb = "use".to_string();
        logic::click_item(g, "use", item); // pick the item, then click what to use it on
        if carried(g, target) {
            logic::click_item(g, "use", target);
        } else {
            click_target(g, "use", target);
        }
    } else {
        let (verb, target) = mv.split_once(' ').unwrap();
        g.verb = verb.to_string();
        click_target(g, verb, target);
    }
    settle(g);
}

fn assert_state(g: &G, e: &Expect) {
    if let Some(l) = &e.location {
        assert_eq!(&g.st.location, l, "location");
    }
    if let Some(inv) = &e.inventory {
        let (mut a, mut b) = (g.st.inventory.clone(), inv.clone());
        a.sort();
        b.sort();
        assert_eq!(a, b, "inventory");
    }
    for (f, v) in &e.flags {
        assert_eq!(g.st.flags.get(f).copied().unwrap_or(false), *v, "flag {f}");
    }
    for (o, v) in &e.objects {
        assert_eq!(g.st.objects.get(o), Some(v), "object {o}");
    }
    for (c, l) in &e.characters {
        assert_eq!(g.st.characters.get(c).map(|p| p.location.clone()).unwrap_or_default(), *l, "character {c}");
    }
}

#[test]
fn parity_scenarios() {
    let text = std::fs::read_to_string(parity_dir().join("scenarios.yaml")).unwrap();
    let scenarios: Vec<Scenario> = serde_yaml::from_str(&text).unwrap();
    for s in &scenarios {
        eprintln!("scenario {}", s.name); // shown with the failure, so it's clear which one broke
        let zip = bundle(&parity_dir().join(&s.project), &s.name);
        let mut g = boot(&zip, false);
        let (loc, mark) = (g.st.location.clone(), g.st.mark.clone());
        logic::enter(&mut g, &loc, &mark);
        settle(&mut g);
        for mv in &s.moves {
            eprintln!("  {mv}");
            play(&mut g, mv);
        }
        assert_state(&g, &s.expect);
        for mv in &s.blocked {
            assert!(!possible(&g, mv), "'{mv}' should be blocked");
        }
        std::fs::remove_file(zip).ok();
    }
}

// ---------------- the interface styles, played through ui::input with a scripted mouse ----------------

const KEY: Vec2 = Vec2::new(255.0, 300.0); // inside the key's hotspot in the demo's street
const DOOR: Vec2 = Vec2::new(425.0, 220.0); // inside the door's

/// A room with its real masks and object sprites (decoded on the CPU; no textures), so clicks hit
/// what a player's would.
fn real_room(g: &G, loc: &str) -> Room {
    let l = &g.spec.locations[loc];
    let decode = |path: &str| Image::from_file_with_format(&spec::read(&g.zip, path, &g.key), None).unwrap();
    let mut masks = HashMap::new();
    for (name, path) in [("walk", &l.walkway), ("objects", &l.objects_mask), ("depth", &l.depth)] {
        if !path.is_empty() {
            masks.insert(name.to_string(), decode(path));
        }
    }
    let mut sprites = HashMap::new();
    for id in l.hotspots.keys() {
        for sprite in g.spec.objects[id].states.values().filter(|s| !s.is_empty()) {
            sprites.insert(sprite.clone(), decode(sprite));
        }
    }
    let (walk, gw, gh) = bake_walk(&masks, vec2(res(g).0, res(g).1));
    Room { tex: HashMap::new(), masks, sprites, walk, gw, gh }
}

/// The demo in the given interface style, waiting in the street. (Delete g.zip when done.)
fn demo(style: &str, labels: &str) -> G {
    let zip = bundle(&parity_dir().join("../../editor/examples/demo"), &format!("ui-{style}-{labels}"));
    let mut g = boot(&zip, false);
    g.spec.game.interface.style = style.to_string();
    g.spec.game.interface.labels = labels.to_string();
    for loc in ["street", "house"] {
        let room = real_room(&g, loc);
        g.rooms.insert(loc.to_string(), room);
    }
    let (loc, mark) = (g.st.location.clone(), g.st.mark.clone());
    logic::enter(&mut g, &loc, &mark);
    settle(&mut g);
    g
}

/// One frame: the mouse, then the frame's update, a thirtieth of a second on.
fn frame(g: &mut G, m: ui::Mouse) {
    ui::input(g, m);
    if g.pause.is_none() {
        g.t += 1.0 / 30.0; // like the main loop: the menu pauses the game
        update(g, 1.0 / 30.0);
    }
}

fn at(p: Vec2) -> ui::Mouse {
    ui::Mouse { pos: p, ..Default::default() }
}

fn click(g: &mut G, p: Vec2) {
    frame(g, ui::Mouse { pressed: true, ..at(p) });
    frame(g, ui::Mouse { released: true, ..at(p) });
}

/// Ring style: press on p, hold until the ring opens, move onto the verb and let go.
fn ring_pick(g: &mut G, p: Vec2, verb: &str) {
    frame(g, ui::Mouse { pressed: true, ..at(p) });
    for _ in 0..10 {
        frame(g, at(p)); // a third of a second
    }
    assert!(g.press.as_ref().is_some_and(|p| p.ring), "the ring didn't open on {p}");
    let verbs = ui::ring_verbs(g);
    let i = verbs.iter().position(|v| v == verb).unwrap();
    let s = ui::ring_slot(ui::ring_centre(g, p), i, verbs.len());
    frame(g, at(s));
    frame(g, ui::Mouse { released: true, ..at(s) });
}

fn said(g: &G, line: &str) -> bool {
    logic::has(&g.st.said, line)
}

#[test]
fn ring_style_plays_like_a_player_would() {
    let mut g = demo("ring", "");
    ring_pick(&mut g, KEY, "take");
    settle(&mut g);
    assert!(logic::has(&g.st.inventory, "key"), "taking the key from the ring");

    frame(&mut g, ui::Mouse { right: true, ..at(vec2(320.0, 100.0)) });
    assert!(g.inv_open, "a right-click opens the inventory");
    let slot = ui::inv_rect(&g, 0).center();
    click(&mut g, slot);
    assert_eq!(g.item, "key", "a click in the inventory picks the key up");
    click(&mut g, vec2(20.0, 20.0));
    assert!(!g.inv_open && g.item == "key", "a click outside puts the inventory away, still holding the key");
    click(&mut g, DOOR);
    settle(&mut g);
    assert!(said(&g, "str_guard_not_on_watch") && g.item.is_empty(), "using the key on the door: the guard objects");

    let guard = g.pos["guard"] - vec2(0.0, 20.0);
    ring_pick(&mut g, guard, "talk");
    settle(&mut g);
    let opts = logic::options(&g);
    let i = opts.iter().position(|o| o.line == "str_opt_who").expect("talking opens the conversation");
    let option = ui::option_rect(&g, i, opts.len()).center();
    click(&mut g, option);
    settle(&mut g);
    assert!(said(&g, "str_guard_who"), "clicking an option at the bottom plays it");
    std::fs::remove_file(&g.zip).ok();
}

#[test]
fn bar_styles_play_like_a_player_would() {
    for labels in ["words", "icons", "captioned"] {
        let mut g = demo("bar", labels);
        let verb = |g: &G, v: &str| ui::verb_rect(g, g.spec.game.verbs.iter().position(|x| x == v).unwrap()).center();
        let (take, use_) = (verb(&g, "take"), verb(&g, "use"));
        click(&mut g, take);
        assert_eq!(g.verb, "take", "{labels}: clicking the verb selects it");
        click(&mut g, KEY);
        settle(&mut g);
        assert!(logic::has(&g.st.inventory, "key"), "{labels}: taking the key");
        let slot = ui::inv_rect(&g, 0).center();
        click(&mut g, use_);
        click(&mut g, slot);
        assert_eq!(g.item, "key", "{labels}: picking the key in the bar");
        click(&mut g, DOOR);
        settle(&mut g);
        assert!(said(&g, "str_guard_not_on_watch"), "{labels}: using the key on the door");
        std::fs::remove_file(&g.zip).ok();
    }
}

#[test]
fn clicks_follow_the_draw_order_and_skip_what_isnt_there() {
    // put the cake down in the street, over the key (the key's spot is in its painted colour)
    let mut g = demo("bar", "words");
    let cake = spec::Hotspot { color: "#123456".into(), mark: "key_spot".into(), pos: [245.0, 292.0], ..Default::default() };
    g.spec.locations.get_mut("street").unwrap().hotspots.insert("cake".into(), cake);
    let room = real_room(&g, "street");
    g.rooms.insert("street".into(), room);
    let set = |g: &mut G, layer: &str, cond: Vec<spec::Cond>| {
        let h = g.spec.locations.get_mut("street").unwrap().hotspots.get_mut("cake").unwrap();
        h.layer = layer.to_string();
        h.cond = cond;
    };
    let order = |g: &G| scene(g, &g.rooms["street"]).into_iter().filter(|x| x.0 == "object").map(|x| x.1).collect::<Vec<_>>();

    // depth layer: the cake's bottom edge (308) is below the key's (307), so it's drawn in front
    assert_eq!(order(&g), ["key", "cake"]);
    assert_eq!(target_at(&g, KEY), "cake", "the front object gets the click, on its own pixels");
    // back layer: behind everything, so the key's painted colour wins
    set(&mut g, "back", vec![]);
    assert_eq!(order(&g), ["cake", "key"]);
    assert_eq!(target_at(&g, KEY), "key");
    // in front again, but only there once the game's won: until then it isn't drawn or clicked
    set(&mut g, "front", vec![spec::Cond::Flag { flag: "game_won".into(), is: true }]);
    assert_eq!(order(&g), ["key"]);
    assert_eq!(target_at(&g, KEY), "key", "a hidden object lets the one behind it be clicked");
    g.st.flags.insert("game_won".into(), true);
    assert_eq!(target_at(&g, KEY), "cake");
    std::fs::remove_file(&g.zip).ok();
}

// ---------------- saving and loading, through the menu ----------------

/// A fresh, empty saves folder for one test.
fn temp_saves(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pnc-saves-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    dir
}

fn click_menu(g: &mut G, action: &str) {
    let r = ui::menu_buttons(g).into_iter().find(|(_, a)| a == action).map(|(r, _)| r);
    click(g, r.unwrap_or_else(|| panic!("no '{action}' button on this page")).center());
}

fn page(g: &G) -> &'static str {
    g.pause.as_ref().map(|p| p.page).unwrap_or("closed")
}

#[test]
fn the_menu_saves_to_a_slot_and_loads_it_back() {
    let mut g = demo("ring", "");
    g.saves = temp_saves("menu");
    ring_pick(&mut g, KEY, "take");
    settle(&mut g);
    assert!(carried(&g, "key"));

    let corner = ui::menu_button(&g).center();
    click(&mut g, corner); // the ☰ button opens the menu
    assert_eq!(page(&g), "main");
    click_menu(&mut g, "save");
    click_menu(&mut g, "save:2");
    assert!(g.saves.join("2.sav").is_file());
    assert!(g.pause.as_ref().unwrap().note.contains("slot 2"));
    click_menu(&mut g, "back");
    click_menu(&mut g, "resume");
    assert_eq!(page(&g), "closed");

    // things change after saving...
    g.st.inventory.clear();
    g.st.flags.insert("guard_left".into(), true);
    // ...and loading slot 2 puts them back
    ui::toggle_menu(&mut g); // what Esc does
    click_menu(&mut g, "load");
    click_menu(&mut g, "load:5"); // empty: nothing happens
    assert_eq!(page(&g), "load");
    click_menu(&mut g, "load:2");
    assert_eq!(page(&g), "closed");
    assert!(carried(&g, "key") && !g.st.flags["guard_left"]);
    std::fs::remove_dir_all(&g.saves).ok();
    std::fs::remove_file(&g.zip).ok();
}

#[test]
fn saving_waits_until_the_game_is_waiting_for_the_player() {
    let mut g = demo("ring", "");
    g.saves = temp_saves("waits");
    let guard = g.pos["guard"] - vec2(0.0, 20.0);
    ring_pick(&mut g, guard, "talk");
    settle(&mut g); // in the conversation, its options showing
    ui::toggle_menu(&mut g);
    click_menu(&mut g, "save"); // greyed out: nothing happens
    assert_eq!(page(&g), "main");
    assert!(!can_save(&g) && !g.saves.exists());
    std::fs::remove_file(&g.zip).ok();
}

#[test]
fn entering_a_room_autosaves() {
    let mut g = demo("bar", "words");
    g.saves = temp_saves("auto");
    for mv in ["take key", "talk guard", "say 'str_opt_who'", "say 'str_opt_cat'", "use key on door", "walk door"] {
        play(&mut g, mv);
    }
    let (st, at) = spec::read_save(&g.saves, "auto", &g.spec, &g.key).expect("an autosave");
    assert!(st.location == "house" && at > 0);
    // going back out to the street autosaves again, over it: the autosave follows the player
    play(&mut g, "walk house_door");
    assert_eq!(spec::read_save(&g.saves, "auto", &g.spec, &g.key).unwrap().0.location, "street");
    // and it loads: whatever happens next is undone
    g.st.inventory.push("cake".into());
    assert!(load_slot(&mut g, "auto"));
    assert!(g.st.location == "street" && !carried(&g, "cake"));
    std::fs::remove_dir_all(&g.saves).ok();
    std::fs::remove_file(&g.zip).ok();
}
