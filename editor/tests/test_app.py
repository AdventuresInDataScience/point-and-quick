"""The editor itself, run headlessly with Streamlit's AppTest against copies of real projects in a
temporary folder (PNC_PROJECTS), so the projects in the repo are never touched.

AppTest drives Streamlit's own widgets. The location page's click-to-place image is a custom
component it can't click, so that needs a browser test."""
import shutil
from pathlib import Path
import pytest, yaml
from streamlit.testing.v1 import AppTest

EDITOR = Path(__file__).resolve().parents[1]


@pytest.fixture
def projects(tmp_path, monkeypatch):
    shutil.copytree(EDITOR / "projects/demo", tmp_path / "demo")
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
