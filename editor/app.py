"""Point & Click Maker — the Streamlit editor.   Run:  streamlit run editor/app.py
Each page edits one part of the project dict (see spec.load). Edits save to disk automatically,
into the projects folder set on the Settings page (see config.py for where everything lives)."""
import copy, os, re
from pathlib import Path
import pandas as pd, streamlit as st
from PIL import Image, ImageDraw
from streamlit_image_coordinates import streamlit_image_coordinates
import ai, check, config, spec

HERE = Path(__file__).parent
PROJECTS = config.folder("projects")      # read each run, so a change on the Settings page takes effect at once
st.set_page_config("Point & Click Maker", layout="wide")
S = st.session_state
for k, v in {"ver": 0, "undo": [], "dirty": False, "pts": [], "click": None}.items(): S.setdefault(k, v)
Col = st.column_config
PALETTE = ["#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff", "#ff8000", "#8000ff", "#0080ff", "#80ff00"]
FACING = ["", "e", "se", "s", "sw", "w", "nw", "n", "ne", "left", "right"]
LAYER_FILE = {"background": "bg", "walkway": "walk", "objects_mask": "objects", "depth": "depth"}


# ---------------- widget plumbing ----------------
# Widget keys include S.ver, so bumping it (after undo, a patch, or adding/removing list items)
# throws away stale widget state and every picker re-reads the project.

def K(*parts): return ":".join(map(str, (S.ver, *parts)))
def changed(): S.dirty = True
def restructure(): S.ver += 1; S.dirty = True; st.rerun()
def reload(P): S.proj = P; S.ver += 1; S.dirty = True
# Undo: S.saved is the project as last written to disk. Every run that changes something pushes
# it onto S.undo before saving (see the bottom of this file), so each edit is one undo step.


def put(d, k, v, default=None):
    """Store a widget value if it changed. Empty values remove the (optional) key. A key the sheet
    leaves out stays out while its widget shows the `default`, so viewing a sheet never writes."""
    if k not in d and default is not None and v == default: return
    if v in ("", None, [], {}):
        if d.get(k) not in ("", None, [], {}): d.pop(k); changed()
    elif d.get(k) != v: d[k] = v; changed()


def table(rows, cols, key, /, **colcfg):
    """Editable table of dicts. Returns rows with empty cells dropped and numbers made plain.
    (Positional-only arguments, so column configs may be named rows/cols/key.)"""
    df = st.data_editor(pd.DataFrame(rows, columns=cols), num_rows="dynamic", hide_index=True, key=K(*key), column_config=colcfg)
    def plain(v):
        v = v.item() if hasattr(v, "item") else v
        return int(v) if isinstance(v, float) and v.is_integer() else v
    return [{k: plain(v) for k, v in r.items() if v == v and v not in ("", None)} for r in df.to_dict("records")]


# ---------------- sidebar: project ----------------

def create():
    """Make a project from the name typed: the name becomes the title, and the folder gets an id
    made from it ("My Game" -> my_game). Says why when it can't, and switches to the new project."""
    name = S.new_name.strip(); gid = spec.make_id(name); where = config.folder("projects")
    if not gid: S.create_msg = ("error", "Type a name for the game, using some letters or numbers."); return
    if (where / gid).exists(): S.create_msg = ("error", f"There's already a project called '{gid}'."); return
    spec.new_project(where / gid, gid, title=name)
    S.pick, S.new_name = gid, ""          # the box empties only on success, so a bad name can be fixed
    S.create_msg = ("success", f"Created '{name}' (in {where / gid}).")


def copy_example(name):
    gid = config.copy_example(name); S.pick = gid
    S.create_msg = ("success", f"Copied the {name} example to {config.folder('projects') / gid}. Edit it as you like.")


def set_folder(kind, default=False):
    """Use the folder typed on the Settings page for projects, exports or models (or go back to the
    default), creating it if need be."""
    path = None if default else Path(S[f"set:{kind}"].strip()).expanduser()
    try:
        if path is not None and not path.is_absolute(): raise ValueError("give a full path, like C:\\Games or ~/Games")
        if path is not None: path.mkdir(parents=True, exist_ok=True)
    except (OSError, ValueError) as e:
        S.settings_msg = ("error", f"Can't use that folder: {e}"); return
    config.update(**{kind: str(path) if path else None})
    S[f"set:{kind}"] = config.settings()[kind]
    if kind == "projects": S.pop("pick", None); S.loaded = None        # open a project from the new folder
    S.settings_msg = ("success", f"{kind.capitalize()} now go in {config.settings()[kind]}.")


def settings_page():
    st.header("Settings")
    if not config.projects():
        st.info(f"There are no games in your projects folder ({config.folder('projects')}) yet. Create one in the sidebar, "
                "copy an example below, or choose a different projects folder.")
    st.caption(f"Kept in `{config.settings_file()}`, so they stay the same wherever the editor is run from. Nothing is ever "
               "saved inside the editor's own folder.")
    s, d = config.settings(), config.defaults()
    st.subheader("Folders")
    what = {"projects": "Projects: where your games are kept", "exports": "Exports: where playable builds go",
            "models": "AI models: downloaded models for the AI assets page"}
    for kind in config.FOLDERS:
        S.setdefault(f"set:{kind}", s[kind])
        c = st.columns([6, 1, 1], vertical_alignment="bottom")
        c[0].text_input(what[kind], key=f"set:{kind}")
        c[1].button("Use this folder", key=f"use:{kind}", on_click=set_folder, args=(kind,), disabled=S[f"set:{kind}"].strip() == s[kind])
        c[2].button("Default", key=f"def:{kind}", on_click=set_folder, args=(kind, True), disabled=s[kind] == d[kind], help=d[kind])
    if os.environ.get("PNC_PROJECTS"): st.info(f"PNC_PROJECTS is set, so for now projects come from {os.environ['PNC_PROJECTS']}.")
    if S.get("settings_msg"):
        level, msg = S.pop("settings_msg"); (st.error if level == "error" else st.success)(msg)
    st.subheader("Examples")
    st.caption("Games that come with the editor. Copying one makes a new project of your own; the example itself never changes.")
    for name in config.examples():
        st.button(f"Copy the {name} example", key=f"example:{name}", on_click=copy_example, args=(name,))


PAGES = ["Game", "Map", "Locations", "Objects", "Characters", "Dialogue", "Strings", "Assets", "Assistant", "Check & export", "AI assets"]
with st.sidebar:
    st.title("Point & Click Maker")
    names = config.projects()
    pick_project = st.selectbox("Project", names, key="pick") if names else None
    with st.form("new_project", border=False):     # a form, so pressing Enter creates it too
        st.text_input("New game", key="new_name", placeholder="My Game")
        st.form_submit_button("Create project", on_click=create)
    if S.get("create_msg"):
        level, msg = S.pop("create_msg"); (st.error if level == "error" else st.success)(msg)
    if pick_project:
        st.caption(f"Every change saves as you make it, to `{PROJECTS / pick_project}`. There's no Save button; Undo steps back.")
    page = st.radio("Edit", PAGES + ["Settings"]) if pick_project else st.radio("Edit", ["Settings"], key="page_empty")
    if S.undo and pick_project and st.button("Undo last change", help="Undoes edits to the spec files; painted layers and uploads stay"):
        reload(S.undo.pop()); S.undoing = True; st.rerun()
if page == "Settings": settings_page(); st.stop()
if S.get("loaded") != pick_project:
    S.proj, S.loaded, S.undo = spec.load(PROJECTS / pick_project), pick_project, []; S.ver += 1
    S.saved = copy.deepcopy(S.proj)
P = S.proj; G = P["game"]; LANG = G.get("lang", "en"); ROOT = Path(P["root"]); I = spec.ids(P)
PNGS = [f for f in I["file"] if f.endswith(".png")]
AUDIO = [f for f in I["file"] if f.endswith((".wav", ".ogg", ".mp3", ".flac"))]


# ---------------- pickers bound to real IDs ----------------

def txt(lid): return P["strings"].get(LANG, {}).get(lid, "")
def label(i): return "—" if i == "" else (f"{spec.name(P, i)} ({i})" if spec.name(P, i) != i else i)


def pick(lbl, options, value, key, blank=True, help=None):
    opts = ([""] if blank else []) + list(options)
    if value not in opts: opts.append(value)
    return st.selectbox(lbl, opts, opts.index(value), key=key, format_func=label, help=help)


MARK_HELP = ("A mark is a named spot in a room, like by_the_door. Place marks on the Locations page by clicking the room's "
             "picture where a character's feet should go.")


def line(lbl, lid, key, hint="line"):
    """Edit player-facing text in place. Its string ID is made once from the text and then kept
    stable, so rewording a line keeps its translations, TTS file and 'already said' tracking."""
    tab = P["strings"].setdefault(LANG, {})
    t = st.text_input(lbl, tab.get(lid, ""), key=K("line", key), placeholder="(none)")
    if t and not lid:
        base = "str_" + ("_".join(re.findall(r"[a-z0-9]+", t.lower())[:4]) or hint)
        lid, n = base, 2
        while lid in tab: lid, n = f"{base}_{n}", n + 1
    if lid and t and tab.get(lid) != t: tab[lid] = t; changed()
    return lid if t else ""


def field(f, kind, v, key, it, loc):
    """One picker for a condition/action field, choosing from IDs of the right kind."""
    kind = kind.rstrip("?")
    if kind == "bool": return st.checkbox(f, True if v is None else v, key=key)
    if kind == "num": return st.number_input(f, value=float(v if v is not None else 1.0), step=0.5, key=key)
    if kind == "line": return line(f, v or "", key)
    if kind == "text": return st.text_input(f, v or "", key=key)
    if kind == "state": opts = list(P["objects"].get(it.get("object"), {}).get("states", {}))
    elif kind == "mark": opts = list(P["locations"].get(it.get("location") or loc, {}).get("marks", {})) or I["mark"]
    elif kind == "node": opts = sorted({n for d in P["dialogue"].get(it.get("character"), {}).values() for n in d.get("nodes", {})})
    else: opts = I[kind]
    return pick(f, opts, v or "", key)


def edit_list(items, table_, key, loc=None):
    """Conditions or actions as rows: a type picker, then one picker per field. Edits in place."""
    types, keep, removed = list(table_), [], False
    for i, it in enumerate(items):
        cols = st.columns([2, 3, 3, 3, 1])
        t = cols[0].selectbox("type", types, types.index(it["type"]) if it["type"] in types else 0, key=K(key, i, "type"))
        new = {"type": t}
        for j, (f, kind) in enumerate(table_[t].items()):
            with cols[1 + j]: v = field(f, kind, it.get(f) if t == it["type"] else None, K(key, i, t, f), {**it, **new}, loc)
            if v not in ("", None) and not (kind == "bool" and v is True): new[f] = v      # true is the default
        if cols[4].button("✕", key=K(key, i, "x")): removed = True
        else: keep.append(new)
    if keep != items: items[:] = keep; changed()
    if removed: restructure()
    if st.button("Add condition" if table_ is spec.CONDS else "Add action", key=K(key, "add")):
        items.append({"type": types[0]}); restructure()


# ---------------- rule cards ----------------

VERBS = {"walk": "walks to", "look": "looks at", "talk": "talks to", "use": "uses", "take": "picks up", "enter": "enters"}
COND_TXT = {"flag": ("{flag} is set", "{flag} is not set"), "state": ("{object} is {is}", "{object} is {is}"),
            "has": ("the player has {item}", "the player doesn't have {item}"),
            "at": ("{character} is in {location}", "{character} isn't in {location}"),
            "said": ("{line} has been said", "{line} hasn't been said")}
ACT_TXT = {"say": "{who} says {line}", "set_flag": "set {flag}", "set_state": "make {object} {state}", "give": "give {item}",
           "remove": "take away {item}", "move": "move {character} to {location}", "char_state": "set {character} to {state}",
           "go": "exit via {edge}", "walk": "walk to {mark}", "talk": "talk to {character}", "play_music": "play music {file}",
           "play_sound": "play {file}", "wait": "wait {seconds}s"}


def tok(kind, v):
    """A bold token for the prose form of a rule."""
    kind = kind.rstrip("?")
    if v in ("", None): return "**?**"
    if kind == "line": return f"“{txt(v)[:60] or v}”"
    return f"**{spec.name(P, v) if kind in ('object', 'item', 'character', 'location') else v}**"


def phrase(it, table_, tmpl):
    vals = {f: tok(k, it.get(f)) for f, k in table_.get(it["type"], {}).items()}
    if it["type"] == "say": vals["who"] = tok("character", it.get("who") or G["player"])
    if it["type"] == "set_flag" and it.get("value") is False: return f"clear {vals['flag']}"
    t = tmpl.get(it["type"], it["type"])
    return (t[0 if it.get("is", True) is not False else 1] if isinstance(t, tuple) else t).format(**vals)


def prose(r, owner, kind):
    """Canonical readable form: When the player uses Key on Door, if Door is locked → make Door open."""
    if kind == "combo": a, b = (r.get("items", []) + ["", ""])[:2]; head = f"When the player **uses** {tok('item', a)} **with** {tok('item', b)}"
    elif r.get("verb") == "enter": head = f"When the player **enters** {tok('location', owner)}"
    elif r.get("with"): head = f"When the player **uses** {tok('item', r['with'])} **on** {tok('object', owner)}"
    else: head = f"When the player **{VERBS.get(r.get('verb'), r.get('verb'))}** {tok('object', owner)}"
    conds = " and ".join(phrase(c, spec.CONDS, COND_TXT) for c in r.get("if", []))
    acts = ", ".join(phrase(a, spec.ACTS, ACT_TXT) for a in r.get("do", []))
    return f"{head}{', if ' + conds if conds else ''} → {acts or 'nothing'}."


def rule_cards(d, owner, kind, verbs, loc=None, key="rules"):
    """Every rule as a prose card; Edit opens pickers for each token. First matching rule wins."""
    rules, order = d.get(key, []), None       # no empty list written for a sheet without rules
    for i, r in enumerate(rules):
        with st.container(border=True):
            st.markdown(prose(r, owner, kind))
            with st.expander("Edit"):
                c = st.columns(4)
                if kind == "combo":
                    two = (r.get("items", []) + ["", ""])[:2]
                    with c[0]: a = pick("Item", I["item"], two[0], K(owner, key, i, "a"))
                    with c[1]: b = pick("With item", I["item"], two[1], K(owner, key, i, "b"))
                    put(r, "items", [a, b])
                else:
                    with c[0]: put(r, "verb", pick("Verb", verbs, r.get("verb", ""), K(owner, key, i, "verb"), blank=False))
                    if kind != "location":
                        with c[1]: put(r, "with", pick("Used with item", I["item"], r.get("with", ""), K(owner, key, i, "with")))
                if i and c[2].button("Move up", key=K(owner, key, i, "up")): order = (i, i - 1)
                if c[3].button("Delete rule", key=K(owner, key, i, "del")): order = (i, None)
                st.caption("If all of these hold")
                edit_list(r.setdefault("if", []), spec.CONDS, K(owner, key, i, "if"), loc)
                st.caption("Do these, in order")
                edit_list(r.setdefault("do", []), spec.ACTS, K(owner, key, i, "do"), loc)
                if not r["if"]: r.pop("if")
    if order:
        i, j = order
        if j is None: rules.pop(i)
        else: rules[i], rules[j] = rules[j], rules[i]
        restructure()
    if st.button("Add rule", key=K(owner, key, "add")):
        d.setdefault(key, []).append({"items": ["", ""], "do": []} if kind == "combo" else {"verb": verbs[0], "do": []}); restructure()


def fallbacks(d, owner):
    """Lines for a verb when no rule matched (an item's 'use' line covers using it on anything)."""
    cols, new = st.columns(len(I["verb"]) or 1), {}
    for c, v in zip(cols, I["verb"]):
        with c: new[v] = line(v, d.get("fallback", {}).get(v, ""), f"{owner}:fb:{v}", f"{owner}_{v}")
    put(d, "fallback", {k: x for k, x in new.items() if x})


def add_entity(kind):
    """Add a location, object or character from the name typed. The name becomes its display name
    and its id is made from it ("Living Room" -> living_room). Says why when it can't."""
    name = S.get(f"new:{kind}", "").strip(); i = spec.make_id(name)
    if not i: S[f"msg:{kind}"] = "Type a name, using some letters or numbers."; return
    taken = next((k[:-1] for k in ("locations", "objects", "characters") if i in S.proj[k]), None)
    if taken: S[f"msg:{kind}"] = f"'{i}' is already a {taken}'s id (locations, objects and characters share ids)."; return
    sheet = spec.blank(kind, i)
    tab = S.proj["strings"].setdefault(S.proj["game"].get("lang", "en"), {})
    sid, n = f"str_{i}", 2
    while sid in tab: sid, n = f"str_{i}_{n}", n + 1
    tab[sid], sheet["name"] = name, sid
    S.proj[kind][i] = sheet; S[f"sel:{kind}"] = i; S[f"new:{kind}"] = ""; S.ver += 1; S.dirty = True


EXAMPLE = {"locations": "Living Room", "objects": "Brass Key", "characters": "Old Sailor"}


def entity(kind, title):
    """Pick an existing entity or add a new one (type a name, press Enter or Add). Returns the
    selected ID (or None)."""
    c = st.columns([3, 3, 1], vertical_alignment="bottom")
    cur = c[0].selectbox(title, list(P[kind]), key=f"sel:{kind}", format_func=label) if P[kind] else None
    with c[1].form(f"add:{kind}", border=False):
        f = st.columns([3, 1], vertical_alignment="bottom")
        f[0].text_input(f"New {title.lower()}", key=f"new:{kind}", placeholder=EXAMPLE[kind])
        f[1].form_submit_button("Add", on_click=add_entity, args=(kind,))
    if cur and c[2].button("Delete", key=K("del", kind)): spec.delete(P, kind, cur); S.pop(f"sel:{kind}", None); restructure()
    if S.get(f"msg:{kind}"): st.error(S.pop(f"msg:{kind}"))
    return cur


# ---------------- location layer editor ----------------

def composite(lid, l, bg, show):
    """The background with its objects (in their starting state) and, with marks shown, characters
    at their marks, all in the engine's draw order. Over that, the chosen layers, the marks, and
    the polygon being drawn. An object that's only here under some condition is drawn faded."""
    im = bg.copy()
    items = []          # (layer, bottom edge, listed order, image, top-left): sorted like the engine's scene()
    states = spec.new_state(P)["objects"]
    for k, (o, h) in enumerate(l.get("hotspots", {}).items()):
        sprite = P["objects"].get(o, {}).get("states", {}).get(states.get(o, ""), "")
        if states.get(o) == "gone" or not sprite or "pos" not in h or not (ROOT / sprite).is_file(): continue
        sp = Image.open(ROOT / sprite).convert("RGBA")
        if h.get("if"): sp.putalpha(sp.getchannel("A").point(lambda a: a // 2))
        x, y = (max(0, int(v)) for v in h["pos"])
        items.append(({"back": 0, "front": 2}.get(h.get("layer"), 1), y + sp.height, k, sp, (x, y)))
    if "marks" in show:
        depth = Image.open(ROOT / l["depth"]).convert("L") if l.get("depth") and (ROOT / l["depth"]).is_file() else None
        s0, s1 = l.get("scale", [1, 1])
        who = [(n["character"], n["mark"]) for n in l.get("npcs", [])]
        who += [(G["player"], e["spawn"]) for e in P["map"].get("edges", []) if e["to"] == lid]
        if G.get("start", {}).get("location") == lid: who.append((G["player"], G["start"].get("mark")))
        for char, mark in who:
            a = P["characters"].get(char, {}).get("animations", {}).get("idle")
            if mark not in l.get("marks", {}) or not a or not (ROOT / a["sheet"]).is_file(): continue
            x, y = l["marks"][mark]
            sh = Image.open(ROOT / a["sheet"]).convert("RGBA")
            fw, fh = sh.width // a.get("cols", 1), sh.height // a.get("rows", 1)
            row = 2 if a.get("rows", 1) == 8 else 0                                 # facing the viewer
            sc = s0 + (s1 - s0) * (depth.getpixel((min(int(x), bg.width - 1), min(int(y), bg.height - 1))) / 255 if depth else 1)
            f = sh.crop((0, row * fh, fw, (row + 1) * fh)).resize((max(1, int(fw * sc)), max(1, int(fh * sc))))
            items.append((1, y, len(l.get("hotspots", {})), f, (max(0, int(x - f.width / 2)), max(0, int(y - f.height)))))
    for _, _, _, sp, xy in sorted(items, key=lambda t: (t[0], t[1] if t[0] == 1 else 0, t[2])):
        im.alpha_composite(sp, xy)

    def over(rel, how, alpha, tint=(0, 255, 120)):
        if not rel or not (ROOT / rel).is_file(): return
        m = Image.open(ROOT / rel).convert("RGB").resize(bg.size)
        a = m.convert("L").point(lambda v: int(alpha * 255) if v or how == "image" else 0)
        layer = (Image.new("RGB", bg.size, tint) if how == "tint" else m).convert("RGBA"); layer.putalpha(a)
        im.alpha_composite(layer)

    over(l.get("walkway"), "tint", .45 if "walkway" in show else .15)     # always faintly visible
    if "objects" in show: over(l.get("objects_mask"), "colour", .55)
    if "depth" in show: over(l.get("depth"), "image", .6)
    if "walk-behinds" in show:
        for b in l.get("walkbehinds", []): over(b["mask"], "tint", .45, (255, 0, 255))
    d = ImageDraw.Draw(im)
    if "marks" in show:
        for m, (x, y) in l.get("marks", {}).items():
            d.ellipse([x - 3, y - 3, x + 3, y + 3], fill="yellow", outline="black"); d.text((x + 5, y - 12), m, fill="white")
    if S.pts:
        d.line(S.pts + S.pts[:1] if len(S.pts) > 2 else S.pts, fill="cyan", width=2)
        for x, y in S.pts: d.rectangle([x - 2, y - 2, x + 2, y + 2], fill="cyan")
    if S.click:
        x, y = S.click; d.line([x - 8, y, x + 8, y], fill="red", width=2); d.line([x, y - 8, x, y + 8], fill="red", width=2)
    return im


def paint(rel, size, fill, poly, gradient=False):
    """Fill a polygon into a mask layer (created black if new). Depth gradients run black (far) at
    the polygon's top to white (near) at its bottom."""
    path = ROOT / rel; path.parent.mkdir(parents=True, exist_ok=True)
    m = Image.open(path).convert("RGB") if path.is_file() else Image.new("RGB", size, "black")
    shape = Image.new("L", size, 0); ImageDraw.Draw(shape).polygon(poly, fill=255)
    if gradient:
        y0, y1 = min(p[1] for p in poly), max(p[1] for p in poly)
        ramp = Image.new("RGB", size, "black"); ramp.paste(Image.linear_gradient("L").resize((size[0], max(1, y1 - y0))).convert("RGB"), (0, y0))
        m.paste(ramp, mask=shape)
    else: m.paste(fill, mask=shape)
    m.save(path)


def layer_editor(lid, l):
    """The picture (clicking it places a mark or a polygon point) with its controls beside it, so
    naming a mark or filling a polygon happens in view of the picture. Uploads sit in an expander."""
    if S.get("pts_loc") != lid: S.pts, S.click, S.pts_loc = [], None, lid
    has_bg = (ROOT / l.get("background", "")).is_file()
    with st.expander("Upload a background or layer image", expanded=not has_bg):
        up = st.file_uploader("Image", type=["png", "jpg"], key=K(lid, "up"))
        which = st.selectbox("It is the", list(LAYER_FILE), key=K(lid, "upwhich"))
        if up and st.button("Save image", key=K(lid, "upsave")):
            rel = f"locations/{lid}/{LAYER_FILE[which]}.png"
            (ROOT / rel).parent.mkdir(parents=True, exist_ok=True); Image.open(up).convert("RGB").save(ROOT / rel)
            put(l, which, rel); restructure()
    if not has_bg: st.info("Upload a background to start drawing layers."); return
    bg = Image.open(ROOT / l["background"]).convert("RGBA")
    pic, side = st.columns([3, 2])
    with side:
        show = st.multiselect("Show layers", ["walkway", "objects", "depth", "walk-behinds", "marks"], ["marks"], key=K(lid, "show"))
        mode = st.radio("Clicking the picture will", ["place a mark", "add a polygon point"], key=K(lid, "mode"), horizontal=True)
        if mode == "place a mark": mark_controls(lid, l)
        else: polygon_controls(lid, l, bg)
    with pic:      # stretched to the column; clicks are scaled back to the picture's own pixels
        click = streamlit_image_coordinates(composite(lid, l, bg, show), key=K(lid, "img"), width="stretch")
    if click and click != S.get("last_click"):
        S.last_click = click
        xy = (round(click["x"] * bg.width / click.get("width", bg.width)), round(click["y"] * bg.height / click.get("height", bg.height)))
        if mode == "place a mark": S.click = xy
        else: S.pts = S.pts + [xy]
        st.rerun()


LAYERS = {"back": "Back (behind characters)", "depth": "Depth (the default)", "front": "Front (in front of all)"}


def placed_objects(lid, l):
    """Each object in the room in draw order: its layer, moving it back or forward in the list, and
    the conditions under which it's here. The engine draws and clicks in exactly this order."""
    hot = l.get("hotspots", {})
    if not hot: return
    st.subheader("Objects in this room")
    st.caption("**Back** is behind characters (rugs, wall art). **Depth** sorts with characters by bottom edge. **Front** is "
               "in front of everything. Within a layer, lower in this list is in front. Clicks go to whatever is drawn in "
               "front. An object is only here (drawn and clickable) while its conditions hold.")
    ids, swap = list(hot), None
    for i, o in enumerate(ids):
        h = hot[o]
        c = st.columns([3, 3, 1, 1, 5], vertical_alignment="center")
        n = len(h.get("if", []))
        held = "its condition holds" if n == 1 else f"its {n} conditions hold"
        c[0].markdown(f"**{i + 1}.** {label(o)}" + (f"  \n*only here while {held}*" if n else ""))
        layer = c[1].selectbox(f"Layer for {o}", list(LAYERS), list(LAYERS).index(h.get("layer", "depth")), format_func=LAYERS.get,
                               key=K(lid, "layer", o), label_visibility="collapsed")
        put(h, "layer", "" if layer == "depth" else layer)          # depth is the default, so it isn't written
        if i and c[2].button("▲", key=K(lid, "back", o), help="Earlier in the list: drawn further back"): swap = (i, i - 1)
        if i < len(ids) - 1 and c[3].button("▼", key=K(lid, "fwd", o), help="Later in the list: drawn further forward"): swap = (i, i + 1)
        with c[4].expander("Only here if…"):
            edit_list(h.setdefault("if", []), spec.CONDS, K(lid, "hotif", o), lid)
        if not h["if"]: h.pop("if")
    if swap:
        i, j = swap; ids[i], ids[j] = ids[j], ids[i]
        l["hotspots"] = {o: hot[o] for o in ids}; restructure()


def mark_controls(lid, l):
    if not S.click: st.caption("Click the picture where a character's feet should go."); return
    nm = st.text_input(f"Name the mark at {S.click}", key=K(lid, "mname"), help="Marks are anchored at the feet.")
    mid = spec.make_id(nm or "")
    if nm and not mid: st.caption("Use some letters or numbers.")
    elif mid in l.get("marks", {}): st.caption(f"Adding moves the existing mark '{mid}' here.")
    elif mid and mid != nm: st.caption(f"Saved as '{mid}'.")
    if st.button("Add mark", key=K(lid, "addmark"), disabled=not mid):
        l.setdefault("marks", {})[mid] = list(S.click); S.click = None; restructure()


def polygon_controls(lid, l, bg):
    st.caption(f"{len(S.pts)} point(s) so far. Click the picture to add more; three make a polygon.")
    wb = l.get("walkbehinds", [])
    targets = ["walkway", "depth gradient"] + [f"object {o}" for o in l.get("hotspots", {})] + [f"walk-behind {i}" for i in range(len(wb))] + ["new walk-behind"]
    tgt = st.selectbox("Fill the polygon into", targets, key=K(lid, "tgt"))
    erase = st.checkbox("Erase", key=K(lid, "erase"))
    c = st.columns(2)
    if c[1].button("Clear points", key=K(lid, "clear")): S.pts = []; st.rerun()
    if c[0].button("Fill", key=K(lid, "fill"), disabled=len(S.pts) < 3, type="primary"):
        if tgt == "walkway": rel, fill = l.get("walkway") or f"locations/{lid}/walk.png", (255, 255, 255); put(l, "walkway", rel)
        elif tgt == "depth gradient": rel, fill = l.get("depth") or f"locations/{lid}/depth.png", None; put(l, "depth", rel)
        elif tgt.startswith("object"):
            rel = l.get("objects_mask") or f"locations/{lid}/objects.png"; put(l, "objects_mask", rel)
            fill = tuple(int(l["hotspots"][tgt[7:]]["color"][k:k + 2], 16) for k in (1, 3, 5))
        elif tgt == "new walk-behind":
            rel, fill = f"locations/{lid}/behind_{len(wb)}.png", (255, 255, 255)
            l["walkbehinds"] = wb + [{"mask": rel, "baseline": max(p[1] for p in S.pts)}]
        else: rel, fill = wb[int(tgt.split()[-1])]["mask"], (255, 255, 255)
        paint(rel, bg.size, (0, 0, 0) if erase else fill, S.pts, gradient=(tgt == "depth gradient" and not erase))
        S.pts = []; restructure()


# =============================== pages ===============================

if page == "Game":
    st.header("Game")
    if not P["locations"]:
        st.info("This game has no locations yet. Start on the **Locations** page: add one and upload its background. "
                "Then come back here and pick it as the start location.")
    c = st.columns(4)
    with c[0]: put(G, "title", line("Title", G.get("title", ""), "title", "title"))
    put(G, "lang", c[1].text_input("Main language", LANG, key=K("lang")), default="en")
    with c[2]: put(G, "player", pick("Player character", I["character"], G.get("player", ""), K("player")))
    with c[3]: put(G, "ending", pick("Ending flag", I["flag"], G.get("ending", ""), K("ending"),
                                     help="The game ends when this flag becomes true. Add it under Flags below first."))
    w, h = G.get("resolution", [640, 360])
    c = st.columns([1, 1, 2])         # the verbs get room to show every verb
    put(G, "resolution", [c[0].number_input("Width", value=int(w), key=K("w")), c[1].number_input("Height", value=int(h), key=K("h"))],
        default=[640, 360])
    common = ["walk", "look", "talk", "use", "take", "open", "close", "push", "pull", "give"]
    put(G, "verbs", c[2].multiselect("Verbs", sorted(set(common) | set(G.get("verbs", []))), G.get("verbs", []), key=K("verbs")))
    st.subheader("Interface")
    st.caption("How the player gives commands. The style decides which other settings apply.")
    ui = spec.interface(G)
    STYLES = {"bar": "Verb bar under the scene", "ring": "Verb ring: hold on something; right-click for the inventory"}
    LABELS = {"words": "Words", "icons": "Icons", "captioned": "Icons with captions"}
    SPEECH = {"top": "At the top of the screen", "over_speaker": "Over the speaker"}
    c = st.columns(3)
    style = c[0].radio("Style", list(STYLES), list(STYLES).index(ui["style"]), format_func=STYLES.get, key=K("ui_style"))
    labels = (c[1].radio("Verbs shown as", list(LABELS), list(LABELS).index(ui.get("labels", "words")), format_func=LABELS.get,
                         key=K("ui_labels")) if style == "bar" else "words")
    speech = c[2 if style == "bar" else 1].radio("Spoken lines appear", list(SPEECH), list(SPEECH).index(ui["speech"]),
                                                 format_func=SPEECH.get, key=K("ui_speech"))     # no gap where labels would be
    icons = dict(ui.get("icons", {}))         # kept whatever the style, so switching back finds them again
    if style == "ring" or labels != "words":
        shown = [v for v in G.get("verbs", []) if not (style == "ring" and v == "walk")]
        st.caption("Verb icons are drawn at 24×24 game pixels; 48×48 images stay sharp in the default window. A verb without "
                   "an icon shows its name." + (" The ring leaves out walk: walking is a plain click." if style == "ring" else ""))
        for col, v in zip(st.columns(len(shown) or 1), shown):
            with col:
                icons[v] = pick(v, PNGS, icons.get(v, ""), K("ui_icon", v))
                if icons[v] and (ROOT / icons[v]).is_file(): st.image(str(ROOT / icons[v]), width=32)
    new = spec.interface({"verbs": G.get("verbs", []), "interface": {"style": style, "labels": labels, "icons": icons, "speech": speech}})
    if new != ui: G["interface"] = new; changed()       # only settings the chosen style uses are written
    st.subheader("Flags: what the game remembers")
    st.caption("A flag is a yes/no fact the game remembers, like *guard_left* or *door_unlocked*. Rules set flags "
               "(**set flag**) and check them (**if guard_left is set**), which is how one puzzle unlocks the next. "
               "List each flag here with how it starts (ticked means yes); then the Check can catch a misspelt flag in a rule.")
    rows = [{"flag": f, "default": v} for f, v in G.get("flags", {}).items()]
    put(G, "flags", {r["flag"]: bool(r.get("default", False)) for r in table(rows, ["flag", "default"], ("flags",),
                     flag=Col.TextColumn(help="A short name: lowercase, with underscores"),
                     default=Col.CheckboxColumn("starts true?")) if r.get("flag")})
    st.subheader("Where the game starts")
    st.caption("The room the player starts in, the spot they stand on, what they're carrying, and where everyone else is.")
    s = G.setdefault("start", {})
    c = st.columns(3)
    with c[0]: put(s, "location", pick("Starting location", I["location"], s.get("location", ""), K("sloc")))
    start_marks = list(P["locations"].get(s.get("location"), {}).get("marks", {}))
    with c[1]:
        put(s, "mark", pick("Starting spot (a mark)", start_marks, s.get("mark", ""), K("smark"), help=MARK_HELP))
        if s.get("location") and not start_marks: st.caption("This location has no marks yet: add one on the Locations page.")
    put(s, "inventory", c[2].multiselect("Starting inventory", I["item"], [i for i in s.get("inventory", []) if i in I["item"]],
                                         key=K("sinv"), format_func=label, help="Only objects with an inventory icon can be carried."))
    st.caption("Other characters, and where each one starts (a room and a mark in it):")
    rows = [{"character": k, **v} for k, v in s.get("characters", {}).items()]
    new = table(rows, ["character", "location", "mark", "state"], ("schars",), character=Col.SelectboxColumn(options=I["character"]),
                location=Col.SelectboxColumn(options=I["location"]), mark=Col.SelectboxColumn(options=I["mark"]))
    put(s, "characters", {r.pop("character"): r for r in new if r.get("character")})
    st.subheader("Default responses")
    st.caption("What the player says when a verb is used on something with no rule or line of its own for it.")
    new = {}
    for col, v in zip(st.columns(len(I["verb"]) or 1), I["verb"]):
        with col: new[v] = line(v, G.get("defaults", {}).get(v, ""), f"def:{v}", f"default_{v}")
    put(G, "defaults", {k: x for k, x in new.items() if x})
    st.subheader("Item-on-item combinations")
    rule_cards(G, "game", "combo", [], key="combinations")

elif page == "Map":
    st.header("Map")
    st.caption("Every way between locations: where it goes from and to, and **spawn**, the mark the player arrives on. "
               "Each location's page then says which of its objects (a door, say) leads this way.")
    new = table(P["map"].get("edges", []), ["id", "from", "to", "spawn"], ("edges",), spawn=Col.SelectboxColumn(options=I["mark"]),
                **{"from": Col.SelectboxColumn(options=I["location"]), "to": Col.SelectboxColumn(options=I["location"])})
    put(P["map"], "edges", [r for r in new if r.get("id")])
    hot = {e: f"{l}: {x['hotspot']}" for l, d in P["locations"].items() for e, x in d.get("exits", {}).items()}
    dot = [f'"{l}" [label="{spec.name(P, l)}"];' for l in P["locations"]]
    dot += [f'"{e["from"]}" -> "{e["to"]}" [label="{hot.get(e["id"], "(no hotspot)")}"];' for e in P["map"].get("edges", [])]
    st.graphviz_chart("digraph { node [shape=box]; " + " ".join(dot) + "}")

elif page == "Locations":
    st.header("Locations")
    lid = entity("locations", "Location")
    if lid:
        l = P["locations"][lid]
        c = st.columns(4)
        with c[0]: put(l, "name", line("Name", l.get("name", ""), f"{lid}:name", lid))
        with c[1]: put(l, "music", pick("Default music", AUDIO, l.get("music", ""), K(lid, "music")))
        s0, s1 = l.get("scale", [1, 1])
        put(l, "scale", [c[2].number_input("Scale far (depth black)", value=float(s0), step=0.05, key=K(lid, "s0")),
                         c[3].number_input("Scale near (depth white)", value=float(s1), step=0.05, key=K(lid, "s1"))], default=[1, 1])
        layer_editor(lid, l)
        marks = list(l.get("marks", {}))
        a, b = st.columns(2)
        with a:
            st.subheader("Marks")
            st.caption("Named spots in this room, like *by_the_door*. Click the picture to place one, at a character's feet. "
                       "Characters start on marks and walk to them, each object names the mark the player walks to before using "
                       "it, and each exit names the mark the player arrives on.")
            rows = [{"mark": m, "x": xy[0], "y": xy[1]} for m, xy in l.get("marks", {}).items()]
            put(l, "marks", {r["mark"]: [r.get("x", 0), r.get("y", 0)] for r in table(rows, ["mark", "x", "y"], (lid, "marks")) if r.get("mark")})
            st.subheader("Walk-behinds")
            st.caption("Background areas drawn over characters whose feet are above the baseline.")
            put(l, "walkbehinds", [{"mask": r["mask"], "baseline": r.get("baseline", 0)} for r in
                                   table(l.get("walkbehinds", []), ["mask", "baseline"], (lid, "wb"), mask=Col.SelectboxColumn(options=PNGS)) if r.get("mask")])
        with b:
            st.subheader("Hotspots")
            st.caption("Each object's colour in the object mask (its click area if it has no sprite, extra area if it has), "
                       "the mark the player walks to, and where its sprite's top-left goes.")
            old_hot = l.get("hotspots", {})
            rows = [{"object": o, "color": h["color"], "mark": h["mark"], "x": h.get("pos", [None, None])[0], "y": h.get("pos", [None, None])[1]}
                    for o, h in old_hot.items()]
            new, used = {}, [h["color"] for h in old_hot.values()]
            for r in table(rows, ["object", "color", "mark", "x", "y"], (lid, "hot"), object=Col.SelectboxColumn(options=I["object"]),
                           mark=Col.SelectboxColumn(options=marks)):
                if not r.get("object"): continue
                colour = r.get("color") or next((p for p in PALETTE if p not in used), "#808080"); used.append(colour)   # auto-pick a free colour
                kept = {k: v for k, v in old_hot.get(r["object"], {}).items() if k in ("layer", "if")}   # set below, not in the table
                new[r["object"]] = {"color": colour, "mark": r.get("mark", ""), **({"pos": [r["x"], r["y"]]} if "x" in r and "y" in r else {}), **kept}
            put(l, "hotspots", new)
            st.subheader("Exits")
            st.caption("Walking onto the hotspot takes the map edge, if the hotspot's object is in the given state.")
            rows = [{"edge": e, **x} for e, x in l.get("exits", {}).items()]
            put(l, "exits", {r.pop("edge"): r for r in table(rows, ["edge", "hotspot", "state"], (lid, "exits"),
                             edge=Col.SelectboxColumn(options=[e["id"] for e in P["map"].get("edges", []) if e["from"] == lid]),
                             hotspot=Col.SelectboxColumn(options=list(l.get("hotspots", {})))) if r.get("edge")})
        placed_objects(lid, l)
        st.subheader("Characters placed here")
        old = l.get("npcs", [])
        new = table([{k: n.get(k) for k in ("character", "mark", "facing", "state")} for n in old], ["character", "mark", "facing", "state"], (lid, "npcs"),
                    character=Col.SelectboxColumn(options=I["character"]), mark=Col.SelectboxColumn(options=marks), facing=Col.SelectboxColumn(options=FACING))
        # Placement conditions aren't table columns, so carry them over by character (the engine uses a
        # character's first placement here); matching by row would shift them when a row is deleted.
        conds = {}
        for n in old:
            if n.get("if"): conds.setdefault(n["character"], n["if"])
        new = [{**r, **({"if": conds[r["character"]]} if r["character"] in conds else {})} for r in new if r.get("character")]
        put(l, "npcs", new)
        for i, n in enumerate(l.get("npcs", [])):
            with st.expander(f"Only here if… ({spec.name(P, n['character'])})"): edit_list(n.setdefault("if", []), spec.CONDS, K(lid, "npc", i), lid)
            if not n["if"]: n.pop("if")
        st.subheader("Rules")
        rule_cards(l, lid, "location", ["enter"], loc=lid)

elif page == "Objects":
    st.header("Objects")
    oid = entity("objects", "Object")
    if oid:
        o = P["objects"][oid]
        c = st.columns(3)
        with c[0]: put(o, "name", line("Name", o.get("name", ""), f"{oid}:name", oid))
        with c[1]: put(o, "icon", pick("Inventory icon (makes it a carryable item)", PNGS, o.get("icon", ""), K(oid, "icon")))
        st.caption("Visual states: rules flip the state and the matching sprite follows. A state called 'gone' hides the object.")
        rows = [{"state": k, "sprite": v} for k, v in o["states"].items()]
        put(o, "states", {r["state"]: r.get("sprite", "") for r in table(rows, ["state", "sprite"], (oid, "states"),
                          sprite=Col.SelectboxColumn(options=[""] + PNGS)) if r.get("state")} or {"default": ""})
        with c[2]: put(o, "state", pick("Starting state", list(o["states"]), o["state"], K(oid, "state"), blank=False))
        st.subheader("Rules"); rule_cards(o, oid, "object", I["verb"])
        st.subheader("Fallback lines"); fallbacks(o, oid)

elif page == "Characters":
    st.header("Characters")
    cid = entity("characters", "Character")
    if cid:
        ch = P["characters"][cid]
        c = st.columns(4)
        with c[0]: put(ch, "name", line("Name", ch.get("name", ""), f"{cid}:name", cid))
        put(ch, "speed", c[1].number_input("Walk speed (px/s)", value=float(ch.get("speed", 120)), key=K(cid, "speed")), default=120)
        put(ch, "text_color", c[2].color_picker("Speech colour", ch.get("text_color", "#ffffff"), key=K(cid, "col")), default="#ffffff")
        voice = dict(ch.get("voice", {}))
        voice["model"] = c[3].text_input("Voice model", voice.get("model", ""), key=K(cid, "voice"))
        st.subheader("Animations")
        st.caption("Sheets are grids: columns are frames, rows are 8 facing directions (e, se, s, sw, w, nw, n, ne) or 1 row (mirrored for west).")
        rows = [{"name": k, **v} for k, v in ch.get("animations", {}).items()]
        put(ch, "animations", {r.pop("name"): r for r in table(rows, ["name", "sheet", "cols", "rows", "fps"], (cid, "anims"),
                               sheet=Col.SelectboxColumn(options=PNGS), rows=Col.SelectboxColumn(options=[1, 8])) if r.get("name") and r.get("sheet")})
        prev = [ROOT / a["sheet"] for a in ch.get("animations", {}).values() if (ROOT / a["sheet"]).is_file()]
        if prev: st.image([str(p) for p in prev], caption=[p.name for p in prev])
        wav = st.file_uploader("Reference voice WAV for cloning", type=["wav"], key=K(cid, "wav"))
        if wav and st.button("Save reference voice"):
            rel = f"characters/{cid}/voice.wav"; (ROOT / rel).parent.mkdir(parents=True, exist_ok=True); (ROOT / rel).write_bytes(wav.getvalue())
            voice["ref_wav"] = rel
        put(ch, "voice", {k: v for k, v in voice.items() if v})
        st.subheader("Rules"); rule_cards(ch, cid, "character", I["verb"])
        st.subheader("Fallback lines"); fallbacks(ch, cid)

elif page == "Dialogue":
    st.header("Dialogue")
    cid = st.selectbox("Character", I["character"], key="dlg_char", format_func=label) if I["character"] else None
    files = P["dialogue"].get(cid, {})
    f = st.radio("File (a location's lines come first; global is the fallback)", ["global", *I["location"]], horizontal=True,
                 format_func=lambda x: x + (" ✓" if x in files else ""), key="dlg_file")
    if cid and f not in files and st.button(f"Create {f}.yaml for {spec.name(P, cid)}"):
        P["dialogue"].setdefault(cid, {})[f] = {"start": "start", "nodes": {"start": {}}}; restructure()
    if cid and f in files:
        d = files[f]; nodes = d.setdefault("nodes", {})
        merged = spec.dialogue(P, cid, f)[1] if f != "global" else nodes
        put(d, "start", pick("Start node", list(merged), d.get("start", ""), K(cid, f, "start")))
        esc = lambda s: s.replace('"', "'")
        dot = [f'"{n}" [label="{n}\\n{esc(txt(x.get("say", ""))[:40])}"];' for n, x in nodes.items()]
        for n, x in nodes.items():
            if x.get("goto"): dot.append(f'"{n}" -> "{x["goto"]}" [style=dashed];')
            dot += [f'"{n}" -> "{o["goto"]}" [label="{esc(txt(o["line"])[:30])}"];' for o in x.get("options", []) if o.get("goto")]
        st.graphviz_chart("digraph { rankdir=LR; node [shape=box, fontsize=10]; edge [fontsize=9]; " + " ".join(dot) + "}")
        drop = None
        for nid, n in nodes.items():
            with st.expander(f"{nid}: {txt(n.get('say', ''))[:60]}"):
                c = st.columns([4, 2, 1])
                with c[0]: put(n, "say", line(f"{spec.name(P, cid)} says", n.get("say", ""), f"{cid}:{f}:{nid}:say", f"{cid}_{nid}"))
                with c[1]: put(n, "goto", pick("Then go to (when no options)", list(merged), n.get("goto", ""), K(cid, f, nid, "goto")))
                if c[2].button("Delete node", key=K(cid, f, nid, "del")): drop = nid
                st.caption("Actions when this node plays")
                edit_list(n.setdefault("do", []), spec.ACTS, K(cid, f, nid, "do"), None if f == "global" else f)
                st.caption("Player options")
                opts, cut = n.setdefault("options", []), None
                for j, o in enumerate(opts):
                    c = st.columns([4, 2, 1, 1])
                    with c[0]: put(o, "line", line(f"Option {j + 1}", o.get("line", ""), f"{cid}:{f}:{nid}:o{j}", f"opt_{nid}"))
                    with c[1]: put(o, "goto", pick("Goes to", list(merged), o.get("goto", ""), K(cid, f, nid, j, "goto")))
                    put(o, "once", c[2].checkbox("Once", o.get("once", False), key=K(cid, f, nid, j, "once")) or None)
                    if c[3].button("✕", key=K(cid, f, nid, j, "x")): cut = j
                    st.caption("Shown only if…"); edit_list(o.setdefault("if", []), spec.CONDS, K(cid, f, nid, j, "if"))
                    if not o["if"]: o.pop("if")
                if cut is not None: opts.pop(cut); restructure()
                if st.button("Add option", key=K(cid, f, nid, "addopt")): opts.append({"line": ""}); restructure()
                for k in ("do", "options"):
                    if not n[k]: n.pop(k)
        if drop: nodes.pop(drop); restructure()
        c = st.columns([3, 1], vertical_alignment="bottom")
        nn = c[0].text_input("New node", key=K(cid, f, "newnode"), placeholder="Ask about the cat")
        nid = spec.make_id(nn or "")
        if nn and not nid: st.caption("Use some letters or numbers.")
        elif nid in nodes: st.caption(f"There's already a node '{nid}'.")
        elif nid and nid != nn: st.caption(f"Saved as '{nid}'.")
        if c[1].button("Add node", disabled=not nid or nid in nodes): nodes[nid] = {}; restructure()

elif page == "Strings":
    st.header("Strings")
    langs = list(P["strings"]) or [LANG]
    lang = st.radio("Language", langs, horizontal=True, key="lang_pick")
    rows = [{"id": k, "text": v} for k, v in P["strings"].get(lang, {}).items()]
    put(P["strings"], lang, {r["id"]: r.get("text", "") for r in table(rows, ["id", "text"], ("strings", lang)) if r.get("id")})
    missing = [k for k in P["strings"].get(LANG, {}) if not P["strings"].get(lang, {}).get(k)]
    if missing: st.warning(f"{len(missing)} lines have no {lang} text yet.")
    c = st.columns([2, 1])
    new = c[0].text_input("Add a language (e.g. fr)", key="newlang")
    if c[1].button("Add language", disabled=not new or new in P["strings"]):
        P["strings"][new] = {k: "" for k in P["strings"].get(LANG, {})}; restructure()

elif page == "Assets":
    st.header("Assets")
    folder = st.text_input("Upload into folder", "assets")
    ups = st.file_uploader("Files", accept_multiple_files=True)
    if ups and st.button("Save uploads", type="primary"):
        for u in ups: (ROOT / folder).mkdir(parents=True, exist_ok=True); (ROOT / folder / u.name).write_bytes(u.getvalue())
        st.rerun()
    for f in I["file"]:
        with st.expander(f):
            if f.endswith(".png"): st.image(str(ROOT / f))
            elif f in AUDIO: st.audio(str(ROOT / f))

elif page == "Assistant":
    st.header("Assistant")
    st.caption("Describe what should happen. The assistant drafts rule changes, checks them, and shows them as cards to accept.")
    cfg = ai.settings()
    intent = st.text_area("Intent", key="intent", placeholder="the key opens the door, but only after the guard leaves")

    def draft(answers):
        # anything a backend can throw (no backend set, network errors, a malformed reply) is shown, not a traceback
        try: S.draft = ai.author(P, intent, cfg, answers); S.answers = answers
        except Exception as e: st.error(f"{type(e).__name__}: {e}" if not isinstance(e, RuntimeError) else str(e))

    if st.button("Draft changes", type="primary", disabled=not intent): draft([])
    dr = S.get("draft")
    if dr and "ask" in dr:
        st.info(dr["ask"]["question"])
        for o in dr["ask"]["options"]:
            if st.button(o, key=f"ans:{o}"): draft(S.answers + [[dr["ask"]["question"], o]]); st.rerun()
    elif dr:
        for op in dr["ops"]:
            with st.container(border=True):
                if op["op"] == "add_rule": st.markdown(prose(op["rule"], op["target"], "location" if op["target"] in P["locations"] else "object"))
                elif op["op"] == "add_combination": st.markdown(prose(op["rule"], "game", "combo"))
                elif op["op"] == "set_string": st.markdown(f"New line **{op['id']}**: “{op['text']}”")
                elif op["op"] == "set_flag_default": st.markdown(f"Declare flag **{op['flag']}** (starts {op['value']})")
                elif op["op"] == "link_exit": st.markdown(f"Exit **{op['edge']['from']} → {op['edge']['to']}** via **{op['hotspot']}**")
                elif op["op"] == "add_mark": st.markdown(f"Mark **{op['mark']}** in {op['location']} at {op['pos']}")
        for e in dr["errors"]: st.error(e)
        c = st.columns(2)
        if c[0].button("Accept", type="primary", disabled=dr["proj"] is None or bool(dr["errors"])):
            reload(dr["proj"]); S.draft = None; st.rerun()
        if c[1].button("Discard"): S.draft = None; st.rerun()

elif page == "Check & export":
    st.header("Check & export")
    if st.button("Run the Check", type="primary"): S.issues, S.report = check.run_all(P)
    if S.get("issues") is not None:
        if not S.issues: st.success("No problems found.")
        for lvl, w, m in S.issues: (st.error if lvl == "error" else st.warning)(f"{w}: {m}")
        r = S.get("report")
        if r and r["ending"]: st.success(f"Solver explored {r['states']} states. Shortest route to the ending: " + " → ".join(r["ending_path"]))
    c = st.columns(2)
    if c[0].button("Export dev build (unencrypted)"): S.export = spec.export(P, config.folder("exports"), encrypted=False)
    if c[1].button("Export release build (encrypted)"):
        S.issues, S.report = check.run_all(P)
        if any(lvl == "error" for lvl, _, _ in S.issues): st.error("Release builds need a clean Check. Fix the errors above first.")
        else: S.export = spec.export(P, config.folder("exports"), encrypted=True)
    if S.get("export"):
        out, key = S.export
        st.download_button(f"Download {out.name}", out.read_bytes(), out.name)
        st.caption("Run it with the engine (the key is compiled in, so each release game gets its own build):")
        if os.name == "nt": env, lang = f'$env:GAME_KEY="{key}"; ' if key else "", "powershell"
        else: env, lang = f"GAME_KEY={key} " if key else "", "bash"
        st.code(f'cd engine\n{env}cargo run --release -- "{out.resolve()}"', language=lang)

elif page == "AI assets":
    st.header("AI assets")
    cfg = ai.settings()
    with st.expander("Backends", expanded=all(cfg[k] == "none" for k in ("llm", "tts", "image"))):
        c = st.columns(3)
        for col, k, title in zip(c, ("llm", "tts", "image"), ("Rule authoring", "Voices", "Art")):
            cfg[k] = col.radio(title, ["none", "onnx", "remote"], ["none", "onnx", "remote"].index(cfg[k]), key=f"be:{k}")
        for k in ("llm_model", "llm_url", "llm_name", "tts_models", "tts_url", "image_model", "image_model_inpaint", "image_url", "audio_url"):
            cfg[k] = st.text_input(k, cfg[k], key=f"cfg:{k}")
        cfg["api_key"] = st.text_input("API key for remote backends", cfg["api_key"], type="password")
        if st.button("Save settings"): ai.save_settings(cfg); st.success(f"Saved to {config.settings_file()}")

    def attempt(fn, *a):
        try: return fn(*a)
        except Exception as e: st.error(f"{type(e).__name__}: {e}" if not isinstance(e, RuntimeError) else str(e))

    voices, art, sheets, sound = st.tabs(["Voices", "Location and object art", "Sprite sheets", "Music and SFX (experimental)"])
    with voices:
        st.caption("Writes voice/<line id>.wav for each spoken line. Only lines whose text or voice changed are regenerated.")
        if st.button("Generate changed voice lines"):
            done = attempt(ai.voice_all, P, cfg)
            if done is not None: st.success(f"Generated {len(done)} lines."); st.write(done)
    with art:
        prompt = st.text_area("Describe the image", key="art_prompt")
        c = st.columns(3)
        rw, rh = G.get("resolution", [640, 360])
        size = (int(c[0].number_input("Width", value=int(rw))), int(c[1].number_input("Height", value=int(rh))))
        path = c[2].text_input("Save as", "assets/art.png")
        if st.button("Generate image", disabled=not prompt): S.art = attempt(ai.image, cfg, prompt, size)
        if S.get("art"):
            st.image(S.art)
            if st.button("Save image"): (ROOT / path).parent.mkdir(parents=True, exist_ok=True); S.art.save(ROOT / path); st.success(f"Saved {path}")
    with sheets:
        c = st.columns(4)
        rows, cols = c[0].selectbox("Rows (directions)", [8, 1]), int(c[1].number_input("Frames per row", 1, 16, 4))
        colors = int(c[2].number_input("Palette colours (0 = keep)", 0, 256, 0))
        path = c[3].text_input("Save sheet as", "characters/new/walk.png")
        desc = st.text_input("Describe the character")
        up = st.file_uploader("…or upload a grid image to clean up", type=["png", "jpg"])
        if st.button("Generate sheet", disabled=not desc):
            raw = attempt(ai.image, cfg, ai.sheet_prompt(desc, rows, cols), (cols * 96, rows * 128))
            if raw: S.sheet = ai.process_sheet(raw, rows, cols, colors)
        if up and st.button("Clean up upload"): S.sheet = ai.process_sheet(Image.open(up), rows, cols, colors)
        if S.get("sheet"):
            st.image(S.sheet, caption="Background removed, feet aligned to one pivot")
            c = st.columns(4)
            r, k = int(c[0].number_input("Row", 0, rows - 1, 0)), int(c[1].number_input("Frame", 0, cols - 1, 0))
            if c[2].button("Regenerate this frame", disabled=not desc):
                new = attempt(ai.regen_frame, cfg, S.sheet, rows, cols, r, k, ai.sheet_prompt(desc, rows, cols))
                if new: S.sheet = new; st.rerun()
            if c[3].button("Save sheet"): (ROOT / path).parent.mkdir(parents=True, exist_ok=True); S.sheet.save(ROOT / path); st.success(f"Saved {path}")
    with sound:
        prompt = st.text_input("Describe the music or sound", key="snd_prompt")
        c = st.columns(2)
        secs, path = c[0].number_input("Seconds", 1, 120, 10), c[1].text_input("Save as", "music/new.wav")
        if st.button("Generate audio", disabled=not prompt): attempt(ai.audio, cfg, prompt, secs, ROOT / path)
        if (ROOT / path).is_file(): st.audio(str(ROOT / path))

# Save whatever this run changed, keeping the previous version for undo (unless this run was the undo).
if S.dirty:
    if not S.pop("undoing", False) and S.proj != S.saved: S.undo = S.undo[-49:] + [S.saved]
    spec.save(S.proj); S.saved = copy.deepcopy(S.proj); S.dirty = False
