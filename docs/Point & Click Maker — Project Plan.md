# Point & Click Maker — Project Plan

Nov 10, 2026 · @Malhar Patel

## Overview

The project is two apps joined by one contract: a Python editor that builds and validates a game, and a Rust engine that runs it. Nothing passes between them except the exported zip of spec files and assets.

&#91;embedded content: architecture · editor, zip contract, engine\]

AI assists inside the editor only, so shipped games carry none of its weight.

## Project & spec structure

Each game is a folder of small, per-entity spec files plus assets, exported as one encrypted zip once it passes the Check. The spec schema is the real product: both apps follow from it, so it is defined first (JSON Schema) and versioned from day one.

| File | Holds | Notes |
| --- | --- | --- |
| `game.yaml` (global sheet) | Spec version, title, declared flags and defaults, inventory, verb list (look, talk, use…), item-on-item combinations, default responses | Flags must be declared here so the Check catches typos like `guard_left` vs `guard_gone` |
| `map.yaml` | Every location-to-location connection: from, to, spawn mark | Single source of truth for exits; also drives the engine's preloading |
| `locations/<id>/sheet.yaml` | Location rules, default music, which hotspot triggers each map edge, NPC placements | One per location |
| `locations/<id>/` images | Background image, walkway mask, object mask (indexed colour, one colour per object ID), depth/scale mask, walk-behind masks | Painted image layers |
| `objects/<id>.yaml` | Object rules, named visual states (door open/closed), sprites per state | Stateful art lives here, never baked into backgrounds |
| `characters/<id>.yaml` | Character rules, animation sets, voice settings + reference WAV | Current location is global state, so NPCs can move between rooms |
| `characters/<id>/dialogue/` | `global.yaml` plus `<location>.yaml` files | Lookup: named-location lines first, then global fallback |
| `strings/<lang>.yaml` | All player-facing text, keyed by stable IDs | Localization-ready from the start |

**NPC placements** are data, not pixels: a list per location of character, position (anchored at the feet), facing, starting state, and an optional condition ("guard is here unless `guard_bribed`"). In the editor they appear as a draggable layer.

**Named marks** are named points on a location (`door_mark`, `east_entrance`), with or without a character attached. Rules, exits and spawns reference them by name.

**State manifest and save files.** All changeable game state lives in one manifest schema: current location and mark, inventory, flags, object states, character locations and states, and conversation state. The global sheet's starting state is written in this schema, and a save file is simply a snapshot of it. Saves are versioned with the spec, so the engine can detect and migrate old saves after a game update, and can be checksummed or encrypted to deter hand-editing.

**Encrypted bundle.** The exported zip encrypts each file individually (e.g. AES-256-GCM: Python's `cryptography` in the editor, the `aes-gcm` crate in the engine), so the engine can still stream single assets without decrypting the whole archive. A small unencrypted header carries the spec version and a file index. The key is set per game at export and compiled into that game's engine build. This deters casual asset extraction and is standard practice commercially, but a determined user can still pull the key from the binary. The editor keeps an unencrypted dev mode for fast iteration.

## Logic model

Logic is data, not code: every rule reads as "when VERB on TARGET, if CONDITIONS, do ACTIONS". This covers most adventure puzzles, with no embedded scripting language.

- **Ownership convention:** the target owns the interaction. "Use Key on Door" lives on the door's sheet; the key's sheet holds only its own look/pickup lines and a fallback. This prevents the same interaction being defined in two places.
- **Global vs local:** item-on-item combinations, flags, verbs and default responses live in the global sheet; everything tied to a place or thing lives on that entity's sheet.
- **Gated exits:** an exit is gated by the state of the object that triggers it (a locked door), never by a second definition in the map.
- **Visual states:** rules flip an object's named state; the matching sprite follows.
- **Music:** each location sheet sets a default track; a `play_music` action lets rules override it (on enter, if a flag is set, play the tense track). The engine handles crossfades.
- **Stable IDs everywhere:** every location, object, character, mark, flag and dialogue line has a permanent ID. Editing a line regenerates only that line's TTS audio, and localization and "already said this" tracking key off the same IDs.
- **Conversation state is global:** exhausted topics and branches taken persist across locations, so NPCs don't forget conversations when met elsewhere.

## Editor (Python, Streamlit)

The editor is written in Streamlit for ease of coding: forms, file uploads, project lists and rule cards are all quick to build. Its job is to make filling the spec precise without feeling like coding.

**Location UI as layers.** A user adds a location, then its image, then works on transparent layers stacked on top: walkway, objects, characters/marks, depth, walk-behinds. They drag a character or object onto the layer, snapshot its position and name it, creating a mark.

- Anchor snapshots at the character's feet, not the image corner.
- Show the walkway layer faintly underneath so users see whether a mark is on walkable ground.
- Preview characters at the scale the depth mask gives them at that spot.

**Rule cards.** Every rule renders in a readable canonical form, e.g. *When the player **uses** Key **on** Door, if Door is locked → unlock Door, play `creak.ogg`.* Each bold token is a picker bound to real IDs, so rules read like prose but can't reference things that don't exist.

**LLM-assisted authoring.** Users describe intent loosely ("the key opens the door, but only after the guard leaves"). The LLM turns it into a structured patch shown as a rule card to accept or tweak.

1. The LLM emits patch operations (`add_rule`, `set_flag_default`, `link_exit`) against existing IDs, never whole YAML files. Edits stay small and reviewable, and undo is trivial.
2. Output uses constrained decoding (JSON schema or grammar), so invalid structure can't be produced.
3. Every patch runs through the Check; errors go back to the model for an automatic repair pass before the user sees it.
4. Where intent is ambiguous (leaves the room, or leaves for good?), the assistant asks with tappable options rather than guessing.
5. Only the relevant slice of the project goes into context, so small local models stay effective.

**Streamlit caveat:** drag-and-drop on a canvas is Streamlit's weak spot. The layer editor will likely need `streamlit-drawable-canvas` or a small custom component; if that becomes painful, NiceGUI is a similarly easy Python framework with better interactive canvas support.

## AI layer

All AI runs through ONNX Runtime behind a plugin interface, so models are optional downloads that can be swapped, or pointed at a remote API, without touching the editor. Generative models are several GB, so none are bundled.

| Feature | Approach | Maturity in ONNX |
| --- | --- | --- |
| Scripting / rule authoring | Small local LLM via onnxruntime-genai with constrained output | Good |
| Dialogue voices | TTS per character (e.g. Piper, Kokoro) | Good |
| Voice customisation | Zero-shot cloning from a provided WAV plus TTS (e.g. F5-TTS, OpenVoice ports); reference WAV stored per character so lines regenerate when the script changes | Varies by model |
| Location and object art | Stable Diffusion via ONNX | Good, but large models |
| Character animation | Single diffusion call producing the whole 8-direction frameset as a grid | Workable with grid-trained models or LoRAs |
| Music and SFX | Generative audio models | Least mature; treat as experimental |

**Sprite-sheet post-processing pipeline:** auto-slice the grid, remove backgrounds, align every frame's pivot to the feet so walks don't jitter, and optionally quantize the palette for a consistent look. Users can regenerate a single bad frame using the others as reference, rather than rerolling the whole sheet.

## The Check

A project only exports to zip once it passes the Check, which runs in two tiers. The same Check also validates every LLM patch during authoring.

**Tier 1, validation:** schema conformance and spec version; missing or unused assets; references to undeclared flags, objects, characters, marks or dialogue lines; map edges whose hotspot or spawn mark doesn't exist; marks placed off the walkway; duplicate IDs; text missing from string tables.

**Tier 2, puzzle solver:** search the game's state graph (location, inventory, flags, object states) from the start state to detect unreachable locations, items that can never be obtained, softlocks where the player can get stuck, and whether the ending is reachable at all. Few tools do this, and it's a standout feature.

## Engine (Rust, macroquad)

The engine loads the zip, parses the spec into typed data, and runs the game in macroquad's single async loop. It is written in a deliberately Pythonic style so AI-generated code stays easy to read.

**Style guide**

- `mut`, `HashMap` and `Vec` everywhere is fine.
- Refer to things by ID strings (`"guard"`), not references, and clone strings freely. This sidesteps most borrow-checker fights and matches the ID-based spec.
- Use `f32`, not `f64`: macroquad's positions, `Vec2` and colours are all `f32`.
- The two exceptions worth learning: structs with `#[derive(Deserialize)]` that mirror the YAML (serde does all the parsing and catches bad fields at load time), and serde-tagged enums for actions and conditions, handled with `match` (a dict with a `type` key, but compiler-checked).

**Loading and memory**

- Read files from the zip with the `zip` crate into bytes, decrypt them with the build's key, then `Texture2D::from_file_with_format(&bytes, None)`.
- Keep only the current location's assets plus its neighbours in the map graph; drop the rest.
- macroquad's async loading isn't truly in the background on desktop, so read and decode on a `std::thread`, and do only the GPU upload on the main thread, a few per frame or during room transitions.

**Runtime systems:** walking on a navmesh or grid baked from the walkway mask, depth scaling, walk-behind occlusion, verb UI and hit-testing via the indexed object mask, dialogue trees with the location-then-global lookup, save/load of global state, and cutscene sequencing.

**Audio:** macroquad's built-in audio is basic. Use the `kira` crate for streamed music, crossfades and volume buses.

**Bonus:** macroquad builds to WASM, so games can also run in a browser.

## Build order

Build the contract first and the AI last, so each step is testable against the one before it.

1. **Schema v0.1:** JSON Schema for the global sheet, map, location, object and character sheets, and dialogue.
2. **Hand-written test game:** two locations, one locked door, one key, one NPC with a short conversation, written directly in YAML.
3. **Engine MVP:** load the zip, show a location, walk on the walkway, hit-test objects, run rules, change rooms, play music.
4. **Check, tier 1:** validation and reference checking; export to the encrypted zip only on pass, plus an unencrypted dev export.
5. **Editor MVP (Streamlit):** project create/load, asset upload, layered location editor with marks, rule cards with pickers.
6. **Engine depth:** depth scaling, walk-behinds, dialogue trees, save/load, adjacent-room preloading, `kira` audio.
7. **Check, tier 2:** the puzzle solver.
8. **LLM authoring:** patch operations, constrained output, the Check-and-repair loop.
9. **Generative assets:** TTS and voice cloning first, then art and sprite sheets, then music and SFX.

## Open questions & risks

| Risk | Mitigation |
| --- | --- |
| Streamlit struggles with drag-and-drop layer editing | Custom canvas component, or switch that screen to NiceGUI |
| Single-call sprite sheets lose consistency across directions | Post-processing pipeline plus per-frame regeneration with reference |
| Model downloads make the editor heavy | Optional, swappable models; remote API fallback |
| Puzzle solver state space explodes on large games | Solve per chapter or region; cap search depth |
| Music and SFX generation quality | Mark as experimental; allow imported audio everywhere |
| Rules defined in two places | Target-owns-interaction convention, enforced by the Check |

- [ ] Expression language for conditions: needed in v1, or only flags and states?
- [ ] Verb UI style: fixed verb bar, verb coin, or contextual single click?
- [ ] Target resolution and aspect ratio for location art.
- [ ] Where save files live on each platform.
