# Point & Click Maker

A Python editor that builds and validates point-and-click adventure games, and a Rust engine that plays them. The two share nothing but an exported zip. See [DEVELOPMENT.md](DEVELOPMENT.md) for how it all fits together.

## Quick start

```bash
# editor (or: pip install -r editor/requirements.txt)
uv venv
uv pip install -r editor/requirements.txt
uv run streamlit run editor/app.py   # open the "demo" project; Check & export -> Export dev build

# engine
cd engine
cargo run --release -- ../editor/exports/demo-dev.zip
```

Release builds are encrypted per game. Export the release build in the editor, then build the engine with the key it shows:

```bash
GAME_KEY=<hex> cargo run --release -- ../editor/exports/demo.zip          # bash
$env:GAME_KEY="<hex>"; cargo run --release -- ../editor/exports/demo.zip  # PowerShell
```

Each game picks its interface on the editor's Game page (it's stored in `game.yaml` as `interface`):

- **Verb bar** (words, icons, or icons with captions). Left-click uses the selected verb and right-click looks. To use one item on another, pick Use, click the first item, then click the second. Clicking the first item again uses it on its own.
- **Verb ring.** Click to walk. Hold the mouse on something to open a ring of verbs, and let go on one. Right-click opens the inventory: click an item to pick it up, then click what to use it on (or the same item again to use it on its own). Right-click again puts the item back.

Spoken lines appear at the top of the screen (or over the speaker), and dialogue options at the bottom. F5 saves, F9 loads, `[` and `]` change the music volume, and Esc quits. The demo uses the ring.

## Tests

```bash
uv pip install -r editor/requirements-dev.txt
uv run pytest editor/tests      # the Check, and the solver side of the parity scenarios
cd engine && cargo test         # the engine side of the same scenarios
```
