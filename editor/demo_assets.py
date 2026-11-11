"""Draw placeholder art and synthesize audio for examples/demo, so it runs with no artist.
Run: python editor/demo_assets.py   (already run once; outputs are committed)"""
import math, struct, wave
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

D = Path(__file__).parent / "examples/demo"
W, H = 640, 360


def img(mode="RGB", color="black"): return Image.new(mode, (W, H), color)


def save(im, rel): (D / rel).parent.mkdir(parents=True, exist_ok=True); im.save(D / rel)


def gradient(top, bottom, y0=0, y1=H, im=None):
    """Vertical gradient between two RGB colours over rows y0..y1 (drawn into im)."""
    im = im or img()
    d = ImageDraw.Draw(im)
    for y in range(y0, y1):
        t = (y - y0) / max(1, y1 - y0 - 1)
        d.line([(0, y), (W, y)], fill=tuple(int(a + (b - a) * t) for a, b in zip(top, bottom)))
    return im


# ---------- street ----------
bg = gradient((90, 140, 210), (190, 210, 235), 0, 250)
d = ImageDraw.Draw(bg)
d.rectangle([300, 60, 600, 272], fill=(150, 70, 55))                 # house wall
for y in range(66, 272, 12):                                         # brick lines
    d.line([(300, y), (600, y)], fill=(120, 55, 45))
d.polygon([(290, 60), (450, 10), (610, 60)], fill=(70, 40, 40))       # roof
d.rectangle([400, 170, 450, 270], fill=(95, 60, 30))                 # closed door (part of the background)
d.ellipse([440, 218, 446, 224], fill=(220, 190, 60))                 # door knob
d.rectangle([330, 110, 380, 150], fill=(250, 230, 140))              # window
gradient((120, 120, 125), (80, 80, 85), 250, H, bg)                  # pavement
d.rectangle([200, 140, 214, 342], fill=(40, 40, 45))                 # lamp post (walk-behind)
d.ellipse([190, 120, 224, 146], fill=(255, 240, 170))
save(bg, "locations/street/bg.png")

walk = img(); dw = ImageDraw.Draw(walk)
dw.rectangle([10, 254, 630, 356], fill="white")
dw.rectangle([192, 332, 222, 350], fill="black")                     # lamp base blocks walking
save(walk, "locations/street/walk.png")

objs = img(); do = ImageDraw.Draw(objs)
do.rectangle([400, 170, 450, 270], fill="#ff0000")                   # door
do.rectangle([242, 291, 268, 310], fill="#00ff00")                   # key (padded for easy clicking)
save(objs, "locations/street/objects.png")

save(gradient((0, 0, 0), (255, 255, 255), 250, H, img()), "locations/street/depth.png")

lamp = img(); ImageDraw.Draw(lamp).rectangle([200, 120, 224, 342], fill="white")
save(lamp, "locations/street/behind_lamp.png")

# ---------- house ----------
bg = gradient((200, 180, 140), (170, 150, 115), 0, 245)
d = ImageDraw.Draw(bg)
gradient((120, 80, 50), (90, 55, 35), 245, H, bg)                    # floor
d.rectangle([60, 128, 112, 250], fill=(80, 50, 25))                  # exit door
d.rectangle([260, 210, 350, 222], fill=(110, 70, 40))                # table top
d.rectangle([266, 222, 274, 252], fill=(90, 55, 30)); d.rectangle([336, 222, 344, 252], fill=(90, 55, 30))
d.rectangle([420, 70, 540, 150], fill=(120, 160, 200))               # painting
save(bg, "locations/house/bg.png")

walk = img(); ImageDraw.Draw(walk).rectangle([10, 258, 630, 356], fill="white")
save(walk, "locations/house/walk.png")
objs = img(); do = ImageDraw.Draw(objs)
do.rectangle([60, 128, 112, 250], fill="#0000ff")
do.rectangle([286, 190, 318, 212], fill="#ffff00")
save(objs, "locations/house/objects.png")
save(gradient((0, 0, 0), (255, 255, 255), 245, H, img()), "locations/house/depth.png")

# ---------- object sprites ----------
s = Image.new("RGBA", (50, 100), (25, 18, 12, 255)); save(s, "objects/door/open.png")
s = Image.new("RGBA", (16, 8), (0, 0, 0, 0)); ds = ImageDraw.Draw(s)
ds.ellipse([0, 1, 6, 7], outline=(230, 200, 60), width=2); ds.rectangle([6, 3, 15, 4], fill=(230, 200, 60))
save(s, "objects/key/ground.png"); save(s.resize((32, 16), Image.NEAREST), "objects/key/icon.png")
s = Image.new("RGBA", (24, 16), (0, 0, 0, 0)); ds = ImageDraw.Draw(s)
ds.rectangle([1, 5, 22, 15], fill=(240, 220, 200)); ds.rectangle([1, 3, 22, 6], fill=(230, 90, 120))
ds.ellipse([10, 0, 14, 4], fill=(220, 40, 40))
save(s, "objects/cake/table.png"); save(s.resize((32, 22), Image.NEAREST), "objects/cake/icon.png")


# ---------- characters: 8 direction rows (e, se, s, sw, w, nw, n, ne), feet at bottom-centre ----------
def figure(shirt, hat, frame, direction):
    f = Image.new("RGBA", (32, 64), (0, 0, 0, 0)); d = ImageDraw.Draw(f)
    a = math.radians(direction * 45)
    step = [0, 3, 0, -3][frame % 4]
    d.rectangle([11 + step, 44, 14 + step, 63], fill=(40, 40, 60))   # legs
    d.rectangle([18 - step, 44, 21 - step, 63], fill=(40, 40, 60))
    d.rounded_rectangle([8, 22, 24, 46], 4, fill=shirt)              # body
    d.ellipse([9, 4, 23, 20], fill=(235, 195, 160))                  # head
    if hat: d.rectangle([8, 2, 24, 8], fill=hat)
    if direction not in (5, 6, 7):                                    # eyes unless facing away
        ex, ey = 16 + math.cos(a) * 4, 12 + max(0, math.sin(a)) * 2
        d.rectangle([ex - 3, ey, ex - 2, ey + 1], fill="black"); d.rectangle([ex + 2, ey, ex + 3, ey + 1], fill="black")
    return f


for who, shirt, hat in [("player", (60, 110, 200), None), ("guard", (40, 90, 50), (20, 40, 25))]:
    for anim, cols in [("idle", 1), ("walk", 4)]:
        sheet = Image.new("RGBA", (32 * cols, 64 * 8), (0, 0, 0, 0))
        for r in range(8):
            for c in range(cols): sheet.paste(figure(shirt, hat, c if anim == "walk" else 0, r), (c * 32, r * 64))
        save(sheet, f"characters/{who}/{anim}.png")


# ---------- audio: tiny synthesized loops and effects ----------
RATE = 11025


def write_wav(rel, samples):
    (D / rel).parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(D / rel), "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 12000)) for s in samples))


def melody(notes, beat):
    out = []
    for n in notes:
        f = 440 * 2 ** ((n - 69) / 12)
        for i in range(int(RATE * beat)):
            t = i / RATE; env = min(1, t * 40) * math.exp(-t * 3)
            out.append(0.5 * env * math.sin(2 * math.pi * f * t) + 0.2 * env * math.sin(4 * math.pi * f * t))
    return out


write_wav("music/street.wav", melody([60, 64, 67, 72, 67, 64, 62, 65, 69, 74, 69, 65], 0.4))
write_wav("music/house.wav", melody([57, 60, 64, 60, 55, 59, 62, 59], 0.6))
write_wav("sfx/creak.wav", [math.sin(2 * math.pi * (180 + 300 * i / RATE) * i / RATE) * (1 - i / (RATE * .6)) for i in range(int(RATE * .6))])
write_wav("sfx/pickup.wav", melody([84, 88], 0.12))


# ---------- verb icons for the interface: 48x48 (drawn at 24 game px, so sharp at 2x) ----------
# Each is drawn as a white-on-black mask at 4x, given a dark outline by dilating the mask, then
# scaled down. Coordinates are on the 48-pixel grid; fill=0 cuts a hole.
def icon(rel, draw):
    S = 4
    m = Image.new("L", (48 * S, 48 * S), 0)
    draw(ImageDraw.Draw(m), lambda *v: [c * S for c in v])
    im = Image.new("RGBA", m.size, (0, 0, 0, 0))
    im.paste((30, 26, 40, 255), mask=m.filter(ImageFilter.MaxFilter(13)))   # ~1.5 px outline
    im.paste((240, 232, 210, 255), mask=m)
    save(im.resize((48, 48), Image.LANCZOS), rel)


def walk_icon(d, s):        # two footprints
    for x, y in [(8, 14), (28, 6)]:
        d.ellipse(s(x, y, x + 12, y + 14), fill=255); d.ellipse(s(x + 2, y + 16, x + 10, y + 26), fill=255)


def look_icon(d, s):        # an eye
    t = [i / 20 for i in range(21)]
    d.polygon([tuple(s(5 + 38 * u, 24 - 13 * math.sin(math.pi * u))) for u in t] +
              [tuple(s(5 + 38 * u, 24 + 13 * math.sin(math.pi * u))) for u in reversed(t)], fill=255)
    d.ellipse(s(16, 16, 32, 32), fill=0); d.ellipse(s(20, 20, 28, 28), fill=255)


def talk_icon(d, s):        # a speech bubble
    d.rounded_rectangle(s(5, 7, 43, 33), radius=8 * 4, fill=255)
    d.polygon([tuple(s(13, 30)), tuple(s(10, 43)), tuple(s(24, 32))], fill=255)
    for x in (15, 24, 33): d.ellipse(s(x - 3, 17, x + 3, 23), fill=0)


def use_icon(d, s):         # a cog
    for k in range(8):
        a = k * math.pi / 4
        pts = [(r * math.cos(a) - w * math.sin(a), r * math.sin(a) + w * math.cos(a)) for r, w in [(12, -4), (21, -3.5), (21, 3.5), (12, 4)]]
        d.polygon([tuple(s(24 + x, 24 + y)) for x, y in pts], fill=255)
    d.ellipse(s(9, 9, 39, 39), fill=255); d.ellipse(s(18, 18, 30, 30), fill=0)


def take_icon(d, s):        # an arrow lifting out of a tray
    d.rectangle(s(7, 30, 41, 43), fill=255); d.rectangle(s(12, 30, 36, 38), fill=0)
    d.rectangle(s(20, 14, 28, 33), fill=255); d.polygon([tuple(s(10, 18)), tuple(s(24, 3)), tuple(s(38, 18))], fill=255)


for verb, fn in [("walk", walk_icon), ("look", look_icon), ("talk", talk_icon), ("use", use_icon), ("take", take_icon)]:
    icon(f"ui/{verb}.png", fn)
print("demo assets written to", D)
