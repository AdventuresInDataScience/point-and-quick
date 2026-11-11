"""Where the editor keeps things (config.py): settings and folders outside the editor's own folder,
folders that can be set, and examples copied rather than edited."""
from pathlib import Path
import ai, config, spec


def test_nothing_defaults_to_inside_the_editor():
    s = config.settings()
    for kind in config.FOLDERS: assert not Path(s[kind]).is_relative_to(config.HERE), kind
    assert not config.settings_file().is_relative_to(config.HERE)


def test_a_folder_setting_persists_until_cleared(tmp_path):
    config.update(projects=str(tmp_path / "games"))
    assert config.settings()["projects"] == str(tmp_path / "games")
    assert config.folder("projects") == tmp_path / "games" and (tmp_path / "games").is_dir()
    config.update(projects=None)                                     # back to the default
    assert config.settings()["projects"] == config.defaults()["projects"] and "projects" not in config.stored()


def test_pnc_projects_overrides_the_setting_for_one_run(monkeypatch, tmp_path):
    config.update(projects=str(tmp_path / "set"))
    monkeypatch.setenv("PNC_PROJECTS", str(tmp_path / "override"))
    assert config.folder("projects") == tmp_path / "override"
    assert config.stored()["projects"] == str(tmp_path / "set")      # the setting itself is untouched


def test_copying_an_example_makes_a_new_project(monkeypatch, tmp_path):
    monkeypatch.setenv("PNC_PROJECTS", str(tmp_path))
    assert config.copy_example("demo") == "demo"
    assert config.copy_example("demo") == "demo_2"                   # never over an existing project
    assert spec.load(tmp_path / "demo_2")["game"]["id"] == "demo_2"  # its id matches its folder
    assert config.projects() == ["demo", "demo_2"]
    assert (config.EXAMPLES / "demo/game.yaml").is_file()            # the example is still there


def test_ai_settings_live_in_the_settings_file():
    cfg = ai.settings(); cfg["llm"] = "remote"; ai.save_settings(cfg)
    assert config.stored()["ai"]["llm"] == "remote" and ai.settings()["llm"] == "remote"
    assert Path(ai.settings()["llm_model"]).is_relative_to(config.settings()["models"])
