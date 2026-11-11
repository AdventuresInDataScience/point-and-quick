//! spec.rs — structs that mirror the YAML spec (editor/schema/spec_v0.1.yaml), reading files
//! out of the game zip, and save files. serde does all the parsing and rejects unknown fields.
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;

pub const SPEC_VERSION: &str = "0.1";

// defaults for fields the YAML may leave out
fn yes() -> bool { true }
fn one() -> f32 { 1.0 }
fn one_u() -> u32 { 1 }
fn unit() -> [f32; 2] { [1.0, 1.0] }
fn speed() -> f32 { 120.0 }
fn fps() -> f32 { 8.0 }

/// A condition: a dict with a "type" key, but checked by the compiler. All in a list must hold.
#[derive(Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cond {
    Flag { flag: String, #[serde(default = "yes")] is: bool },
    State { object: String, is: String },
    Has { item: String, #[serde(default = "yes")] is: bool },
    At { character: String, location: String, #[serde(default = "yes")] is: bool },
    Said { line: String, #[serde(default = "yes")] is: bool },
}

/// An action. They run in order from a queue; say, walk, wait and go block until done.
#[derive(Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Say { line: String, #[serde(default)] who: String },          // who "" = the player
    SetFlag { flag: String, #[serde(default = "yes")] value: bool },
    SetState { object: String, state: String },
    Give { item: String },
    Remove { item: String },
    Move { character: String, location: String, #[serde(default)] mark: String },
    CharState { character: String, state: String },
    Go { edge: String },
    Walk { mark: String },
    Talk { character: String, #[serde(default)] node: String },   // node "" = the start node
    PlayMusic { file: String, #[serde(default = "one")] fade: f32 },
    PlaySound { file: String },
    Wait { seconds: f32 },
}

/// "When VERB on TARGET (with ITEM), if CONDITIONS, do ACTIONS". Combinations use `items` instead.
#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    pub verb: String,
    pub with: String,
    pub items: Vec<String>,
    #[serde(rename = "if")]
    pub cond: Vec<Cond>,
    #[serde(rename = "do")]
    pub act: Vec<Action>,
}

/// Where a character is. Part of the state, so NPCs can move between rooms.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CharPos {
    pub location: String,
    pub mark: String,
    pub state: String,
}

/// The state manifest: the start state in game.yaml, and every save file.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub spec_version: String,
    pub location: String,
    pub mark: String,
    pub inventory: Vec<String>,
    pub flags: HashMap<String, bool>,
    pub objects: HashMap<String, String>,        // object -> visual state
    pub characters: HashMap<String, CharPos>,
    pub said: Vec<String>,                       // conversation state: line IDs already said
    pub music: String,                           // play_music override; "" = the room's default
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Game {
    pub spec_version: String,
    pub id: String,
    pub title: String,
    pub resolution: [f32; 2],
    pub lang: String,
    pub player: String,
    pub verbs: Vec<String>,
    pub flags: HashMap<String, bool>,
    pub ending: String,
    pub start: State,
    pub defaults: HashMap<String, String>,
    pub combinations: Vec<Rule>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub spawn: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MapSheet {
    pub edges: Vec<Edge>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Hotspot {
    pub color: String,       // this object's colour in the object mask
    pub mark: String,        // where the player walks to interact
    pub pos: [f32; 2],       // top-left of the object's sprite
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Exit {
    pub hotspot: String,
    pub state: String,       // the hotspot's object must be in this state ("" = always open)
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Npc {
    pub character: String,
    pub mark: String,
    pub facing: String,
    pub state: String,
    #[serde(rename = "if")]
    pub cond: Vec<Cond>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Behind {
    pub mask: String,
    pub baseline: f32,       // drawn over characters whose feet are above this line
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Location {
    pub id: String,
    pub name: String,
    pub background: String,
    pub walkway: String,
    pub objects_mask: String,
    pub depth: String,
    #[serde(default = "unit")]
    pub scale: [f32; 2],     // character scale at depth black (far) and white (near)
    pub walkbehinds: Vec<Behind>,
    pub music: String,
    pub marks: HashMap<String, [f32; 2]>,
    pub hotspots: HashMap<String, Hotspot>,
    pub exits: HashMap<String, Exit>,
    pub npcs: Vec<Npc>,
    pub rules: Vec<Rule>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Object {
    pub id: String,
    pub name: String,
    pub icon: String,                        // set = the object can be carried
    pub state: String,
    pub states: HashMap<String, String>,     // state -> sprite ("" = draw nothing)
    pub rules: Vec<Rule>,
    pub fallback: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Anim {
    pub sheet: String,
    #[serde(default = "one_u")]
    pub cols: u32,           // frames
    #[serde(default = "one_u")]
    pub rows: u32,           // 8 facing directions, or 1 (mirrored for west)
    #[serde(default = "fps")]
    pub fps: f32,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Voice {
    pub model: String,
    pub ref_wav: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Character {
    pub id: String,
    pub name: String,
    #[serde(default = "speed")]
    pub speed: f32,
    pub text_color: String,
    pub animations: HashMap<String, Anim>,
    pub voice: Voice,
    pub rules: Vec<Rule>,
    pub fallback: HashMap<String, String>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Opt {
    pub line: String,
    pub goto: String,
    pub once: bool,          // hidden once said
    #[serde(rename = "if")]
    pub cond: Vec<Cond>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Node {
    pub say: String,
    #[serde(rename = "do")]
    pub act: Vec<Action>,
    pub options: Vec<Opt>,
    pub goto: String,        // followed when no options are showing
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Dialogue {
    pub start: String,
    pub nodes: HashMap<String, Node>,
}

/// The unencrypted header.json at the front of the zip.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Header {
    pub spec_version: String,
    pub game: String,
    pub encrypted: bool,
    pub files: Vec<String>,
}

/// The whole game spec, keyed by ID. Dialogue is character -> file stem ("global" or a location).
pub struct Spec {
    pub header: Header,
    pub game: Game,
    pub edges: Vec<Edge>,
    pub locations: HashMap<String, Location>,
    pub objects: HashMap<String, Object>,
    pub characters: HashMap<String, Character>,
    pub dialogue: HashMap<String, HashMap<String, Dialogue>>,
    pub strings: HashMap<String, String>,
}

/// One file from the game zip, decrypted when the bundle is encrypted.
/// The zip is opened per call: each file is stored on its own, so this streams single assets.
pub fn read(zip_path: &str, name: &str, key: &Option<Vec<u8>>) -> Vec<u8> {
    let file = std::fs::File::open(zip_path).expect("can't open the game zip");
    let mut archive = zip::ZipArchive::new(file).expect("not a zip file");
    let mut bytes = Vec::new();
    archive.by_name(name).expect(name).read_to_end(&mut bytes).unwrap();
    match key {
        Some(k) => decrypt(&bytes, k).expect("can't decrypt: was the engine built with this game's GAME_KEY?"),
        None => bytes,
    }
}

/// AES-256-GCM, laid out as nonce (12 bytes) + ciphertext + tag, the same as the editor writes.
pub fn decrypt(bytes: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).unwrap();
    cipher.decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..]).ok()
}

pub fn encrypt(plain: &[u8], key: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(key).unwrap();
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(&nonce, plain).unwrap());
    out
}

/// Parse every spec file listed in the header. Files are recognised by their path.
pub fn load(zip_path: &str, key: &Option<Vec<u8>>, header: Header) -> Spec {
    let text = |name: &str| String::from_utf8(read(zip_path, name, key)).expect(name);
    let game: Game = serde_yaml::from_str(&text("game.yaml")).expect("game.yaml");
    let map: MapSheet = serde_yaml::from_str(&text("map.yaml")).expect("map.yaml");
    let lang = if game.lang.is_empty() { "en".to_string() } else { game.lang.clone() };
    let mut spec = Spec {
        header, game, edges: map.edges,
        locations: HashMap::new(), objects: HashMap::new(), characters: HashMap::new(),
        dialogue: HashMap::new(), strings: HashMap::new(),
    };
    for f in spec.header.files.clone() {
        if !f.ends_with(".yaml") { continue; }
        let p: Vec<&str> = f.trim_end_matches(".yaml").split('/').collect();
        if p.len() == 3 && p[0] == "locations" && p[2] == "sheet" {
            spec.locations.insert(p[1].to_string(), serde_yaml::from_str(&text(&f)).expect(&f));
        } else if p.len() == 2 && p[0] == "objects" {
            spec.objects.insert(p[1].to_string(), serde_yaml::from_str(&text(&f)).expect(&f));
        } else if p.len() == 2 && p[0] == "characters" {
            spec.characters.insert(p[1].to_string(), serde_yaml::from_str(&text(&f)).expect(&f));
        } else if p.len() == 4 && p[0] == "characters" && p[2] == "dialogue" {
            let sheet: Dialogue = serde_yaml::from_str(&text(&f)).expect(&f);
            spec.dialogue.entry(p[1].to_string()).or_default().insert(p[3].to_string(), sheet);
        } else if f == format!("strings/{lang}.yaml") {
            spec.strings = serde_yaml::from_str(&text(&f)).expect(&f);
        }
    }
    spec
}

/// A new game: the start state with every declared flag and object state filled in.
pub fn new_state(spec: &Spec) -> State {
    let mut st = spec.game.start.clone();
    migrate(spec, &mut st);
    st
}

/// Bring a state (the start state, or a save from an older build of the game) in line with
/// the current spec: add flags, objects and characters it doesn't know, drop removed ones.
pub fn migrate(spec: &Spec, st: &mut State) {
    for (f, v) in &spec.game.flags { st.flags.entry(f.clone()).or_insert(*v); }
    st.flags.retain(|f, _| spec.game.flags.contains_key(f));
    for (o, sheet) in &spec.objects { st.objects.entry(o.clone()).or_insert(sheet.state.clone()); }
    st.objects.retain(|o, _| spec.objects.contains_key(o));
    for (c, p) in &spec.game.start.characters { st.characters.entry(c.clone()).or_insert(p.clone()); }
    st.characters.retain(|c, _| spec.characters.contains_key(c));
    st.inventory.retain(|i| spec.objects.contains_key(i));
    if !spec.locations.contains_key(&st.location) {
        st.location = spec.game.start.location.clone();
        st.mark = spec.game.start.mark.clone();
    }
    st.spec_version = SPEC_VERSION.to_string();
}

/// A save is the state manifest as JSON, encrypted with the game key when there is one
/// (GCM's tag also rejects hand-edited saves). Saves live in ./saves/<game id>.sav.
pub fn save(st: &State, game_id: &str, key: &Option<Vec<u8>>) {
    let json = serde_json::to_vec_pretty(st).unwrap();
    let bytes = match key { Some(k) => encrypt(&json, k), None => json };
    std::fs::create_dir_all("saves").unwrap();
    std::fs::write(format!("saves/{game_id}.sav"), bytes).unwrap();
}

pub fn load_save(spec: &Spec, key: &Option<Vec<u8>>) -> Option<State> {
    let bytes = std::fs::read(format!("saves/{}.sav", spec.game.id)).ok()?;
    let json = match key { Some(k) => decrypt(&bytes, k)?, None => bytes };
    let mut st: State = serde_json::from_slice(&json).ok()?;
    migrate(spec, &mut st);
    Some(st)
}
