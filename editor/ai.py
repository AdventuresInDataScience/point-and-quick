"""AI plugins. Each feature is one function that dispatches on a backend named in ai.yaml:
"onnx" runs a local model through ONNX Runtime, "remote" posts to an HTTP API, "none" is off.
Models are optional downloads; their libraries are only imported when a feature is used."""
import base64, copy, hashlib, io, json, re, urllib.request, wave
from pathlib import Path
import numpy as np, yaml
from PIL import Image
import check, spec

CFG = Path(__file__).parent / "ai.yaml"
DEFAULTS = {
    "llm": "none", "llm_model": "models/llm", "llm_url": "http://localhost:8080/v1/chat/completions", "llm_name": "local",
    "tts": "none", "tts_models": "models/piper", "tts_url": "",
    "image": "none", "image_model": "models/sd", "image_model_inpaint": "models/sd-inpaint", "image_url": "",
    "audio_url": "", "api_key": "",
}
_models = {}                         # loaded models, kept for the session


def settings(): return {**DEFAULTS, **(yaml.safe_load(CFG.read_text(encoding="utf-8")) if CFG.exists() else {})}
def save_settings(cfg): CFG.write_text(yaml.safe_dump(cfg, sort_keys=False), encoding="utf-8")


def post(cfg, url, payload):
    """POST JSON to a remote backend; returns the raw response body."""
    head = {"Content-Type": "application/json", **({"Authorization": f"Bearer {cfg['api_key']}"} if cfg["api_key"] else {})}
    with urllib.request.urlopen(urllib.request.Request(url, json.dumps(payload).encode(), head), timeout=600) as r:
        return r.read()


def b64png(im):
    buf = io.BytesIO(); im.save(buf, "PNG"); return base64.b64encode(buf.getvalue()).decode()


# ======================= LLM-assisted authoring =======================

def llm(cfg, system, user, schema):
    """Model text constrained to a JSON Schema, so invalid structure can't be produced."""
    if cfg["llm"] == "onnx":                 # small local model via onnxruntime-genai (>= 0.9)
        import onnxruntime_genai as og
        if "llm" not in _models: m = og.Model(cfg["llm_model"]); _models["llm"] = (m, og.Tokenizer(m))
        m, tok = _models["llm"]
        p = og.GeneratorParams(m)
        p.set_search_options(max_length=8192)
        p.set_guidance("json_schema", json.dumps(schema))
        gen = og.Generator(m, p)
        msgs = json.dumps([{"role": "system", "content": system}, {"role": "user", "content": user}])
        gen.append_tokens(tok.encode(tok.apply_chat_template(messages=msgs, add_generation_prompt=True)))
        out = []
        while not gen.is_done():
            gen.generate_next_token(); out.append(gen.get_next_tokens()[0])
        return tok.decode(out)
    if cfg["llm"] == "remote":               # any OpenAI-compatible server: llama.cpp, vLLM, Ollama, hosted APIs
        body = {"model": cfg["llm_name"], "temperature": 0.2,
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
                "response_format": {"type": "json_schema", "json_schema": {"name": "patch", "schema": schema}}}
        return json.loads(post(cfg, cfg["llm_url"], body))["choices"][0]["message"]["content"]
    raise RuntimeError("No LLM backend is set. Choose one on the AI assets page.")


def system_prompt():
    kinds = lambda t: "; ".join(f"{k}({', '.join(v)})" for k, v in t.items())
    return f"""You edit a point-and-click adventure spec by emitting JSON patch operations.
Logic is data: every rule reads "when VERB on TARGET (with ITEM), if CONDITIONS, do ACTIONS".
- The target owns the interaction: "use key on door" is add_rule with target "door" and "with": "key".
- Item-on-item combinations use add_combination. New exits use link_exit.
- Only use IDs from the context. Declare any new flag with set_flag_default before using it.
- Player-facing text never goes in rules: invent an id like str_door_creaks and add it with set_string.
- Rules are checked top to bottom and the first match wins, so give specific rules conditions.
- If the intent is ambiguous (e.g. "leaves the room" or "leaves for good"), reply with a single
  {{"op": "ask"}} holding a question and 2-4 short options instead of guessing.
Condition types: {kinds(spec.CONDS)}.
Action types: {kinds(spec.ACTS)}."""


def context(P, intent):
    """Only the slice of the project the intent mentions, so small local models stay effective."""
    words = set(re.findall(r"[a-z0-9_]+", intent.lower()))
    names, sheets = {}, {}
    for kind in ("locations", "objects", "characters"):
        for i in P[kind]:
            names[i] = spec.name(P, i)
            if i in words or set(names[i].lower().split()) & words: sheets[f"{kind}/{i}"] = P[kind][i]
    I = spec.ids(P)
    return {"verbs": I["verb"], "flags": P["game"].get("flags", {}), "items": I["item"], "names": names,
            "object_states": {o: list(d["states"]) for o, d in P["objects"].items()},
            "marks": {l: list(d.get("marks", {})) for l, d in P["locations"].items()},
            "edges": P["map"].get("edges", []), "sheets": sheets}


def patch_schema(P):
    """The patch schema with targets restricted to real IDs."""
    defs = copy.deepcopy(spec.SCHEMA["$defs"])
    for op in defs["patch"]["properties"]["ops"]["items"]["oneOf"]:
        if op["properties"]["op"]["const"] == "add_rule":
            op["properties"]["target"] = {"enum": [*P["objects"], *P["characters"], *P["locations"]]}
    return {"$defs": defs, **defs["patch"]}


def author(P, intent, cfg, answers=()):
    """Loose intent -> patch ops, run through the Check with automatic repair passes.
    Returns {"ask": op} for a clarifying question, else {"ops", "proj" (patched copy or None), "errors"}."""
    base, schema = set(check.check(P)), patch_schema(P)
    ops, errors, new = [], [], None
    for _ in range(3):
        user = json.dumps({"context": context(P, intent), "intent": intent, "answers": list(answers),
                           "previous_ops": ops, "errors_to_fix": errors})
        ops = json.loads(llm(cfg, system_prompt(), user, schema))["ops"]
        asks = [o for o in ops if o["op"] == "ask"]
        if asks: return {"ask": asks[0]}
        try: new = spec.apply_patch(P, ops)
        except (KeyError, StopIteration, TypeError) as e:     # e.g. an ID the model made up
            new, errors = None, [f"patch could not be applied: {e!r}"]; continue
        errors = [f"{w}: {m}" for lvl, w, m in check.check(new) if lvl == "error" and (lvl, w, m) not in base]
        if not errors: break
    return {"ops": ops, "proj": new, "errors": errors}


# ======================= voices =======================

def tts(cfg, text, voice, out, root):
    """Speak `text` in a character's voice into a WAV file."""
    out.parent.mkdir(parents=True, exist_ok=True)
    if cfg["tts"] == "onnx":                 # Piper voices are ONNX models (pip install piper-tts)
        from piper import PiperVoice
        path = str(Path(cfg["tts_models"]) / voice.get("model", ""))
        if path not in _models: _models[path] = PiperVoice.load(path)
        with wave.open(str(out), "wb") as w: _models[path].synthesize_wav(text, w)
    elif cfg["tts"] == "remote":             # e.g. an F5-TTS / OpenVoice server: zero-shot clone from the reference WAV
        ref = voice.get("ref_wav")
        wav = base64.b64encode((root / ref).read_bytes()).decode() if ref else ""
        out.write_bytes(post(cfg, cfg["tts_url"], {"text": text, "voice": voice.get("model", ""), "ref_wav": wav}))
    else: raise RuntimeError("No TTS backend is set. Choose one on the AI assets page.")


def voice_all(P, cfg):
    """Write voice/<line id>.wav for every spoken line whose text or voice changed since last run.
    Hashes live in voice/.hashes.json, so editing one line regenerates only that line."""
    root = Path(P["root"]); hp = root / "voice/.hashes.json"
    hashes = json.loads(hp.read_text(encoding="utf-8")) if hp.exists() else {}
    tab, player = P["strings"][P["game"].get("lang", "en")], P["game"]["player"]
    speaker = {}                             # line id -> character who says it
    for c, fs in P["dialogue"].items():
        for d in fs.values():
            for n in d.get("nodes", {}).values():
                if n.get("say"): speaker[n["say"]] = c
                for o in n.get("options", []): speaker[o["line"]] = player
    for _, _, _, acts in spec.blocks(P):
        for a in acts:
            if a["type"] == "say": speaker[a["line"]] = a.get("who") or player
    done = []
    for line, who in speaker.items():
        voice = P["characters"].get(who, {}).get("voice", {})
        h = hashlib.sha1(json.dumps([tab.get(line, ""), voice], sort_keys=True).encode()).hexdigest()
        if not tab.get(line) or hashes.get(line) == h: continue
        tts(cfg, tab[line], voice, root / f"voice/{line}.wav", root)
        hashes[line] = h; done.append(line)
    hp.parent.mkdir(parents=True, exist_ok=True); hp.write_text(json.dumps(hashes, indent=1), encoding="utf-8")
    return done


# ======================= art and sprite sheets =======================

def image(cfg, prompt, size, init=None, mask=None):
    """Text -> image, or repaint the white area of `mask` on `init` (inpainting)."""
    if cfg["image"] == "onnx":               # Stable Diffusion exported to ONNX (pip install optimum[onnxruntime])
        from optimum.onnxruntime import ORTStableDiffusionInpaintPipeline, ORTStableDiffusionPipeline
        kind = "inpaint" if mask else "txt2img"
        if kind not in _models:
            pipe = ORTStableDiffusionInpaintPipeline if mask else ORTStableDiffusionPipeline
            _models[kind] = pipe.from_pretrained(cfg["image_model_inpaint" if mask else "image_model"])
        args = {"prompt": prompt, "width": size[0] // 8 * 8, "height": size[1] // 8 * 8}
        if mask: args.update(image=init, mask_image=mask)
        return _models[kind](**args).images[0].resize(size)
    if cfg["image"] == "remote":
        body = {"prompt": prompt, "width": size[0], "height": size[1]}
        if mask: body.update(image=b64png(init), mask=b64png(mask))
        return Image.open(io.BytesIO(post(cfg, cfg["image_url"], body))).resize(size)
    raise RuntimeError("No image backend is set. Choose one on the AI assets page.")


def sheet_prompt(desc, rows, cols):
    """Ask for the whole 8-direction frameset as one grid, in the engine's row order."""
    dirs = "east, south-east, south, south-west, west, north-west, north, north-east"
    return (f"{desc}, pixel art character sprite sheet, grid of {rows} rows by {cols} columns, "
            f"each row one facing direction in order: {dirs}; full body, feet visible, plain flat background")


def slice_grid(im, rows, cols):
    w, h = im.width // cols, im.height // rows
    return [im.crop((c * w, r * h, (c + 1) * w, (r + 1) * h)).convert("RGBA") for r in range(rows) for c in range(cols)]


def join_grid(frames, rows, cols):
    w, h = frames[0].size; out = Image.new("RGBA", (w * cols, h * rows))
    for i, f in enumerate(frames): out.paste(f, ((i % cols) * w, (i // cols) * h))
    return out


def remove_bg(f, tol=48):
    """Key out the background: whatever is close to the median colour of the frame's border."""
    a = np.array(f.convert("RGBA")).astype(int)
    border = np.concatenate([a[0], a[-1], a[:, 0], a[:, -1]])[:, :3]
    far = np.abs(a[..., :3] - np.median(border, axis=0)).sum(-1) >= tol
    a[..., 3] = np.where(far, a[..., 3], 0)
    return Image.fromarray(a.astype(np.uint8))


def align_feet(frames):
    """Move each frame so its feet (centre of the lowest opaque rows) sit on one pivot: no walk jitter."""
    out = []
    for f in frames:
        ys, xs = np.nonzero(np.array(f)[..., 3] > 0)
        if not len(ys): out.append(f); continue
        bottom = ys.max(); feet = int(xs[ys >= bottom - 2].mean())
        g = Image.new("RGBA", f.size); g.paste(f, (f.width // 2 - feet, f.height - 1 - bottom)); out.append(g)
    return out


def quantize(sheet, colors):
    """One shared palette across every frame, for a consistent look."""
    alpha = sheet.getchannel("A")
    out = sheet.convert("RGB").quantize(colors).convert("RGBA"); out.putalpha(alpha)
    return out


def process_sheet(im, rows, cols, colors=0):
    """Grid image -> engine-ready sheet: slice, remove background, align feet, optional palette."""
    sheet = join_grid(align_feet([remove_bg(f) for f in slice_grid(im, rows, cols)]), rows, cols)
    return quantize(sheet, colors) if colors else sheet


def regen_frame(cfg, sheet, rows, cols, r, c, prompt):
    """Repaint one bad frame by inpainting just its cell, so the other frames act as the reference."""
    w, h = sheet.width // cols, sheet.height // rows
    box = (c * w, r * h, (c + 1) * w, (r + 1) * h)
    mask = Image.new("L", sheet.size, 0); mask.paste(255, box)
    flat = Image.new("RGB", sheet.size, "white"); flat.paste(sheet, mask=sheet.getchannel("A"))
    frame = align_feet([remove_bg(image(cfg, prompt, sheet.size, flat, mask).crop(box))])[0]
    out = sheet.copy(); out.paste(frame, box[:2]); return out


# ======================= music and sfx (experimental) =======================

def audio(cfg, prompt, seconds, out):
    """Generative audio is the least mature; this only talks to a remote server (e.g. MusicGen)."""
    if not cfg["audio_url"]: raise RuntimeError("Set an audio server URL on the AI assets page.")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(post(cfg, cfg["audio_url"], {"prompt": prompt, "seconds": seconds}))
