"""Parity tests, solver side: play every scenario in tests/parity/scenarios.yaml through check.py's
simulator and check the state it ends in. engine/src/tests.rs plays the same scenarios through the
engine's logic, so the two can't drift apart unnoticed."""
from pathlib import Path
import pytest, yaml
import check, spec

PARITY = Path(__file__).resolve().parents[2] / "tests/parity"
SCENARIOS = yaml.safe_load((PARITY / "scenarios.yaml").read_text(encoding="utf-8"))


@pytest.mark.parametrize("sc", SCENARIOS, ids=[s["name"] for s in SCENARIOS])
def test_scenario(sc):
    P = spec.load(PARITY / sc["project"])
    s, rel = check.start(P)
    for mv in sc["moves"]:
        offered = dict(check.moves(P, s, rel))
        assert mv in offered, f"the solver doesn't offer {mv!r}; it offers {sorted(offered)}"
        s = offered[mv]
    offered = dict(check.moves(P, s, rel))
    for mv in sc.get("blocked", []): assert mv not in offered, f"the solver offers {mv!r}, which should be blocked"
    e = sc.get("expect", {})
    if "location" in e: assert s["loc"] == e["location"]
    if "inventory" in e: assert sorted(s["inv"]) == sorted(e["inventory"])
    for f, v in e.get("flags", {}).items(): assert s["flags"].get(f, False) == v, f"flag {f}"
    for o, v in e.get("objects", {}).items(): assert s["objs"].get(o) == v, f"object {o}"
    for c, l in e.get("characters", {}).items(): assert s["chars"].get(c, "") == l, f"character {c}"
