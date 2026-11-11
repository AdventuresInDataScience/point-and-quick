"""Where the editor keeps things. Nothing is written inside the editor's own folder, so it works the
same from a source checkout or an installed package (whose folder may be read-only).

  settings  settings.yaml in the user's config folder: %APPDATA%/Point and Click Maker on Windows,
            ~/Library/Application Support/Point and Click Maker on macOS, and
            $XDG_CONFIG_HOME/point-and-click-maker (or ~/.config/...) elsewhere. PNC_CONFIG
            overrides the folder (the tests use a temporary one).
  projects  where games are made, exports where their zips go, and models where AI models live.
  exports   Each can be set on the editor's Settings page; by default they're projects/, exports/
  models    and models/ in Documents/Point and Click Maker. PNC_PROJECTS overrides the projects
            folder for one run without changing the settings (the tests use this too).
  examples  ship with the editor (editor/examples) and are never edited: copying one makes a project.
"""
import os, shutil, sys
from pathlib import Path
import yaml
import spec

HERE = Path(__file__).parent
EXAMPLES = HERE / "examples"
LEGACY = HERE / "projects"          # where projects lived before the folder could be set
APP = "Point and Click Maker"
FOLDERS = ("projects", "exports", "models")


def config_dir():
    if os.environ.get("PNC_CONFIG"): return Path(os.environ["PNC_CONFIG"])
    if sys.platform == "win32": return Path(os.environ.get("APPDATA") or Path.home() / "AppData/Roaming") / APP
    if sys.platform == "darwin": return Path.home() / "Library/Application Support" / APP
    return Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "point-and-click-maker"


def settings_file(): return config_dir() / "settings.yaml"


def stored():
    """Only what the user has set: the contents of settings.yaml."""
    f = settings_file()
    return (yaml.safe_load(f.read_text(encoding="utf-8")) or {}) if f.is_file() else {}


def defaults():
    docs = Path.home() / "Documents"
    base = (docs if docs.is_dir() else Path.home()) / APP
    return {k: str(base / k) for k in FOLDERS}


def settings():
    """Every setting: what the user set, and the defaults for the rest. ai holds the AI backends."""
    return {**defaults(), "ai": {}, **stored()}


def update(**changes):
    """Set some settings (None clears one, so its default applies again) and write the file."""
    s = {**stored(), **changes}
    s = {k: v for k, v in s.items() if v is not None}
    settings_file().parent.mkdir(parents=True, exist_ok=True)
    settings_file().write_text(yaml.safe_dump(s, sort_keys=False, allow_unicode=True), encoding="utf-8")


def folder(kind):
    """The projects, exports or models folder, created if it isn't there yet."""
    p = Path(os.environ.get("PNC_PROJECTS") or settings()["projects"]) if kind == "projects" else Path(settings()[kind])
    p.mkdir(parents=True, exist_ok=True)
    return p


def projects(where=None):
    """The projects in a folder (the projects folder by default): folders with a game.yaml."""
    where = Path(where) if where else folder("projects")
    return sorted(p.name for p in where.iterdir() if (p / "game.yaml").is_file()) if where.is_dir() else []


def examples(): return projects(EXAMPLES)


def free_id(where, gid):
    """gid, or gid_2, gid_3… if a project of that name is already in the folder."""
    n, i = 2, gid
    while (Path(where) / i).exists(): i, n = f"{gid}_{n}", n + 1
    return i


def copy_example(name):
    """Copy an example into the projects folder as a new project. Returns its id."""
    dest = folder("projects"); gid = free_id(dest, name)
    shutil.copytree(EXAMPLES / name, dest / gid)
    if gid != name:                                   # its id has to match its folder
        P = spec.load(dest / gid); P["game"]["id"] = gid; spec.save(P)
    return gid


def legacy_projects():
    """Projects still in the editor's own folder (from before the projects folder could be set)."""
    return projects(LEGACY) if LEGACY.resolve() != folder("projects").resolve() else []


def move_legacy(names):
    """Move projects from the editor's own folder into the projects folder (renaming any whose
    name is taken there). Returns the new ids."""
    dest, moved = folder("projects"), []
    for name in names:
        gid = free_id(dest, name); shutil.move(str(LEGACY / name), str(dest / gid)); moved.append(gid)
        if gid != name: P = spec.load(dest / gid); P["game"]["id"] = gid; spec.save(P)
    return moved
