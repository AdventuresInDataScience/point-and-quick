"""Project I/O, the shared spec tables, patch operations and the bundle exporter.
A project is held in memory as one plain dict (see load) mirroring the folder on disk."""
import copy, json, os, zipfile
from pathlib import Path
import yaml
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

SPEC_VERSION = "0.1"
SCHEMA = yaml.safe_load((Path(__file__).parent / "schema/spec_v0.1.yaml").read_text())

# Field kinds for every condition and action type ("?" = optional). The editor's pickers, the
# Check's reference tests and the rule-card prose are all driven by these two tables.
CONDS = {
    "flag": {"flag": "flag", "is": "bool"},
    "state": {"object": "object", "is": "state"},
    "has": {"item": "item", "is": "bool"},
    "at": {"character": "character", "location": "location", "is": "bool"},
    "said": {"line": "line", "is": "bool"},
}
ACTS = {
    "say": {"line": "line", "who": "character?"},
    "set_flag": {"flag": "flag", "value": "bool"},
    "set_state": {"object": "object", "state": "state"},
    "give": {"item": "item"},
    "remove": {"item": "item"},
    "move": {"character": "character", "location": "location", "mark": "mark?"},
    "char_state": {"character": "character", "state": "text"},
    "go": {"edge": "edge"},
    "walk": {"mark": "mark"},
    "talk": {"character": "character", "node": "node?"},
    "play_music": {"file": "file", "fade": "num?"},
    "play_sound": {"file": "file"},
    "wait": {"seconds": "num"},
}
RESERVED = {"str_the_end"}          # strings the engine uses by name (plus verb_<verb> labels)


def read(path): return yaml.safe_load(Path(path).read_text()) or {}


def dump(path, data):
    path = Path(path); path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(yaml.safe_dump(data, sort_keys=False, default_flow_style=None, allow_unicode=True, width=110))


def load(root):
    """Folder -> project dict. Keys mirror the spec files; dialogue is {character: {file stem: sheet}}."""
    root = Path(root)
    P = {"root": str(root), "game": read(root / "game.yaml"),
         "map": read(root / "map.yaml") if (root / "map.yaml").exists() else {"edges": []},
         "locations": {p.parent.name: read(p) for p in sorted(root.glob("locations/*/sheet.yaml"))},
         "objects": {p.stem: read(p) for p in sorted(root.glob("objects/*.yaml"))},
         "characters": {p.stem: read(p) for p in sorted(root.glob("characters/*.yaml"))},
         "strings": {p.stem: read(p) for p in sorted(root.glob("strings/*.yaml"))},
         "dialogue": {}}
    for p in sorted(root.glob("characters/*/dialogue/*.yaml")):
        P["dialogue"].setdefault(p.parent.parent.name, {})[p.stem] = read(p)
    return P


def save(P):
    root = Path(P["root"])
    dump(root / "game.yaml", P["game"]); dump(root / "map.yaml", P["map"])
    for i, d in P["locations"].items(): dump(root / f"locations/{i}/sheet.yaml", d)
    for i, d in P["objects"].items(): dump(root / f"objects/{i}.yaml", d)
    for i, d in P["characters"].items(): dump(root / f"characters/{i}.yaml", d)
    for lang, d in P["strings"].items(): dump(root / f"strings/{lang}.yaml", d)
    for c, files in P["dialogue"].items():
        for f, d in files.items(): dump(root / f"characters/{c}/dialogue/{f}.yaml", d)


def blank(kind, i):
    """A new, minimal sheet of the given kind."""
    return {"locations": {"id": i, "name": "", "background": f"locations/{i}/bg.png", "scale": [1, 1], "marks": {},
                          "hotspots": {}, "exits": {}, "npcs": [], "walkbehinds": [], "rules": []},
            "objects": {"id": i, "name": "", "state": "default", "states": {"default": ""}, "rules": [], "fallback": {}},
            "characters": {"id": i, "name": "", "speed": 120, "text_color": "#ffffff", "animations": {}, "rules": [], "fallback": {}},
            }[kind]


def new_project(root, gid):
    root = Path(root)
    P = {"root": str(root), "map": {"edges": []}, "locations": {}, "objects": {}, "dialogue": {},
         "characters": {"player": blank("characters", "player")}, "strings": {"en": {"str_title": gid}},
         "game": {"spec_version": SPEC_VERSION, "id": gid, "title": "str_title", "resolution": [640, 360], "lang": "en",
                  "player": "player", "verbs": ["walk", "look", "talk", "use", "take"], "flags": {}, "ending": "",
                  "start": {"location": "", "mark": "", "inventory": [], "characters": {}}, "defaults": {}, "combinations": []}}
    save(P)
    return P


def delete(P, kind, i):
    """Remove an entity and its sheet file (assets are left alone)."""
    P[kind].pop(i)
    path = Path(P["root"]) / {"locations": f"locations/{i}/sheet.yaml", "objects": f"objects/{i}.yaml", "characters": f"characters/{i}.yaml"}[kind]
    path.unlink(missing_ok=True)


def assets(P):
    """Every non-YAML file in the project, as root-relative posix paths (hidden files skipped)."""
    root = Path(P["root"])
    rel = [p.relative_to(root) for p in sorted(root.rglob("*")) if p.is_file()]
    return [r.as_posix() for r in rel if r.suffix != ".yaml" and not any(s.startswith(".") for s in r.parts)]


def ids(P):
    """All referenceable IDs, by kind. Items are objects with an icon (they can be carried)."""
    g = P["game"]
    return {"flag": list(g.get("flags", {})), "object": list(P["objects"]), "character": list(P["characters"]),
            "item": [o for o, d in P["objects"].items() if d.get("icon")], "location": list(P["locations"]),
            "edge": [e["id"] for e in P["map"].get("edges", [])], "verb": g.get("verbs", []),
            "line": list(P["strings"].get(g.get("lang", "en"), {})), "file": assets(P),
            "mark": sorted({m for l in P["locations"].values() for m in l.get("marks", {})})}


def name(P, i):
    """Display name of any entity ID, falling back to the ID itself."""
    d = P["objects"].get(i) or P["characters"].get(i) or P["locations"].get(i) or {}
    return P["strings"].get(P["game"].get("lang", "en"), {}).get(d.get("name", ""), i)


def files(P):
    """(file, schema kind, data) for every spec file — what check.py validates."""
    yield "game.yaml", "game", P["game"]
    yield "map.yaml", "map", P["map"]
    for i, d in P["locations"].items(): yield f"locations/{i}/sheet.yaml", "location", d
    for i, d in P["objects"].items(): yield f"objects/{i}.yaml", "object", d
    for i, d in P["characters"].items(): yield f"characters/{i}.yaml", "character", d
    for c, fs in P["dialogue"].items():
        for f, d in fs.items(): yield f"characters/{c}/dialogue/{f}.yaml", "dialogue", d
    for lang, d in P["strings"].items(): yield f"strings/{lang}.yaml", "strings", d


def blocks(P):
    """Every piece of logic as (where, location it runs in or None, conditions, actions)."""
    for i, r in enumerate(P["game"].get("combinations", [])):
        yield f"game.yaml combinations[{i}]", None, r.get("if", []), r.get("do", [])
    for kind in ("locations", "objects", "characters"):
        for i, d in P[kind].items():
            for j, r in enumerate(d.get("rules", [])):
                yield f"{kind}/{i} rules[{j}]", i if kind == "locations" else None, r.get("if", []), r.get("do", [])
    for i, l in P["locations"].items():
        for j, n in enumerate(l.get("npcs", [])): yield f"locations/{i} npcs[{j}]", i, n.get("if", []), []
    for c, fs in P["dialogue"].items():
        for f, d in fs.items():
            loc = None if f == "global" else f
            for nid, n in d.get("nodes", {}).items():
                yield f"characters/{c}/dialogue/{f} {nid}", loc, [], n.get("do", [])
                for j, o in enumerate(n.get("options", [])):
                    yield f"characters/{c}/dialogue/{f} {nid} options[{j}]", loc, o.get("if", []), []


def dialogue(P, char, loc):
    """Dialogue lookup: the location's file first, the character's global file as fallback.
    Returns (start node, merged nodes). The engine does exactly the same merge."""
    start, nodes = "", {}
    for f in ("global", loc):
        d = P["dialogue"].get(char, {}).get(f, {})
        start = d.get("start") or start
        nodes.update(d.get("nodes", {}))
    return start, nodes


def new_state(P):
    """The start state as a full manifest: declared flag defaults and object states filled in.
    Same as the engine's new_state/migrate, and the same shape as a save file."""
    g = P["game"]; s = copy.deepcopy(g.get("start", {}))
    s["spec_version"] = SPEC_VERSION
    s["flags"] = {**g.get("flags", {}), **s.get("flags", {})}
    s["objects"] = {**{o: d.get("state", "") for o, d in P["objects"].items()}, **s.get("objects", {})}
    for k, v in (("inventory", []), ("characters", {}), ("said", []), ("music", "")): s.setdefault(k, v)
    return s


def target_kind(P, i):
    return next(k for k in ("objects", "characters", "locations") if i in P[k])


def apply_patch(P, ops):
    """Apply LLM/editor patch operations to a copy of the project. Undo = keep the old copy."""
    P = copy.deepcopy(P)
    for op in ops:
        k = op["op"]
        if k == "add_rule": P[target_kind(P, op["target"])][op["target"]].setdefault("rules", []).append(op["rule"])
        elif k == "add_combination": P["game"].setdefault("combinations", []).append(op["rule"])
        elif k == "set_flag_default": P["game"].setdefault("flags", {})[op["flag"]] = op["value"]
        elif k == "set_string": P["strings"].setdefault(op.get("lang") or P["game"].get("lang", "en"), {})[op["id"]] = op["text"]
        elif k == "add_mark": P["locations"][op["location"]].setdefault("marks", {})[op["mark"]] = op["pos"]
        elif k == "link_exit":
            e = op["edge"]; P["map"].setdefault("edges", []).append(e)
            x = {"hotspot": op["hotspot"], **({"state": op["state"]} if op.get("state") else {})}
            P["locations"][e["from"]].setdefault("exits", {})[e["id"]] = x
    return P


def export(P, out_dir, encrypted=True):
    """Write the game zip. header.json stays in the clear (spec version + file index); every other
    file is stored uncompressed and, for release builds, AES-256-GCM encrypted on its own as
    nonce(12) + ciphertext + tag, so the engine can stream single assets.
    The key is made once per game, kept in <project>/.key, and compiled into the engine build."""
    root = Path(P["root"]); g = P["game"]
    rel = [p.relative_to(root) for p in sorted(root.rglob("*")) if p.is_file()]
    names = [r.as_posix() for r in rel if not any(s.startswith(".") for s in r.parts)]
    key = b""
    if encrypted:
        kf = root / ".key"
        if not kf.exists(): kf.write_text(AESGCM.generate_key(256).hex())
        key = bytes.fromhex(kf.read_text().strip())
    out = Path(out_dir) / f"{g['id']}{'' if encrypted else '-dev'}.zip"
    out.parent.mkdir(parents=True, exist_ok=True)
    header = {"spec_version": g["spec_version"], "game": g["id"], "encrypted": encrypted, "files": names}
    with zipfile.ZipFile(out, "w", zipfile.ZIP_STORED) as z:
        z.writestr("header.json", json.dumps(header, indent=1))
        for n in names:
            data = (root / n).read_bytes()
            if key:
                nonce = os.urandom(12)
                data = nonce + AESGCM(key).encrypt(nonce, data, None)
            z.writestr(n, data)
    return out, key.hex()
