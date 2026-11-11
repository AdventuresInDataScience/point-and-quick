"""Draw placeholder art and synthesize audio for projects/demo, so it runs with no artist.
Run: python editor/demo_assets.py   (already run once; outputs are committed)"""
import math, struct, wave
from pathlib import Path
from PIL import Image, ImageDraw

D = Path(__file__).parent / "projects/demo"
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
print("demo assets written to", D)
