# The editor's modules import each other by bare name (import spec, check), as app.py does.
import sys
from pathlib import Path
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


@pytest.fixture(autouse=True)
def private_settings(tmp_path_factory, monkeypatch):
    """Every test gets its own settings folder and home folder, so nothing reads or writes the
    real settings.yaml, or makes folders in the real Documents (see config.py)."""
    home = tmp_path_factory.mktemp("home")
    monkeypatch.setenv("PNC_CONFIG", str(home / "config"))
    for var in ("HOME", "USERPROFILE"): monkeypatch.setenv(var, str(home))
    return home
