"""The Check. Tier 1 validates every file and reference; tier 2 searches the game's state graph
for unreachable rooms, unobtainable items, softlocks and an unreachable ending.
The tier 2 simulator mirrors engine/src/logic.rs — change both together."""
from pathlib import Path
import jsonschema
from PIL import Image
import spec


# ======================= tier 1: validation =======================

def check(P):
    """Return a list of (level, where, message); level is "error" or "warning"."""
    out = []
    E = lambda w, m: out.append(("error", w, m))
    W = lambda w, m: out.append(("warning", w, m))
    g, root, I = P["game"], Path(P["root"]), spec.ids(P)
    langs = P["strings"] or {g.get("lang", "en"): {}}
    used = {"line": set(), "file": set(), "flag": set(), "edge": set()}
    images = {}

    def image(rel):                      # cached; None if missing
        if rel not in images: images[rel] = Image.open(root / rel) if (root / rel).is_file() else None
        return images[rel]

    def ref(where, kind, v, loc=None):
        """Record a use of ID v and report it if it doesn't exist."""
        if v in ("", None): return
        used.setdefault(kind, set()).add(v)
        if kind == "line":
            for lang, tab in langs.items():
                if v not in tab: E(where, f"text '{v}' missing from strings/{lang}.yaml")
        elif kind == "mark" and loc:
            if v not in P["locations"].get(loc, {}).get("marks", {}): E(where, f"unknown mark '{v}' in {loc}")
        elif kind in I and v not in I[kind]:
            E(where, f"unknown {kind} '{v}'" + (" (items need an icon)" if kind == "item" and v in P["objects"] else ""))

    def logic(where, loc, it, table):
        """Check every ID field of one condition/action using the spec tables."""
        for f, kind in table.get(it.get("type"), {}).items():
            v, kind = it.get(f), kind.rstrip("?")
            if v in ("", None): continue
            if kind == "state":
                if v not in P["objects"].get(it.get("object"), {}).get("states", {}): E(where, f"'{it.get('object')}' has no state '{v}'")
            elif kind == "mark": ref(where, "mark", v, it.get("location") or loc)
            elif kind == "node":
                nodes = {n for d in P["dialogue"].get(it.get("character"), {}).values() for n in d.get("nodes", {})}
                if v not in nodes: E(where, f"'{it.get('character')}' has no dialogue node '{v}'")
            else: ref(where, kind, v)

    def rules(where, d, owner, verbs):
        for j, r in enumerate(d.get("rules", [])):
            w = f"{where} rules[{j}]"
            if r.get("verb") not in verbs: E(w, f"verb '{r.get('verb')}' isn't one of {verbs}")
            if r.get("with"):
                ref(w, "item", r["with"])
                # ownership: item-on-item belongs in game.yaml, so it can't be defined on both items
                if owner in I["item"] and r["with"] in I["item"]: E(w, "item-on-item combinations belong in game.yaml")

    # -- schema conformance and spec version
    for where, kind, data in spec.files(P):
        v = jsonschema.Draft202012Validator({**spec.SCHEMA, "$ref": f"#/$defs/{kind}"})
        for e in v.iter_errors(data): E(where, f"{'/'.join(map(str, e.path)) or '(top)'}: {e.message[:200]}")
    if g.get("spec_version") != spec.SPEC_VERSION: E("game.yaml", f"spec_version must be '{spec.SPEC_VERSION}'")
    if out: return out                   # reference checks assume the shapes are right

    # -- IDs match file names and are unique across kinds (they share one target namespace)
    seen = {}
    for kind in ("locations", "objects", "characters"):
        for i, d in P[kind].items():
            if d.get("id") != i: E(f"{kind}/{i}", f"id '{d.get('id')}' doesn't match the file name")
            if i in seen: E(f"{kind}/{i}", f"duplicate id: also a {seen[i][:-1]}")
            seen[i] = kind
    edge_ids = [e["id"] for e in P["map"].get("edges", [])]
    for e in sorted({e for e in edge_ids if edge_ids.count(e) > 1}): E("map.yaml", f"duplicate edge id '{e}'")

    # -- game.yaml
    ref("game.yaml", "line", g.get("title")); ref("game.yaml", "character", g.get("player"))
    ref("game.yaml", "flag", g.get("ending"))
    if not g.get("ending"): W("game.yaml", "no ending flag; the solver can't check the game can be finished")
    st = g.get("start", {})
    ref("game.yaml start", "location", st.get("location")); ref("game.yaml start", "mark", st.get("mark"), st.get("location"))
    for it in st.get("inventory", []): ref("game.yaml start", "item", it)
    for c, p in st.get("characters", {}).items():
        ref("game.yaml start", "character", c); ref("game.yaml start", "location", p.get("location"))
        ref("game.yaml start", "mark", p.get("mark"), p.get("location"))
    for verb, line in g.get("defaults", {}).items(): ref(f"game.yaml defaults.{verb}", "line", line)
    for i, r in enumerate(g.get("combinations", [])):
        for it in r["items"]: ref(f"game.yaml combinations[{i}]", "item", it)

    # -- the interface: each style reads only its own settings
    ui, w = {**spec.UI_DEFAULTS, **g.get("interface", {})}, "game.yaml interface"
    if ui["style"] == "ring" and "labels" in g.get("interface", {}):
        E(w, "'labels' is a bar setting; the ring style doesn't use it")
    for verb, icon in ui.get("icons", {}).items():
        if verb not in I["verb"]: E(w, f"there's an icon for '{verb}', which isn't one of the verbs")
        ref(w, "file", icon)
    # verbs the style shows as icons (the ring leaves out walk: walking is a plain click)
    shown = ([v for v in I["verb"] if v != "walk"] if ui["style"] == "ring"
             else I["verb"] if ui["labels"] in ("icons", "captioned") else [])
    for v in shown:
        if v not in ui.get("icons", {}): W(w, f"verb '{v}' has no icon, so the {ui['style']} shows its name instead")

    # -- map.yaml
    for e in P["map"].get("edges", []):
        w = f"map.yaml {e['id']}"
        ref(w, "location", e["from"]); ref(w, "location", e["to"]); ref(w, "mark", e["spawn"], e["to"])

    # -- locations
    for i, l in P["locations"].items():
        w = f"locations/{i}"; ref(w, "line", l.get("name"))
        bg = image(l["background"])
        layers = [l.get(k) for k in ("walkway", "objects_mask", "depth")] + [b["mask"] for b in l.get("walkbehinds", [])]
        for rel in [l["background"], l.get("music")] + layers: ref(w, "file", rel)
        for rel in filter(None, layers):
            if bg and image(rel) and image(rel).size != bg.size: E(w, f"{rel} is {image(rel).size}, background is {bg.size}")
        walk = image(l["walkway"]).convert("L") if l.get("walkway") and image(l["walkway"]) else None
        for m, (x, y) in l.get("marks", {}).items():
            if walk and not (0 <= x < walk.width and 0 <= y < walk.height and walk.getpixel((int(x), int(y))) > 127):
                E(w, f"mark '{m}' is off the walkway")
        mask = image(l["objects_mask"]).convert("RGB") if l.get("objects_mask") and image(l["objects_mask"]) else None
        colours = {"#%02x%02x%02x" % c for _, c in mask.getcolors(1 << 24)} if mask else set()
        hot = l.get("hotspots", {})
        for o, h in hot.items():
            ref(w, "object", o); ref(w, "mark", h["mark"], i)
            # the engine finds hotspots by their colour under the mouse, so an unpainted one can't be clicked
            if h["color"].lower() not in colours: E(w, f"colour {h['color']} for '{o}' isn't painted in the object mask, so it can't be clicked")
            if [x["color"].lower() for x in hot.values()].count(h["color"].lower()) > 1: E(w, f"colour {h['color']} is used twice")
        for e, x in l.get("exits", {}).items():
            ref(w, "edge", e); used["edge"].add(e)
            edge = next((d for d in P["map"].get("edges", []) if d["id"] == e), None)
            if edge and edge["from"] != i: E(w, f"exit '{e}' leaves from {edge['from']}, not {i}")
            if x["hotspot"] not in hot: E(w, f"exit '{e}' uses '{x['hotspot']}', which isn't a hotspot here")
            elif x.get("state") and x["state"] not in P["objects"].get(x["hotspot"], {}).get("states", {}):
                E(w, f"exit '{e}' is gated on unknown state '{x['state']}'")
        for n in l.get("npcs", []): ref(w, "character", n["character"]); ref(w, "mark", n["mark"], i)
        rules(w, l, i, ["enter"])

    # -- objects and characters
    for i, o in P["objects"].items():
        w = f"objects/{i}"; ref(w, "line", o.get("name")); ref(w, "file", o.get("icon"))
        for sprite in o["states"].values(): ref(w, "file", sprite)
        if o["state"] not in o["states"]: E(w, f"start state '{o['state']}' isn't one of its states")
        for verb, line in o.get("fallback", {}).items(): ref(f"{w} fallback.{verb}", "line", line)
        rules(w, o, i, I["verb"])
    for i, c in P["characters"].items():
        w = f"characters/{i}"; ref(w, "line", c.get("name")); ref(w, "file", c.get("voice", {}).get("ref_wav"))
        for a in c["animations"].values(): ref(w, "file", a["sheet"])
        if "idle" not in c["animations"]: E(w, "needs an 'idle' animation")
        for verb, line in c.get("fallback", {}).items(): ref(f"{w} fallback.{verb}", "line", line)
        rules(w, c, i, I["verb"])

    # -- dialogue: lines exist and every goto lands on a node in that file's lookup
    for c, fs in P["dialogue"].items():
        ref(f"characters/{c}/dialogue", "character", c)
        for f, d in fs.items():
            w = f"characters/{c}/dialogue/{f}"
            if f != "global": ref(w, "location", f)
            start, nodes = spec.dialogue(P, c, f) if f != "global" else (d.get("start", ""), d.get("nodes", {}))
            if start not in nodes: E(w, f"start node '{start}' doesn't exist")
            for nid, n in d.get("nodes", {}).items():
                ref(f"{w} {nid}", "line", n.get("say"))
                for goto in [n.get("goto")] + [o.get("goto") for o in n.get("options", [])]:
                    if goto and goto not in nodes: E(f"{w} {nid}", f"goto '{goto}' doesn't exist here")
                for o in n.get("options", []): ref(f"{w} {nid}", "line", o["line"])

    # -- every condition and action, via the spec tables
    for where, loc, conds, acts in spec.blocks(P):
        for c in conds: logic(where, loc, c, spec.CONDS)
        for a in acts: logic(where, loc, a, spec.ACTS)
        for a in acts:
            if a["type"] == "go": used["edge"].add(a["edge"])

    # -- unused things (warnings)
    for f in sorted(set(I["flag"]) - used["flag"]): W("game.yaml", f"flag '{f}' is never used")
    for e in sorted(set(edge_ids) - used["edge"]): W("map.yaml", f"edge '{e}' has no hotspot or go action, so it can't be taken")
    for f in sorted(set(I["file"]) - used["file"]):
        if not f.startswith("voice/"): W(f, "asset isn't used")
    verbs = {f"verb_{v}" for v in I["verb"]}
    for s in sorted(set(I["line"]) - used["line"] - spec.RESERVED - verbs): W("strings", f"'{s}' isn't used")
    return out


# ======================= tier 2: puzzle solver =======================
# Abstract state: room, inventory, flags, object states, NPC rooms, the said-lines that some
# condition cares about, and the open conversation. Player position is ignored.

def ok(s, conds):
    for c in conds:
        t, want = c["type"], c.get("is", True)
        if t == "flag": v = s["flags"].get(c["flag"], False) == want
        elif t == "state": v = s["objs"].get(c["object"]) == c["is"]
        elif t == "has": v = (c["item"] in s["inv"]) == want
        elif t == "at": v = (s["chars"].get(c["character"]) == c["location"]) == want
        elif t == "said": v = (c["line"] in s["said"]) == want
        else: v = False
        if not v: return False
    return True


def run(P, s, acts, rel):
    """Apply actions in order. walk/wait/music/sound/char_state don't affect reachability."""
    for a in acts:
        t = a["type"]
        if t == "say" and a["line"] in rel: s["said"].add(a["line"])
        elif t == "set_flag": s["flags"][a["flag"]] = a.get("value", True)
        elif t == "set_state": s["objs"][a["object"]] = a["state"]
        elif t == "give" and a["item"] not in s["inv"]: s["inv"].append(a["item"])
        elif t == "remove": s["inv"] = [i for i in s["inv"] if i != a["item"]]
        elif t == "move": s["chars"][a["character"]] = a["location"]
        elif t == "go":
            e = next(e for e in P["map"]["edges"] if e["id"] == a["edge"])
            enter(P, s, e["to"], rel)
        elif t == "talk": talk(P, s, a["character"], a.get("node", ""), rel)


def enter(P, s, loc, rel):
    s["loc"] = loc
    for r in P["locations"][loc].get("rules", []):
        if r["verb"] == "enter" and ok(s, r.get("if", [])): return run(P, s, r["do"], rel)


def talk(P, s, char, node, rel):
    """Open a dialogue node: the NPC's line, its actions; options are offered once actions finish."""
    start, nodes = spec.dialogue(P, char, s["loc"])
    nid = node or start
    if nid not in nodes: return
    s["dialog"] = (char, nid)
    run(P, s, ([{"type": "say", "line": nodes[nid]["say"]}] if nodes[nid].get("say") else []) + nodes[nid].get("do", []), rel)


def options(P, s):
    char, nid = s["dialog"]
    node = spec.dialogue(P, char, s["loc"])[1].get(nid, {})
    return [o for o in node.get("options", []) if ok(s, o.get("if", [])) and not (o.get("once") and o["line"] in s["said"])]


def settle(P, s, rel):
    """With nothing left to run: a node without visible options follows its goto or ends the chat."""
    for _ in range(50):
        if not s["dialog"] or options(P, s): return
        char, nid = s["dialog"]; s["dialog"] = None
        goto = spec.dialogue(P, char, s["loc"])[1].get(nid, {}).get("goto")
        if goto: talk(P, s, char, goto, rel)


def interact(P, s, verb, target, item, rel):
    """Verb dispatch, same order as the engine: combination, target's rules, dialogue, fallbacks."""
    if item and item != target and verb == "use" and target in s["inv"]:
        for r in P["game"].get("combinations", []):
            if target in r["items"] and item in r["items"] and ok(s, r.get("if", [])): return run(P, s, r["do"], rel)
    sheet = lambda i: P["objects"].get(i) or P["characters"].get(i) or {}
    for r in sheet(target).get("rules", []):
        if r["verb"] == verb and r.get("with", "") == item and ok(s, r.get("if", [])): return run(P, s, r["do"], rel)
    if verb == "talk" and target in P["dialogue"]: return talk(P, s, target, "", rel)
    # a fallback line only gets said, but saying it still counts for "said" conditions
    fb = lambda i: sheet(i).get("fallback", {}).get(verb, "")
    line = (item and fb(item)) or fb(target) or P["game"].get("defaults", {}).get(verb, "")
    if line: run(P, s, [{"type": "say", "line": line}], rel)


def arrive(P, s, verb, target, rel):
    """The player reaches a hotspot: walking onto an exit takes it if the hotspot's state allows."""
    if verb == "walk":
        for e, x in P["locations"][s["loc"]].get("exits", {}).items():
            if x["hotspot"] == target and (not x.get("state") or s["objs"].get(target) == x["state"]):
                return run(P, s, [{"type": "go", "edge": e}], rel)
    interact(P, s, verb, target, "", rel)


def visible(P, s):
    """Targets in the current room: active hotspots and NPCs whose placement condition holds."""
    loc = P["locations"][s["loc"]]
    objs = [o for o in loc.get("hotspots", {}) if s["objs"].get(o) != "gone"]
    chars = []
    for c, l in s["chars"].items():
        n = next((n for n in loc.get("npcs", []) if n["character"] == c), None)
        if l == s["loc"] and (n is None or ok(s, n.get("if", []))): chars.append(c)
    return objs + chars


def clone(s): return {**s, "inv": list(s["inv"]), "flags": dict(s["flags"]), "objs": dict(s["objs"]), "chars": dict(s["chars"]), "said": set(s["said"])}


def freeze(s):
    return (s["loc"], tuple(sorted(s["inv"])), tuple(sorted(s["flags"].items())), tuple(sorted(s["objs"].items())),
            tuple(sorted(s["chars"].items())), tuple(sorted(s["said"])), s["dialog"])


def moves(P, s, rel):
    """Everything the player can do from s, as (label, next state)."""
    out = []

    def do(label, fn):
        n = clone(s); fn(n); settle(P, n, rel); out.append((label, n))

    if s["dialog"]:
        for o in options(P, s):
            def choose(n, o=o):
                char = n["dialog"][0]; n["dialog"] = None
                run(P, n, [{"type": "say", "line": o["line"]}], rel)
                if o.get("goto"): talk(P, n, char, o["goto"], rel)
            do(f"say '{o['line']}'", choose)
        return out
    verbs = P["game"].get("verbs", [])
    for t in visible(P, s):
        for v in verbs: do(f"{v} {t}", lambda n, v=v, t=t: arrive(P, n, v, t, rel))
        for it in s["inv"]: do(f"use {it} on {t}", lambda n, it=it, t=t: interact(P, n, "use", t, it, rel))
    for it in s["inv"]:
        # "use X (inventory)" is using X on its own: in the engine, pick "use", click X, click X again
        for v in verbs: do(f"{v} {it} (inventory)", lambda n, v=v, it=it: interact(P, n, v, it, "", rel))
        for other in s["inv"]:
            if other != it: do(f"use {it} on {other}", lambda n, it=it, o=other: interact(P, n, "use", o, it, rel))
    return out


def start(P):
    """The solver's start state, after the start room's enter rule, plus the said-lines it tracks:
    only those that some condition or "once" option reads are part of the state."""
    rel = {c["line"] for _, _, conds, _ in spec.blocks(P) for c in conds if c["type"] == "said"}
    rel |= {o["line"] for fs in P["dialogue"].values() for d in fs.values() for n in d.get("nodes", {}).values()
            for o in n.get("options", []) if o.get("once")}
    m = spec.new_state(P)
    s0 = {"loc": m["location"], "inv": list(m["inventory"]), "flags": dict(m["flags"]), "objs": dict(m["objects"]),
          "chars": {c: p.get("location", "") for c, p in m["characters"].items()}, "said": set(), "dialog": None}
    enter(P, s0, s0["loc"], rel); settle(P, s0, rel)
    return s0, rel


def solve(P, max_states=200_000):
    """Breadth-first search from the start state. Returns a report dict."""
    ending = P["game"].get("ending", "")
    s0, rel = start(P)
    states, parent, succ, seen = [s0], [None], [[]], {freeze(s0): 0}
    i = 0
    while i < len(states) and len(states) < max_states:
        if not states[i]["flags"].get(ending):                 # the game stops at the ending
            for label, n in moves(P, states[i], rel):
                k = freeze(n)
                if k not in seen: seen[k] = len(states); states.append(n); parent.append((i, label)); succ.append([])
                succ[i].append(seen[k])
        i += 1
    ends = [j for j, s in enumerate(states) if ending and s["flags"].get(ending)]
    capped = i < len(states)
    # Walk edges backwards from the endings: whatever can't reach one is a softlock. If the search
    # was capped, states it never expanded might still lead to an ending, so they count as good too;
    # that way a capped search only reports softlocks it's sure of.
    pred = [[] for _ in states]
    for a, bs in enumerate(succ):
        for b in bs: pred[b].append(a)
    seeds = ends + list(range(i, len(states)))
    good, stack = set(seeds), list(seeds)
    while stack:
        for a in pred[stack.pop()]:
            if a not in good: good.add(a); stack.append(a)
    stuck = [j for j in range(i) if j not in good] if ends or capped else []

    def path(j):
        steps = []
        while parent[j]: j, label = parent[j]; steps.append(label)
        return steps[::-1]

    items = set(spec.ids(P)["item"])
    return {"states": len(states), "capped": capped, "ending": bool(ends),
            "ending_path": path(ends[0]) if ends else [],
            "unreachable_locations": sorted(set(P["locations"]) - {s["loc"] for s in states}),
            "unobtainable_items": sorted(items - {it for s in states for it in s["inv"]}),
            "softlocks": len(stuck), "softlock_path": path(stuck[0]) if stuck else []}


def run_all(P, max_states=200_000):
    """Both tiers as one issue list (tier 2 only runs on a valid project), plus the solver report."""
    issues = check(P)
    if any(lvl == "error" for lvl, _, _ in issues): return issues, None
    r = solve(P, max_states)
    if P["game"].get("ending") and not r["ending"]:
        if r["capped"]: issues.append(("warning", "solver", "no route to the ending was found before the search stopped"))
        else: issues.append(("error", "solver", "the ending can never be reached"))
    # with a capped search these two lists only cover the part of the game that was explored
    maybe = " (in the part of the game the search explored)" if r["capped"] else ""
    for l in r["unreachable_locations"]: issues.append(("warning", "solver", f"location '{l}' can never be reached{maybe}"))
    for it in r["unobtainable_items"]: issues.append(("warning", "solver", f"item '{it}' can never be obtained{maybe}"))
    if r["softlocks"]: issues.append(("error", "solver", f"{r['softlocks']} softlocked states, e.g. after: " + " → ".join(r["softlock_path"])))
    if r["capped"]: issues.append(("warning", "solver", f"search stopped at {r['states']} states; results are partial"))
    return issues, r
