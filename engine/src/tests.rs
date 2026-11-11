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
fn bundle(project: &Path, name: &str) -> String {
    let mut files = vec![];
    collect(project, project, &mut files);
    files.sort();
    let out = std::env::temp_dir().join(format!("pnc-parity-{name}-{}.zip", std::process::id()));
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
    Room { tex: HashMap::new(), masks: HashMap::new(), walk: vec![true; 160 * 90], gw: 160, gh: 90 }
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

/// The target is somewhere the player could click: an NPC standing here, or an active hotspot.
/// (Whether its colour is painted in the object mask is the Check's job.)
fn assert_clickable(g: &G, target: &str) {
    let npc = g.pos.contains_key(target) && logic::npcs_here(g).iter().any(|(c, _, _)| c == target);
    let hot = g.spec.locations[&g.st.location].hotspots.contains_key(target) && logic::active(g, target);
    assert!(npc || hot, "'{target}' isn't there to click in {}", g.st.location);
}

fn assert_carried(g: &G, item: &str) {
    assert!(logic::has(&g.st.inventory, item), "'{item}' isn't carried");
}

/// One move, in the solver's label format (see scenarios.yaml), made the way a player would.
fn play(g: &mut G, mv: &str) {
    if let Some(line) = mv.strip_prefix("say '").and_then(|m| m.strip_suffix('\'')) {
        let o = logic::options(g).into_iter().find(|o| o.line == line);
        let o = o.unwrap_or_else(|| panic!("the option '{line}' isn't offered"));
        logic::choose(g, &o);
    } else if let Some(rest) = mv.strip_suffix(" (inventory)") {
        let (verb, it) = rest.split_once(' ').unwrap();
        assert_carried(g, it);
        g.verb = verb.to_string();
        logic::click_item(g, verb, it);
        if verb == "use" {
            logic::click_item(g, verb, it); // picked it; clicking it again uses it on its own
        }
    } else if let Some((item, target)) = mv.strip_prefix("use ").and_then(|m| m.split_once(" on ")) {
        assert_carried(g, item);
        g.verb = "use".to_string();
        logic::click_item(g, "use", item); // pick the item, then click what to use it on
        if logic::has(&g.st.inventory, target) {
            logic::click_item(g, "use", target);
        } else {
            assert_clickable(g, target);
            click_target(g, "use", target);
        }
    } else {
        let (verb, target) = mv.split_once(' ').unwrap();
        assert_clickable(g, target);
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
        std::fs::remove_file(zip).ok();
    }
}

// ---------------- the interface styles, played through ui::input with a scripted mouse ----------------

const KEY: Vec2 = Vec2::new(255.0, 300.0); // inside the key's hotspot in the demo's street
const DOOR: Vec2 = Vec2::new(425.0, 220.0); // inside the door's

/// A room with its real masks (decoded on the CPU; no textures), so clicks hit what a player's would.
fn real_room(g: &G, loc: &str) -> Room {
    let l = &g.spec.locations[loc];
    let mut masks = HashMap::new();
    for (name, path) in [("walk", &l.walkway), ("objects", &l.objects_mask), ("depth", &l.depth)] {
        if !path.is_empty() {
            masks.insert(name.to_string(), Image::from_file_with_format(&spec::read(&g.zip, path, &g.key), None).unwrap());
        }
    }
    let (walk, gw, gh) = bake_walk(&masks, vec2(res(g).0, res(g).1));
    Room { tex: HashMap::new(), masks, walk, gw, gh }
}

/// The demo in the given interface style, waiting in the street. (Delete g.zip when done.)
fn demo(style: &str, labels: &str) -> G {
    let zip = bundle(&parity_dir().join("../../editor/projects/demo"), &format!("ui-{style}-{labels}"));
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
    g.t += 1.0 / 30.0;
    update(g, 1.0 / 30.0);
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
