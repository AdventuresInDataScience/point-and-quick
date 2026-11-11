# Point & Click Maker: developer guide

Two apps joined by one contract. The **editor** (Python, Streamlit) builds and validates a game; the **engine** (Rust, macroquad) runs it. Nothing passes between them except the exported zip of spec files and assets. AI lives only in the editor, so shipped games carry none of it.

```
editor/                     Python. Everything that authors a game.
  schema/spec_v0.1.yaml     The contract: JSON Schema for every spec file, the state manifest, LLM patches
  spec.py                   Project load/save, the condition/action tables, patch ops, zip export
  check.py                  The Check: tier 1 validation, tier 2 puzzle solver (simulates logic.rs)
  ai.py                     AI plugins: rule authoring, voices, art, sprite sheets, audio
  app.py                    The Streamlit UI (all pages)
  demo_assets.py            Draws placeholder art/audio for the demo project
  projects/demo/            Hand-written test game: 2 rooms, a locked door, a key, a guard, a cake
engine/                     Rust. Everything that plays a game.
  src/spec.rs               Serde structs mirroring the schema; zip reading, decryption, saves
  src/logic.rs              Game rules: conditions, verb dispatch, action queue, dialogue
  src/main.rs               Window, asset streaming, walking, audio, input, drawing, main loop
```

Each file has one clear job, and modules are only split where the job changes. Logic is plain functions over plain data: dicts in Python, `HashMap`/`Vec` of serde structs in Rust. There are no classes in the editor. The engine has exactly two enums (`Cond` and `Action`, serde-tagged, mirroring the YAML's `type` key) and one state struct, `G`, that every function takes.


## The spec

The schema is the product; both apps follow from it. `editor/schema/spec_v0.1.yaml` has one `$def` per file kind, and the engine's structs in `spec.rs` mirror it field for field. Both reject unknown fields, so a typo fails loudly in the Check rather than silently in the game.

A project folder looks like this, and the zip has the same layout:

```
game.yaml                          global sheet: verbs, flags + defaults, start state, defaults, combinations, ending flag
map.yaml                           every location-to-location edge: id, from, to, spawn mark
locations/<id>/sheet.yaml          marks, hotspots, exits, NPC placements, walk-behinds, music, enter rules
locations/<id>/*.png               background, walkway, object mask, depth, walk-behind masks (same size)
objects/<id>.yaml                  visual states -> sprites, rules, fallback lines, icon (icon = carryable)
characters/<id>.yaml               animations, voice, rules, fallback lines
characters/<id>/dialogue/*.yaml    global.yaml, plus <location>.yaml overrides
strings/<lang>.yaml                all player-facing text, by stable ID
voice/<line id>.wav                generated TTS (optional; engine plays it with the line)
```

A few conventions hold everywhere:

- **IDs and paths.** Everything is referred to by ID string. Asset paths are relative to the project root, with forward slashes.
- **Empty means none.** `""` means "none" (no sprite, no music, no gate).
- **Colours.** Hotspots are identified by their exact colour in the object mask (`#rrggbb`), and the editor auto-assigns distinct ones.
- **`gone`.** An object in the state `gone` is neither drawn nor clickable. That's how picked-up items leave the room.
- **Facing rows.** Animation sheets are grids: columns are frames, rows are 8 facing directions in the order e, se, s, sw, w, nw, n, ne. A sheet can also be 1 row, which is mirrored for westward facings. Feet sit at the bottom centre of each frame.
- **Depth.** The depth mask runs black (far) to white (near). A location's `scale: [far, near]` maps that to character scale and walk speed.
- **Strings.** Text never appears in rules, only string IDs. The editor's text fields mint an ID once from the first words of the text and keep it, so rewording a line keeps its translations, TTS file and "already said" tracking.

### Game semantics

This is the behaviour both `engine/src/logic.rs` and the solver in `editor/check.py` implement. **If you change one, change the other**, or the solver will certify games the engine plays differently.

**Rules** read "when VERB on TARGET (with ITEM), if CONDITIONS, do ACTIONS", and the first matching rule on a sheet wins. Rule order therefore matters: put the specific case (guard present) above the general one (door locked).

**Verb dispatch** (`interact`) runs in four steps:
1. Using an item on another carried item looks at `game.yaml` combinations; order of the two items doesn't matter.
2. The target owns the interaction, so the target's rules are checked. "Use key on door" lives on the door with `with: key`. The Check rejects item-on-item rules on item sheets, so nothing is defined in two places.
3. `talk` to a character with no matching rule opens its dialogue.
4. Fallback lines: the used item's line for the verb, then the target's, then `game.yaml` defaults.

**Exits** (`arrive`). Walking onto a hotspot that a location's `exits` names takes that map edge, but only if the hotspot's object is in the exit's `state`. Otherwise it's an ordinary `walk` interaction, which is how a locked door says "It's locked."

**The action queue** runs actions in order. `say`, `walk`, `wait` and `go` block until done. This one mechanism is also the cutscene system.

**Entering a room** resets the music to the room's default and places characters. It then runs the first matching `enter` rule ahead of anything already queued.

**Dialogue lookup.** A character's nodes are its `global.yaml` merged with `<current location>.yaml`; the location's start node and nodes win. A node says its line and runs its actions; then, once the queue is idle, its visible options are offered. If it has none, it follows `goto` or the conversation ends. Choosing an option makes the player say it, then plays the option's node. `once` options hide after being said.

**State** is one manifest: location, mark, inventory, flags, object states, character locations/states, said lines, music override. The start state in `game.yaml` uses this shape, and a save file is simply a snapshot of it. `migrate` (both apps) fills in flags/objects/characters the state doesn't know and drops removed ones, which is what keeps old saves loading after a game update.


## Editor

Run it with `streamlit run editor/app.py`. The whole project is one dict (`spec.load`), with keys mirroring the folder: `game`, `map`, `locations`, `objects`, `characters`, `dialogue`, `strings`. Pages edit that dict in place, and the last line of `app.py` saves it whenever a run changed something.

Three Streamlit habits are worth knowing before editing `app.py`:

- **`put(d, key, value)`** writes a widget's value only if it changed, so merely viewing a page never rewrites files.
- **Widget keys include `S.ver`.** Anything that changes the project from outside the widgets (undo, an accepted AI patch, adding or removing a list item) calls `restructure()` or `reload()`. That bumps the version so every widget re-reads the project instead of keeping stale state.
- **The pickers are data-driven.** `spec.CONDS` and `spec.ACTS` give each field of each condition/action a kind (`flag`, `object`, `mark`, `line`…). `edit_list` turns a kind into a picker bound to real IDs, and `phrase` turns it into rule-card prose. The Check uses the same tables to verify references.

The location page is the layer editor. Clicking the image either drops a mark (named, anchored at the feet) or adds polygon points. Polygons fill into the walkway, the depth mask (as a far-to-near gradient), an object's colour in the object mask, or a walk-behind. The preview always shows the walkway faintly, and draws characters at their marks at the scale the depth mask gives them there. Layers can also be painted elsewhere and uploaded. Clicks come from `streamlit-image-coordinates`. If richer drawing is ever needed, swap that one call for a canvas component, or move this page to NiceGUI.

### The Check

`check.check(P)` is tier 1. It runs schema conformance per file, then (only if shapes are right) every reference: flags, objects, items, characters, locations, marks (within the right location), edges, dialogue nodes, string IDs in every language, and asset files. It also checks marks on the walkway, layer sizes, and colours actually painted in the object mask. It warns about unused assets, flags, strings and untakeable edges.

`check.solve(P)` is tier 2: a breadth-first search over abstract states. A state is room, inventory, flags, object states, NPC rooms, open conversation, and only the said-lines some condition or `once` option reads; player position is ignored. It reports whether the ending flag is reachable (with the shortest route), unreachable rooms, unobtainable items, and softlocks. A softlock is a reachable state that can't reach the ending; walking the graph backwards from ending states finds them, and the report includes a shortest repro path. `max_states` caps the search on big games.

`check.run_all` combines both for the UI. Release export refuses to run unless it's clean.

### AI

`ai.py` keeps every model optional and behind one function per feature, chosen in `editor/ai.yaml` (edited on the AI assets page): `none`, `onnx` (local, via ONNX Runtime), or `remote` (HTTP). Libraries are imported only when a feature is used.

**Rule authoring** (`author`). The model receives a context slice (ID lists plus only the sheets the intent mentions) and must answer in the patch schema. `onnxruntime-genai` guidance and OpenAI-style `json_schema` both constrain the output, with `add_rule` targets restricted to real IDs. Each patch is applied to a copy and run through the Check. New errors go back to the model for up to two repair passes before the user sees anything. An `ask` op becomes tappable options in the UI. Accepting snapshots the old project for undo.

**Voices** (`voice_all`) write `voice/<line id>.wav`, using Piper locally or a cloning server remotely with the character's reference WAV. A hash of text plus voice settings means only changed lines regenerate.

**Art and sprite sheets** use Stable Diffusion (ONNX via `optimum`, or remote). Sheets are generated as one 8-direction grid. `process_sheet` slices it, keys out the background, aligns every frame's feet to one pivot, and optionally quantizes to one shared palette. `regen_frame` inpaints a single cell so the others act as the reference.

**Music and SFX** are remote only and marked experimental.

### Export and the bundle

`spec.export` writes `editor/exports/<id>.zip` (release) or `<id>-dev.zip` (dev). `header.json` stays in the clear: spec version, game ID, an `encrypted` flag, and the file index. Every other file is stored uncompressed and, in release builds, encrypted on its own with AES-256-GCM as `nonce(12) + ciphertext + tag`, so the engine can stream single assets. The key is generated once per game into `<project>/.key` (gitignored) and compiled into that game's engine build. This deters casual extraction; a determined user can still pull the key from the binary.


## Engine

Build and run it with `cargo run --release -- path/to/game.zip`. A release bundle needs the key at build time: `GAME_KEY=<hex> cargo build --release`.

`main()` reads `header.json`, parses every spec file into `Spec` (`spec::load`) and builds the start state. It then starts the loader thread and audio, enters the start room, and runs the frame loop:

```
pump (upload one finished room) -> update (timers, walking, arrivals, queue, menus, fades, ending)
-> keys -> input -> draw
```

`G` holds everything at runtime: the spec, the save-able `State`, the action queue, speech, the open menu, character positions/paths/facings, the selected verb/item, textures, loaded rooms, and audio handles. `logic.rs` functions take `&mut G` and call back into `main.rs` for the physical side (`walk_to`, `move_char`, `play_music`, `room_changed`), which is why it imports `crate::G`.

### Assets and memory

Character sheets and inventory icons load once at startup. Rooms stream:

- On entering a room, `room_changed` keeps it plus its neighbours on the map, drops every other room's textures, and sends missing rooms to the loader thread.
- The thread reads, decrypts and decodes the images, and cuts walk-behinds out of the background.
- `pump` uploads at most one room per frame to the GPU and bakes the walk grid. macroquad's own async loading doesn't truly run in the background on desktop, hence the thread.

While the new room isn't ready, the screen stays faded and the queue waits.

### Walking, drawing and hit-testing

The walkway mask is baked into a 4-pixel grid. Paths are a breadth-first search to the walkable cell nearest the click, then string-pulled into straight segments. Depth scales both size and speed.

Each frame draws the background, then objects, characters and walk-behinds sorted by baseline (sprite bottom, feet, or the walk-behind's `baseline`).

Clicks hit NPCs by their sprite box first, then hotspots by the object mask's colour at that pixel. Everything is drawn at the game's own resolution with a 64-pixel verb/inventory panel underneath, scaled and letterboxed into the window. `View` is that transform.

### Controls, audio and saves

The verb bar comes from `game.yaml` verbs (labels from `verb_<verb>` strings). Left-click uses the selected verb, right-click looks, and the default verb is walk. A click skips the current line. Other controls: F5 saves, F9 loads, `[` and `]` change music volume, Esc quits.

Music streams through kira with crossfades on a music bus; effects and voice lines play on an sfx bus.

Saves are JSON at `./saves/<game id>.sav`, encrypted (and so tamper-checked) with the game key in release builds.


## Common changes

**Adding a condition or action type** touches each layer once, in this order:

1. Schema: add a `oneOf` entry under `cond` or `action` in `spec_v0.1.yaml`.
2. `spec.py`: add it to `CONDS` or `ACTS` with field kinds; pickers and reference checks come for free.
3. `app.py`: add a prose template to `COND_TXT` or `ACT_TXT`.
4. `check.py`: handle it in `ok()` or `run()` if it affects reachability.
5. `spec.rs`: add the enum variant (field names as in the schema).
6. `logic.rs`: handle it in `ok()` or `run_queue()`.

**Adding a verb** needs no code: add it to `game.yaml` verbs (and optionally a `verb_<name>` string).

**Changing the spec shape** means bumping `SPEC_VERSION` in both `spec.py` and `spec.rs`, adding a new schema file, and extending `migrate` (both apps) so old saves still load.


## Status and known gaps

**Verified here.** The Python side ran in a sandbox without network access. The Check passes the demo, and the solver finds its seven-step route. Deliberately broken copies were all caught: a typo'd flag, a mark off the walkway, missing text, an unknown field, a misplaced item-on-item rule, a bad dialogue `goto`, a softlock and an unreachable ending. The export round-trips through AES-GCM, and the sprite pipeline aligns feet exactly. The author/repair loop and clarifying questions work against a mocked model. Every editor page ran against the demo under a mocked Streamlit without altering any data. Every Rust struct and enum was cross-checked field by field against the schema.

**Not verified here.** There was no Rust toolchain or network, so the engine has not been compiled. Expect a few small fixes on first `cargo build`, most likely at third-party API edges:

- kira 0.8's `loop_region(..)`, `from_cursor(cursor, settings)` and `set_volume`.
- macroquad 0.4's `Image::from_file_with_format` returning a `Result`.
- zip 2.x's `by_name`.

Streamlit itself, `jsonschema` and the real AI backends were also not run.

**Other limits:**

- **Comments.** Saving from the editor rewrites YAML, so hand-written comments in spec files are lost (the demo's comments survive until it's first edited).
- **Font.** macroquad's default font is ASCII-focused; load a TTF for localized text.
- **WASM.** macroquad builds to WASM, but the loader thread doesn't exist there; a WASM build needs `room_changed` to load synchronously instead.
- **Fixed engine choices.** Dialogue options show up to four lines in the panel, and longer menus grow up over the scene. The walk grid is 4 px.

**The plan's open questions, as decided here:**

- **Conditions:** typed flag/state/inventory/location/said conditions, AND-ed. There's no expression language yet.
- **Verb UI:** a fixed verb bar.
- **Resolution:** set per game in `game.yaml`.
- **Save location:** `./saves` next to where the engine runs. A per-platform app-data directory is the obvious next step.
