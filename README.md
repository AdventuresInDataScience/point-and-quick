# Point & Click Maker

A Python editor that builds and validates point-and-click adventure games, and a Rust engine that plays them. The two share nothing but an exported zip. See [DEVELOPMENT.md](DEVELOPMENT.md) for how it all fits together.

## Quick start

```bash
# editor
pip install -r editor/requirements.txt
streamlit run editor/app.py          # open the "demo" project; Check & export -> Export dev build

# engine
cd engine
cargo run --release -- ../editor/exports/demo-dev.zip
```

Release builds are encrypted per game. Export the release build in the editor, then build the engine with the key it shows: `GAME_KEY=<hex> cargo run --release -- ../editor/exports/demo.zip`.

In the game, left-click uses the selected verb and right-click looks. F5 saves, F9 loads, `[` and `]` change the music volume, and Esc quits.
