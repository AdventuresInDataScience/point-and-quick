"""The editor in a real browser. Streamlit is served for real on a copy of the demo and driven with
Playwright, through the Edge that ships with Windows (or Chrome, or Playwright's own Chromium).
Skipped when Playwright or a browser isn't available.

This covers what AppTest (test_app.py) can't: what the pages actually render, and the location
page's picture, a custom component that only a browser can click."""
import os, re, shutil, socket, subprocess, sys, time
from io import BytesIO
from pathlib import Path
import pytest, yaml
from PIL import Image

sync_api = pytest.importorskip("playwright.sync_api")
EDITOR = Path(__file__).resolve().parents[1]
PAGES = ["Game", "Map", "Locations", "Objects", "Characters", "Dialogue", "Strings", "Assets", "Assistant", "Check & export", "AI assets"]


def launch(p):
    for kw in ({"channel": "msedge"}, {"channel": "chrome"}, {}):
        try: return p.chromium.launch(headless=True, **kw)
        except Exception: pass
    pytest.skip("no browser for Playwright: needs Edge, Chrome, or `playwright install chromium`")


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0)); return s.getsockname()[1]


@pytest.fixture(scope="module")
def editor(tmp_path_factory):
    """The editor served against a temporary copy of the demo. Yields (open, projects folder);
    open() starts a fresh session, so no widget state carries over between tests."""
    projects = tmp_path_factory.mktemp("projects")
    shutil.copytree(EDITOR / "examples/demo", projects / "demo")
    port = free_port()
    # SSLKEYLOGFILE makes some Windows Pythons' OpenSSL abort (no OPENSSL_Applink); the server doesn't need it
    home = tmp_path_factory.mktemp("home")       # its own settings and home folder, like every test (conftest.py)
    env = {**{k: v for k, v in os.environ.items() if k != "SSLKEYLOGFILE"}, "PNC_PROJECTS": str(projects),
           "PNC_CONFIG": str(home / "config"), "HOME": str(home), "USERPROFILE": str(home)}
    server = subprocess.Popen([sys.executable, "-m", "streamlit", "run", str(EDITOR / "app.py"), "--server.headless", "true",
                               "--server.port", str(port), "--browser.gatherUsageStats", "false"],
                              env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        deadline = time.time() + 60
        while True:
            try: socket.create_connection(("127.0.0.1", port), 1).close(); break
            except OSError:
                if time.time() > deadline: pytest.fail("the editor didn't start")
                time.sleep(0.3)
        with sync_api.sync_playwright() as p:
            browser = launch(p)

            def open_editor():
                page = browser.new_page(viewport={"width": 1500, "height": 950})
                errors = []
                page.on("pageerror", lambda e: errors.append(str(e)))
                page.goto(f"http://127.0.0.1:{port}")
                page.get_by_role("heading", name="Game", exact=True).wait_for(timeout=60_000)
                settle(page)
                return page, errors

            yield open_editor, projects
            browser.close()
    finally:
        server.terminate(); server.wait(10)


def settle(page):
    """Wait until Streamlit has finished the run the last action started."""
    page.wait_for_timeout(300)
    page.wait_for_selector('[data-test-script-state="notRunning"]', timeout=30_000)
    page.wait_for_timeout(200)


def go(page, name):
    page.locator('[data-testid="stSidebar"]').get_by_text(name, exact=True).click()
    page.get_by_role("heading", name=name, exact=True).wait_for()
    settle(page)


def picture(page):
    """The location picture: an <img> inside the component's iframe."""
    img = page.frame_locator("iframe").first.locator("img")
    img.wait_for()
    return img


def click_picture(page, x, y, size=(640, 360)):
    """Click the picture at a point given in its own pixels (it's shown stretched to its column)."""
    img = picture(page)
    box = img.bounding_box()
    img.click(position={"x": x * box["width"] / size[0], "y": y * box["height"] / size[1]})
    settle(page)


def test_every_page_renders_cleanly(editor):
    page, errors = editor[0]()
    for name in PAGES:
        go(page, name)
        assert page.locator('[data-testid="stException"]').count() == 0, f"{name} shows an exception"
    assert errors == []


def test_the_preview_draws_objects_in_their_starting_state(editor):
    page, _ = editor[0]()
    go(page, "Locations")                      # the house, where the cake sits on the table
    shot = Image.open(BytesIO(picture(page).screenshot())).convert("RGB")
    r, g, b = shot.getpixel((int(303 * shot.width / 640), int(206 * shot.height / 360)))    # the middle of the cake
    assert abs(r - 240) + abs(g - 220) + abs(b - 200) < 60, f"no cake at its hotspot's pos, got {(r, g, b)}"


def test_place_a_mark_by_clicking_the_picture(editor):
    page, errors = editor[0]()
    go(page, "Locations")
    click_picture(page, 300, 300)
    name = page.get_by_role("textbox", name=re.compile("^Name the mark"))
    # beside the picture, level with it, not under it where it can fall below the fold
    pic, box = picture(page).bounding_box(), name.bounding_box()
    assert box["x"] >= pic["x"] + pic["width"] and pic["y"] <= box["y"] <= pic["y"] + pic["height"], (pic, box)
    name.fill("browser_mark"); name.press("Enter"); settle(page)
    page.get_by_role("button", name="Add mark", exact=True).click(); settle(page)
    marks = yaml.safe_load((editor[1] / "demo/locations/house/sheet.yaml").read_text(encoding="utf-8"))["marks"]
    x, y = marks["browser_mark"]
    assert abs(x - 300) <= 2 and abs(y - 300) <= 2, marks["browser_mark"]
    assert errors == []


def test_fill_a_polygon_into_the_walkway(editor):
    page, errors = editor[0]()
    go(page, "Locations")
    page.get_by_text("add a polygon point", exact=True).click(); settle(page)
    for x, y in [(40, 40), (160, 40), (100, 120)]:             # up in the house's wall, nowhere walkable
        click_picture(page, x, y)
    page.get_by_role("button", name="Fill", exact=True).click(); settle(page)     # the walkway is the default target
    walk = Image.open(editor[1] / "demo/locations/house/walk.png").convert("L")
    assert walk.getpixel((100, 70)) == 255 and walk.getpixel((300, 150)) == 0
    assert errors == []


def test_create_a_project_by_pressing_enter(editor):
    # last in the file: the new project then sits in the shared folder (it sorts after "demo", so
    # fresh sessions still open on the demo)
    page, errors = editor[0]()
    box = page.get_by_role("textbox", name="New game")
    box.fill("Demo"); box.press("Enter"); settle(page)                # taken: says so, and keeps the text to fix
    sync_api.expect(page.get_by_text("There's already a project called 'demo'.")).to_be_visible()
    sync_api.expect(box).to_have_value("Demo")
    box.fill("Test Game"); box.press("Enter"); settle(page)
    sync_api.expect(page.get_by_role("textbox", name="Title")).to_have_value("Test Game")
    sync_api.expect(box).to_have_value("")                              # emptied once it worked
    assert (editor[1] / "test_game/game.yaml").is_file()
    assert errors == []
