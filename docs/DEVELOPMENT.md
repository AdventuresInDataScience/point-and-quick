# Point & Click Maker: developer guide

Two apps joined by one contract. The **editor** (Python, Streamlit) builds and validates a game; the **engine** (Rust, macroquad) runs it. Nothing passes between them except the exported zip of spec files and assets. AI lives only in the editor, so shipped games carry none of it.

```
editor/                     Python. Everything that authors a game.
  schema/spec_v0.1.yaml     The contract: JSON Schema for every spec file, the state manifest, LLM patches
  spec.py                   Project load/save, the condition/action tables, patch ops, zip export
  check.py                  The Check: tier 1 validation, tier 2 puzzle solver (simulates logic.rs)
  ai.py                     AI plugins: rule authoring, voices, art, sprite sheets, audio
  config.py                 Where the editor keeps things: settings, projects, exports, models (never in editor/)
  app.py                    The Streamlit UI (all pages)
  demo_assets.py            Draws placeholder art/audio (and the verb icons in ui/) for the demo
  examples/demo/            Hand-written test game: 2 rooms, a locked door, a key, a guard, a cake. Ships
                            with the editor and is never edited; copying it makes a project
  tests/                    pytest: the Check, the parity scenarios, settings, and the editor's UI
engine/                     Rust. Everything that plays a game.
  build.rs                  Compiles the game key in (masked) from GAME_KEY
  src/spec.rs               Serde structs mirroring the schema; zip reading, decryption, save files
  src/logic.rs              Game rules: conditions, verb dispatch, action queue, dialogue
  src/main.rs               Window, asset streaming, walking, audio, saving, drawing the room, main loop
  src/ui.rs                 The interface: input, verbs, inventory, speech, dialogue options, the menu
  src/tests.rs              cargo test: the parity scenarios, played through the engine
tests/parity/               Scenarios both apps must play identically, and a tiny YAML-only game for them
```

Each file has one clear job, and modules are only split where the job changes. Logic is plain functions over plain data: dicts in Python, `HashMap`/`Vec` of serde structs in Rust. There are no classes in the editor. The engine's game data has two enums (`Cond` and `Action`, serde-tagged, mirroring the YAML's `type` key), and there's one state struct, `G`, that every function takes. (The only other enum is `Loaded`, the audio thread's reply.)


## The spec

The schema is the product; both apps follow from it. `editor/schema/spec_v0.1.yaml` has one `$def` per file kind, and the engine's structs in `spec.rs` mirror it field for field. Both reject unknown fields, so a typo fails loudly in the Check rather than silently in the game.

A project folder looks like this, and the zip has the same layout:

```
game.yaml                          global sheet: verbs, interface, flags + defaults, start state, defaults, combinations, ending flag
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
- **`gone`.** An object in the state `gone` isn't anywhere: neither drawn nor clickable. That's how picked-up items leave the room. (To have an object turn up or leave because of a flag, give its placement an `if` instead: see [Game semantics](#game-semantics).)
- **Facing rows.** Animation sheets are grids: columns are frames, rows are 8 facing directions in the order e, se, s, sw, w, nw, n, ne. A sheet can also be 1 row, which is mirrored for westward facings. Feet sit at the bottom centre of each frame.
- **Depth.** The depth mask runs black (far) to white (near). A location's `scale: [far, near]` maps that to character scale and walk speed.
- **Strings.** Text never appears in rules, only string IDs. The editor's text fields mint an ID once from the first words of the text and keep it, so rewording a line keeps its translations, TTS file and "already said" tracking.

### Game semantics

This is the behaviour both `engine/src/logic.rs` and the solver in `editor/check.py` implement. **If you change one, change the other**, or the solver will certify games the engine plays differently. The parity scenarios in `tests/parity/scenarios.yaml` hold the two together: pytest plays them through the solver, `cargo test` through the engine, and both must end in the expected state (see [Tests](#tests)).

**Rules** read "when VERB on TARGET (with ITEM), if CONDITIONS, do ACTIONS", and the first matching rule on a sheet wins. Rule order therefore matters: put the specific case (guard present) above the general one (door locked).

**Verb dispatch** (`interact`) runs in four steps:
1. Using an item on another carried item looks at `game.yaml` combinations; order of the two items doesn't matter.
2. The target owns the interaction, so the target's rules are checked. "Use key on door" lives on the door with `with: key`. The Check rejects item-on-item rules on item sheets, so nothing is defined in two places.
3. `talk` to a character with no matching rule opens its dialogue.
4. Fallback lines: the used item's line for the verb, then the target's, then `game.yaml` defaults. A fallback line is said like any other, so it counts for `said` conditions.

**Inventory clicks** (`click_item`). With Use selected, clicking an item picks it ("Use key with…"). Clicking a second item uses the first on it; clicking the picked item again uses it on its own, so a plain `use` rule on an item sheet works from the inventory. Using an item on itself never matches a combination. The solver's moves are exactly these: `use X (inventory)` is X on its own, and `use X on Y` is X on Y.

**Placement conditions are live.** An NPC is in a room while the state puts it there and its placement's `if` holds. An object is in a room while it's placed there (in the location's `hotspots`), isn't `gone`, and its placement's `if` holds (`logic::shown`). Both are re-checked every frame, so things appear or leave as soon as a condition changes, without the player leaving the room. An object that isn't there is neither drawn nor clickable, and the solver doesn't offer it.

**Draw order and clicks.** Each object placed in a room has a `layer`:
- `back` objects are drawn behind characters, in their listed order (rugs, wall art);
- `depth` objects, the default, sort with characters and walk-behinds by bottom edge, with the listed order breaking ties;
- `front` objects are drawn in front of everything, in listed order.

A click goes to the front-most thing under the mouse, in that same order. A character is hit by its sprite's box. An object is hit by its sprite's visible pixels, or by its colour in the object mask, which can add extra click area around a small sprite. After that come objects painted into the background (no sprite in their current state), by mask colour alone. So an object with a sprite needs no mask painting. One without a sprite in some state needs its colour painted, or it can't be clicked in that state. The Check makes that an error, which is what lets the solver treat every object that's there as clickable.

**Exits** (`arrive`). Walking onto a hotspot that a location's `exits` names takes that map edge, but only if the hotspot's object is in the exit's `state`. Otherwise it's an ordinary `walk` interaction, which is how a locked door says "It's locked."

**The action queue** runs actions in order. `say`, `walk`, `wait` and `go` block until done. This one mechanism is also the cutscene system.

**Entering a room** resets the music to the room's default and places characters. It then runs the first matching `enter` rule ahead of anything already queued.

**Dialogue lookup.** A character's nodes are its `global.yaml` merged with `<current location>.yaml`; the location's start node and nodes win. A node says its line and runs its actions; then, once the queue is idle, its visible options are offered. If it has none, it follows `goto` or the conversation ends. Choosing an option makes the player say it, then plays the option's node. `once` options hide after being said.

**State** is one manifest: location, mark, inventory, flags, object states, character locations/states, said lines, music override. The start state in `game.yaml` uses this shape, and a save file is simply a snapshot of it. `migrate` (both apps) fills in flags/objects/characters the state doesn't know and drops removed ones, which is what keeps old saves loading after a game update.


## Editor

Run it with `streamlit run editor/app.py`. The whole project is one dict (`spec.load`), with keys mirroring the folder: `game`, `map`, `locations`, `objects`, `characters`, `dialogue`, `strings`. Pages edit that dict in place, and the last lines of `app.py` save it whenever a run changed something. Spec files are always read and written as UTF-8, whatever the platform's default encoding.

**Undo.** `S.saved` is the project as last written to disk. Each run that changes something pushes it onto the undo stack (50 deep) before saving, so every edit is one step: a committed text field, a table edit, an added rule, an accepted AI patch. Undo covers the spec files only; painted layers and uploaded files stay as they are.

Three Streamlit habits are worth knowing before editing `app.py`:

- **`put(d, key, value, default=)`** writes a widget's value only if it changed, so merely viewing a page never rewrites files. Pass `default` when the widget shows a default for a key the sheet may leave out (speed, scale, resolution…); otherwise viewing a sparse sheet would write that default in. The UI tests check every page this way on a sparse project.
- **Widget keys include `S.ver`.** Anything that changes the project from outside the widgets (undo, an accepted AI patch, adding or removing a list item) calls `restructure()` or `reload()`. That bumps the version so every widget re-reads the project instead of keeping stale state.
- **The pickers are data-driven.** `spec.CONDS` and `spec.ACTS` give each field of each condition/action a kind (`flag`, `object`, `mark`, `line`…). `edit_list` turns a kind into a picker bound to real IDs, and `phrase` turns it into rule-card prose. The Check uses the same tables to verify references.

The Game page's **Interface** section shows only the settings the chosen style uses, and writes only those (`spec.interface` gives that stored form). Icons are kept when the style changes, so switching back finds them again.

**Naming new things.** Projects, locations, objects, characters, marks and dialogue nodes are all added by typing a name, then pressing Enter or the button. `spec.make_id` makes the id from it ("Living Room" → `living_room`). A project's name becomes its title, and an entity's becomes its display name. When a name can't be used (nothing in it, or the id is taken), the editor says so and keeps the text so it can be fixed; nothing fails silently.

### Where things live

Nothing is ever written inside `editor/`, so the editor works the same from a source checkout or an installed package, whose folder may be read-only. `config.py` decides where everything goes:

| What | Where | Set by |
| --- | --- | --- |
| Settings (`settings.yaml`: the folders below, and the AI backends) | The user's config folder: `%APPDATA%/Point and Click Maker` on Windows, `~/Library/Application Support/Point and Click Maker` on macOS, `$XDG_CONFIG_HOME/point-and-click-maker` (or `~/.config/...`) elsewhere | `PNC_CONFIG` overrides the folder |
| Projects, the games being made | `Documents/Point and Click Maker/projects` | The Settings page; `PNC_PROJECTS` overrides it for one run |
| Exports, the playable zips | `Documents/Point and Click Maker/exports` | The Settings page |
| AI models | `Documents/Point and Click Maker/models` | The Settings page (and each model's own path on the AI assets page) |
| Examples (the demo) | `editor/examples`, shipped with the editor and only ever read | — |

The **Settings** page is there even when there's no project yet, and a first run opens on it. It has the three folders and the examples. Copying an example makes a new project in the projects folder, renamed if its name is taken.

The tests never touch the real settings or Documents: `conftest.py` gives every test its own `PNC_CONFIG` and home folder, and the UI tests put their projects in a temporary folder through `PNC_PROJECTS`.

The location page is the layer editor. Its controls sit beside the picture, so naming a mark or filling a polygon happens in view of it; uploads are tucked into an expander. The picture is stretched to its column, and clicks are scaled back to the image's own pixels. Clicking either drops a mark (named, anchored at the feet) or adds polygon points. Polygons fill into the walkway, the depth mask (as a far-to-near gradient), an object's colour in the object mask, or a walk-behind. The preview draws the room's objects in their starting state and its characters at their marks (at the scale the depth mask gives them there), all in the engine's draw order. An object with an "only here if…" condition is drawn faded, so it can still be seen and placed. The walkway always shows faintly. Below the tables, **Objects in this room** lists the placed objects in draw order: each one's layer, ▲/▼ to move it back or forward in the list, and its "only here if…" conditions. The hotspot table carries those over when it rewrites its rows. Layers can also be painted elsewhere and uploaded. Clicks come from `streamlit-image-coordinates`. If richer drawing is ever needed, swap that one call for a canvas component, or move this page to NiceGUI.

### The Check

`check.check(P)` is tier 1. It runs schema conformance per file, then (only if shapes are right) every reference: flags, objects, items, characters, locations, marks (within the right location), edges, dialogue nodes, string IDs in every language, and asset files. It also checks marks on the walkway and layer sizes. It checks that every object can be clicked in every state it can be seen in: if its colour isn't painted in the object mask, each such state needs a sprite, and a missing one is an error. Placement conditions are reference-checked like any other condition. It warns about unused assets, flags, strings and untakeable edges.

`check.solve(P)` is tier 2: a breadth-first search over abstract states. A state is room, inventory, flags, object states, NPC rooms, open conversation, and only the said-lines some condition or `once` option reads; player position is ignored. It reports whether the ending flag is reachable (with the shortest route), unreachable rooms, unobtainable items, and softlocks. A softlock is a reachable state that can't reach the ending; walking the graph backwards from ending states finds them, and the report includes a shortest repro path. `max_states` caps the search on big games. A capped search counts the states it never expanded as possibly fine, so the softlocks it reports are certain ones, and not finding the ending is only a warning.

`check.run_all` combines both for the UI. Release export refuses to run unless it's clean.

### AI

`ai.py` keeps every model optional and behind one function per feature, chosen in the `ai` section of the user's `settings.yaml` (edited on the AI assets page; see [Where things live](#where-things-live)): `none`, `onnx` (local, via ONNX Runtime), or `remote` (HTTP). Libraries are imported only when a feature is used.

**Rule authoring** (`author`). The model receives a context slice (ID lists plus only the sheets the intent mentions) and must answer in the patch schema. `onnxruntime-genai` guidance and OpenAI-style `json_schema` both constrain the output, with `add_rule` targets restricted to real IDs. Each patch is applied to a copy and run through the Check. New errors go back to the model for up to two repair passes before the user sees anything. An `ask` op becomes tappable options in the UI. Accepting a patch is one undo step.

**Voices** (`voice_all`) write `voice/<line id>.wav`, using Piper locally or a cloning server remotely with the character's reference WAV. A hash of text plus voice settings means only changed lines regenerate.

**Art and sprite sheets** use Stable Diffusion (ONNX via `optimum`, or remote). Sheets are generated as one 8-direction grid. `process_sheet` slices it, keys out the background, aligns every frame's feet to one pivot, and optionally quantizes to one shared palette. `regen_frame` inpaints a single cell so the others act as the reference.

**Music and SFX** are remote only and marked experimental.

### Export and the bundle

`spec.export` writes `<id>.zip` (release) or `<id>-dev.zip` (dev) into the exports folder. `header.json` stays in the clear: spec version, game ID, an `encrypted` flag, and the file index. Every other file is stored uncompressed and, in release builds, encrypted on its own with AES-256-GCM as `nonce(12) + ciphertext + tag`, so the engine can stream single assets. The key is generated once per game into `<project>/.key` (gitignored) and compiled into that game's engine build. `engine/build.rs` stores it XOR-masked with random bytes, so it isn't in the binary as a plain hex string. This deters casual extraction; a determined user can still pull the key out with a debugger.


## Engine

Build and run it with `cargo run --release -- path/to/game.zip`. A release bundle needs the key at build time: `GAME_KEY=<hex> cargo build --release` (in PowerShell, `$env:GAME_KEY="<hex>"; cargo build --release`).

`boot()` reads `header.json`, parses every spec file into `Spec` (`spec::load`), builds the start state, and starts the loader thread and audio. `main()` then loads the character sheets and icons, enters the start room, and runs the frame loop:

```
pump (upload one finished room) -> pump_audio (play finished sounds) -> update (timers, walking,
arrivals, queue, menus, NPC placement, fades, ending) -> keys -> input -> draw
```

`G` holds everything at runtime: the spec, the save-able `State`, the action queue, speech, the open menu, character positions/paths/facings, the selected verb/item, textures, loaded rooms, and audio (`None` without a sound device, in which case the game runs silent). `logic.rs` functions take `&mut G` and call back into `main.rs` for the physical side (`walk_to`, `move_char`, `play_music`, `room_changed`), which is why it imports `crate::G`.

### Assets and memory

Character sheets and inventory icons load once at startup. Rooms stream:

- On entering a room, `room_changed` keeps it plus its neighbours on the map, drops every other room's textures, and sends missing rooms to the loader thread.
- The thread reads, decrypts and decodes the images, and cuts walk-behinds out of the background.
- `pump` uploads at most one room per frame to the GPU and bakes the walk grid. It keeps CPU copies of the object sprites, which are small, so clicks can test their pixels. macroquad's own async loading doesn't truly run in the background on desktop, hence the thread.

While the new room isn't ready, the screen stays faded and the queue waits.

### Walking, drawing and hit-testing

The walkway mask is baked into a 4-pixel grid. Paths are a breadth-first search to the walkable cell nearest the click, then string-pulled into straight segments. Depth scales both size and speed.

Each frame draws the background, then `scene()`: the objects that are here, the characters and the walk-behinds, in the draw order described under [Game semantics](#game-semantics). The sort key is layer, then (in the depth layer) bottom edge, then listed order. The bottom edge is the sprite's bottom, the feet, or the walk-behind's `baseline`. On a tie, objects come before characters, and characters before walk-behinds.

`target_at` hit-tests the same list front to back, then falls back to the mask colour for objects painted into the background. Walk-behinds never take clicks, and neither does the player. Locations keep their objects in an `IndexMap`, so the listed order survives loading. Everything is drawn at the game's own resolution, with the interface's panel (if it has one) underneath, scaled and letterboxed into the window. `View` is that transform.

### The interface

`game.yaml`'s `interface` picks how the player gives commands, and the style decides which other settings apply. `ui.rs` holds all of it: the mouse, layout, input and drawing for each style.

| Setting | Values | Used by |
| --- | --- | --- |
| `style` | `bar`, `ring` | — |
| `labels` | `words`, `icons`, `captioned` | bar only (the Check rejects it on the ring) |
| `icons` | verb → image | the bar's icon modes, and the ring |
| `speech` | `top`, `over_speaker` | both |

- **Bar.** The classic verb bar and inventory under the scene. Left-click uses the selected verb (walk unless another is picked), and right-click looks. `words` is the SCUMM-style two-row bar with the sentence line ("Use Key with Door") on top, 64 px tall. `icons` is a single 34 px row, and `captioned` (46 px) adds each verb's name under its icon. The icon modes show the sentence by the cursor instead.
- **Ring.** The scene fills the window. A click walks, or with an item picked, uses it on what's clicked. Holding the button on something for a quarter of a second (or dragging from it) opens a ring of every verb but walk; let go on one to do it, or off the ring to cancel. A right-click opens and closes the inventory, an overlay in the middle of the scene; with an item picked, it puts the item back instead. In the inventory, a click picks an item up (or uses the picked one on it), holding gives the item's own verbs, and a click outside the panel puts the inventory away.
- **Speech and dialogue, both styles.** Spoken lines appear in the speaker's colour, either at the top of the screen like a subtitle or over the speaker's head. A conversation's options sit at the bottom: in the bar's panel, growing up over the scene if they don't fit, and over the scene with the ring.

Icons are drawn at 24×24 game pixels, smoothed rather than pixel-sharp since they're scaled to fit. 48×48 images stay sharp in the default 2× window. A verb without an icon shows its name in the icon's place, and the Check warns about it.

Both styles end in the same calls (`click_target`, `logic::click_item`, `logic::choose`), so they play by the same rules. The engine tests play the demo through `ui::input` in every style (see [Tests](#tests)).

**Autoplay** is a dev tool for checking the interface by eye. `PNC_AUTOPLAY=<script>` drives the mouse from a script instead of the real one, and its `shot FILE` command saves a screenshot. The commands are listed on `ui::Autoplay`. Coordinates are game pixels, so scripts don't depend on the window size.

### Controls, audio and saves

The verbs come from `game.yaml` verbs, with labels from `verb_<verb>` strings. Inventory clicks follow `click_item` (see [Game semantics](#game-semantics)). A click skips the current line. `[` and `]` change music volume.

Music streams through kira with crossfades on a music bus; effects and voice lines play on an sfx bus. An audio thread reads and decrypts every sound, and decodes effects in full, so the frame loop never waits on audio. Decoded effects are kept for replays; voice lines aren't, since each is said about once.

### The menu and saving

**Esc**, or the ☰ button in the scene's top-right corner, opens the menu (`ui::Pause`) and pauses the game: the main loop stops calling `update()`, so no time passes. Its pages:
- **main:** Resume, Save game, Load game, Quit;
- **save:** six numbered slots;
- **load:** Autosave, Quicksave and the six slots, each showing its room and how long ago it was saved.

Esc or a right-click goes back a page, then closes the menu.

- **Saving** waits until the game is waiting for the player (`can_save`): no line, cutscene, conversation or room change under way. A save holds the state, not whatever was mid-way. Until then Save game is greyed out with a note saying why, and F5 says so instead of failing silently.
- **Autosave** is written after entering a room, once its enter rule (any cutscene) has played out. **F5** quicksaves, and **F9** quickloads.
- **Loading** (`load_slot`) drops everything under way, including a room change, and puts the state back.

A save file is `spec::Save`: the state manifest plus `saved_at` (seconds since 1970), as JSON. Release builds encrypt it with the game key, so it's also tamper-checked. On loading, `migrate` brings it up to date with the game, so saves survive game updates.

Saves go in `<app data>/PointAndClick/<game id>/saves/<slot>.sav` (`spec::saves_dir`): `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, `$XDG_DATA_HOME` or `~/.local/share` elsewhere. `PNC_SAVES` overrides it. Never next to the game, which may be installed read-only.

The menu's words, and the engine's own messages ("Saved."), can be translated: a game that defines `ui_resume`, `ui_saved` and the rest in its strings gets its own text, and English otherwise. `spec.RESERVED` lists them, so the Check doesn't flag them as unused.


## Common changes

**Adding a condition or action type** touches each layer once, in this order:

1. Schema: add a `oneOf` entry under `cond` or `action` in `spec_v0.1.yaml`.
2. `spec.py`: add it to `CONDS` or `ACTS` with field kinds; pickers and reference checks come for free.
3. `app.py`: add a prose template to `COND_TXT` or `ACT_TXT`.
4. `check.py`: handle it in `ok()` or `run()` if it affects reachability.
5. `spec.rs`: add the enum variant (field names as in the schema).
6. `logic.rs`: handle it in `ok()` or `run_queue()`.
7. `tests/parity/scenarios.yaml`: add a scenario that uses it (extend `projects/workshop` if needed), and run both test suites.

**Adding a verb** needs no code: add it to `game.yaml` verbs (and optionally a `verb_<name>` string).

**Changing the spec shape** means bumping `SPEC_VERSION` in both `spec.py` and `spec.rs`, adding a new schema file, and extending `migrate` (both apps) so old saves still load.


## Tests

```bash
uv pip install -r editor/requirements-dev.txt
uv run pytest editor/tests      # editor side
cd engine && cargo test         # engine side
```

- **`editor/tests/test_check.py`** runs the Check on the demo (clean, seven-step route), then on copies broken one way each, and expects the error that names the problem. The broken copies cover a typo'd flag, a mark off the walkway, missing text, an unknown field, a misplaced item-on-item rule, a bad dialogue `goto`, an unpainted hotspot, a softlock and an unreachable ending. It also checks that a capped search reports no false errors, and that spec files round-trip as UTF-8. Every copy is edited in memory; the demo on disk is never changed.
- **The parity scenarios** (`tests/parity/scenarios.yaml`) are move lists in the solver's own move labels, plus the state they must end in, and optionally moves that must then be `blocked` (such as taking an object that isn't there yet). `editor/tests/test_parity.py` plays each through `check.moves`, so every move has to be one the solver offers. `engine/src/tests.rs` plays each through the functions `input()` uses (`click_item`, `click_target`, `choose`), with time passing through the real `update()`. Only the window side is stubbed there: rooms "load" with no textures and a fully walkable grid, and there's no audio. `tests/parity/projects/workshop` is a one-room, YAML-only game made to exercise the trickier semantics.
- **The interface tests** (also in `engine/src/tests.rs`) play the demo through `ui::input` itself, with a scripted mouse over the rooms' real masks. The ring style takes the key from the ring, uses it from the inventory on the door, and picks a dialogue option at the bottom. The bar does the same in each of `words`, `icons` and `captioned`. So layout and hit-testing are covered as well as the rules. Another test sets the demo's cake down over the key. It checks that the draw order and the click follow the cake's layer, and that a hidden cake lets the key behind it be clicked.
- **The saving tests** (also in `engine/src/tests.rs`) use the menu with the same scripted mouse. They save to a slot, change things, and load the slot back. They also check that saving waits out a conversation, and that entering a room autosaves and the autosave loads back. Each test uses a temporary saves folder; the other tests have none, so they never write saves.
- **`editor/tests/test_config.py`** checks where the editor keeps things. Nothing defaults to inside the editor, folder settings persist and clear, `PNC_PROJECTS` overrides without changing the setting, copying an example never overwrites, and moving old projects renames on a clash. AI settings live in `settings.yaml`. `test_app.py` also drives the Settings page from a first run: it sets the projects folder, copies the demo, and lands in it.
- **`editor/tests/test_app.py`** runs the editor headlessly with Streamlit's AppTest, against copies of the demo and the workshop in a temporary folder. It checks that every page renders and that viewing pages changes no file, even for a sparse project. It also checks that Undo steps back one edit at a time, and that the Interface section writes only the chosen style's settings. It creates a project and adds a location from ordinary names (checking that every page of the new, empty project works), and checks that unusable or taken names get a message. AppTest runs the script without rendering it, so it can't see layouts or click the location page's picture, a custom component.
- **`editor/tests/test_browser.py`** covers that part. It serves the editor for real on a copy of the demo, and drives it with Playwright through the Edge that ships with Windows (falling back to Chrome, or to Playwright's own Chromium after `playwright install chromium`). It's skipped where none of these is available. It checks that:
  - every page renders without an exception or a browser error;
  - the preview draws objects (by reading the cake's pixels);
  - clicking the picture places a mark, with the name box beside the picture rather than under it;
  - three clicks and Fill paint a polygon into the walkway.

  One environment trap: if `SSLKEYLOGFILE` is set, some Windows Python builds (uv's among them) abort as soon as anything creates an SSL context, so the test removes it from the server's environment.

When the two apps disagree about a scenario, one of the suites fails. Each semantic fix above was checked by reintroducing the old behaviour and watching a test fail.


## Status and known gaps

**Verified.** The engine builds with no warnings (rustc 1.93) and runs the demo from both dev and release (encrypted) exports. Both test suites pass. Each interface style was also checked by eye from autoplay screenshots, and the editor from browser screenshots. Not yet tested: the real AI backends.

**Other limits:**

- **Comments.** Saving from the editor rewrites YAML, so hand-written comments in spec files are lost (the demo's comments survive until it's first edited).
- **Font.** macroquad's default font is ASCII-focused; load a TTF for localized text.
- **WASM.** macroquad builds to WASM, but the loader thread doesn't exist there; a WASM build needs `room_changed` to load synchronously instead.
- **Fixed engine choices.** Dialogue options that don't fit the panel grow up over the scene. The walk grid is 4 px. The ring opens after a 0.25 s hold or a 6 px drag.
- **Long inventories.** Neither the bar nor the ring's panel scrolls yet. Beside the verbs at 640 px wide, the bar fits about 9 items with words or captions and 14 with icons; the ring's panel grows in rows of 8.

**The plan's open questions, as decided here:**

- **Conditions:** typed flag/state/inventory/location/said conditions, AND-ed. There's no expression language yet.
- **Verb UI:** chosen per game in `game.yaml`: a verb bar (words, icons or captioned icons) or a verb ring with a right-click inventory.
- **Resolution:** set per game in `game.yaml`.
- **Save location:** the player's app-data folder, per game (see [The menu and saving](#the-menu-and-saving)).
