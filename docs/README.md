# Point & Click Maker

A Python editor that builds and validates point-and-click adventure games, and a Rust engine that plays them. The two share nothing but an exported zip. See [DEVELOPMENT.md](DEVELOPMENT.md) for how it all fits together.

## Quick start

```bash
# editor (or: pip install -r editor/requirements.txt)
uv venv
uv pip install -r editor/requirements.txt
uv run streamlit run editor/app.py
```

The first time, the editor opens on its Settings page. Copy the **demo** example to get a game to look at, or create a new one in the sidebar. Your games are kept in `Documents/Point and Click Maker/projects`, and exports go in `.../exports`; both can be changed on the Settings page. Every edit saves as you make it.

To play a game, export it on the **Check & export** page. The editor shows the command to run, with the zip's real path:

```bash
cd engine
cargo run --release -- "<path the editor shows>/demo-dev.zip"
```

Release builds are encrypted per game. Export the release build, then build the engine with the key it shows:

```bash
GAME_KEY=<hex> cargo run --release -- "<path>/demo.zip"          # bash
$env:GAME_KEY="<hex>"; cargo run --release -- "<path>/demo.zip"  # PowerShell
```

## Playing

Each game picks its interface on the editor's Game page (it's stored in `game.yaml` as `interface`):

- **Verb bar** (words, icons, or icons with captions). Left-click uses the selected verb and right-click looks. To use one item on another, pick Use, click the first item, then click the second. Clicking the first item again uses it on its own.
- **Verb ring.** Click to walk. Hold the mouse on something to open a ring of verbs, and let go on one. Right-click opens the inventory: click an item to pick it up, then click what to use it on (or the same item again to use it on its own). Right-click again puts the item back.

Spoken lines appear at the top of the screen (or over the speaker), and dialogue options at the bottom. The demo uses the ring.

**Esc** (or the ☰ button in the top corner) pauses and opens the menu: Resume, Save game, Load game, Quit. There are six save slots. The game also autosaves each time you enter a room, and **F5** / **F9** quicksave and quickload. Saving waits until nothing is happening, so not mid-conversation or mid-cutscene. Saves go in your app-data folder, under `PointAndClick/<game id>/saves`: `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, `~/.local/share` on Linux. `[` and `]` change the music volume.

## Tests

```bash
uv pip install -r editor/requirements-dev.txt
uv run pytest editor/tests      # the Check, the solver side of the parity scenarios, and the editor
                                # (headless, plus browser tests through Windows' Edge when available)
cd engine && cargo test         # the engine side of the same scenarios, the interface and saving
```
