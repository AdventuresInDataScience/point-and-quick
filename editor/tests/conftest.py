# The editor's modules import each other by bare name (import spec, check), as app.py does.
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
