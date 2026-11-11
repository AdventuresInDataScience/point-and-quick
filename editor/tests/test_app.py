"""The editor itself, run headlessly with Streamlit's AppTest against copies of real projects in a
temporary folder (PNC_PROJECTS), so the projects in the repo are never touched.

AppTest drives Streamlit's own widgets. The location page's click-to-place image is a custom
component it can't click, so that needs a browser test."""
import shutil
from pathlib import Path
import pytest, yaml
from streamlit.testing.v1 import AppTest
import spec

EDITOR = Path(__file__).resolve().parents[1]


@pytest.fixture
def projects(tmp_path, monkeypatch):
    shutil.copytree(EDITOR / "examples/demo", tmp_path / "demo")
    shutil.copytree(EDITOR.parent / "tests/parity/projects/workshop", tmp_path / "workshop")   # no art, no interface block
    monkeypatch.setenv("PNC_PROJECTS", str(tmp_path))
    return tmp_path


def start(project):
    at = AppTest.from_file(str(EDITOR / "app.py"), default_timeout=60).run()
    at.sidebar.selectbox[0].set_value(project).run()
    assert not at.exception, [e.value for e in at.exception]
    return at


def files(root):
    return {p.relative_to(root).as_posix(): p.read_bytes() for p in sorted(root.rglob("*")) if p.is_file()}


def widget(items, label):
    return next(w for w in items if w.label == label)


@pytest.mark.parametrize("project", ["demo", "workshop"])
def test_every_page_renders_without_changing_files(projects, project):
    before = files(projects)
    at = start(project)
    for page in at.sidebar.radio[0].options:
        at.sidebar.radio[0].set_value(page).run()
        assert not at.exception, f"{page}: {[e.value for e in at.exception]}"
    assert files(projects) == before                 # merely looking never rewrites a file


def test_undo_steps_back_one_edit_at_a_time(projects):
    at = start("demo")
    title = lambda: yaml.safe_load((projects / "demo/strings/en.yaml").read_text(encoding="utf-8"))["str_title"]
    undo = lambda: next(b for b in at.sidebar.button if b.label == "Undo last change")
    original = title()
    widget(at.text_input, "Title").input("First edit").run()
    widget(at.text_input, "Title").input("Second edit").run()
    assert title() == "Second edit"
    undo().click().run()
    assert title() == "First edit" and widget(at.text_input, "Title").value == "First edit"
    undo().click().run()
    assert title() == original and not any(b.label == "Undo last change" for b in at.sidebar.button)


def test_interface_writes_only_the_chosen_styles_settings(projects):
    at = start("demo")
    stored = lambda: yaml.safe_load((projects / "demo/game.yaml").read_text(encoding="utf-8"))["interface"]
    icons = stored()["icons"]
    widget(at.radio, "Style").set_value("bar").run()
    assert stored() == {"style": "bar", "labels": "words", "icons": icons, "speech": "top"}
    widget(at.radio, "Verbs shown as").set_value("captioned").run()
    assert stored()["labels"] == "captioned"
    widget(at.radio, "Style").set_value("ring").run()
    assert stored() == {"style": "ring", "icons": icons, "speech": "top"}      # labels gone, icons kept
    assert not any(r.label == "Verbs shown as" for r in at.radio)
    widget(at.radio, "Spoken lines appear").set_value("over_speaker").run()
    assert stored()["speech"] == "over_speaker"
    assert not at.exception


def test_create_a_project_from_a_name(projects):
    at = start("demo")
    widget(at.sidebar.text_input, "New game").input("My Game")
    widget(at.sidebar.button, "Create project").click().run()
    assert (projects / "my_game/game.yaml").is_file()
    assert at.sidebar.selectbox[0].value == "my_game"               # switched to it
    assert widget(at.text_input, "Title").value == "My Game"         # the name became the title
    assert any("Created 'My Game'" in s.value for s in at.sidebar.success)
    for page in at.sidebar.radio[0].options:                         # a brand-new, empty project: every page works
        at.sidebar.radio[0].set_value(page).run()
        assert not at.exception, f"{page}: {[e.value for e in at.exception]}"


def test_create_explains_a_name_it_cant_use(projects):
    at = start("demo")
    for name, why in [("", "Type a name"), ("!!!", "Type a name"), ("Demo", "already a project called 'demo'")]:
        widget(at.sidebar.text_input, "New game").input(name)
        widget(at.sidebar.button, "Create project").click().run()
        assert any(why in e.value for e in at.sidebar.error), f"{name!r}: {[e.value for e in at.sidebar.error]}"
        assert at.sidebar.selectbox[0].value == "demo"
    assert sorted(p.name for p in projects.iterdir()) == ["demo", "workshop"]


def test_add_a_location_from_a_name(projects):
    at = start("demo")
    at.sidebar.radio[0].set_value("Locations").run()
    widget(at.text_input, "New location").input("Living Room")
    widget(at.button, "Add").click().run()
    P = spec.load(projects / "demo")
    assert P["strings"]["en"][P["locations"]["living_room"]["name"]] == "Living Room"     # the name is its display name
    assert widget(at.selectbox, "Location").value == "living_room"                         # and it's selected
    widget(at.text_input, "New location").input("Street")                                   # taken (case doesn't matter)
    widget(at.button, "Add").click().run()
    assert any("'street' is already a location's id" in e.value for e in at.error)
    assert not at.exception


def test_place_objects_in_a_room(projects):
    at = start("demo")
    at.sidebar.radio[0].set_value("Locations").run()                    # the house: its door, then the cake
    hot = lambda: spec.load(projects / "demo")["locations"]["house"]["hotspots"]
    widget(at.selectbox, "Layer for cake").set_value("back").run()
    assert hot()["cake"]["layer"] == "back"
    next(b for b in at.button if b.label == "▲" and b.key.endswith(":cake")).click().run()
    assert list(hot()) == ["cake", "house_door"]                         # moved back in the draw order
    next(b for b in at.button if (b.key or "").endswith("hotif:cake:add")).click().run()
    next(s for s in at.selectbox if (s.key or "").endswith("hotif:cake:0:flag:flag")).set_value("game_won").run()
    assert hot()["cake"]["if"] == [{"type": "flag", "flag": "game_won"}]
    assert hot()["cake"]["layer"] == "back"                              # the hotspot table didn't drop it
    assert not at.exception


def test_settings_choose_the_projects_folder_and_copy_an_example(tmp_path):
    import config                        # no PNC_PROJECTS here: the projects folder comes from the settings
    at = AppTest.from_file(str(EDITOR / "app.py"), default_timeout=60).run()
    assert any("no games in your projects folder" in i.value for i in at.info)       # first run: nothing yet
    games = tmp_path / "my games"
    widget(at.text_input, "Projects: where your games are kept").input(str(games)).run()
    next(b for b in at.button if b.key == "use:projects").click().run()
    assert config.stored()["projects"] == str(games)
    next(b for b in at.button if b.key == "example:demo").click().run()
    assert (games / "demo/game.yaml").is_file()
    assert at.sidebar.selectbox[0].value == "demo" and widget(at.text_input, "Title").value == "The Guard and the Cake"
    assert not at.exception
