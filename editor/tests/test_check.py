"""The Check against the demo, and against copies of it broken in the ways it should catch."""
from pathlib import Path
import pytest
import check, spec

DEMO = Path(__file__).resolve().parents[1] / "examples/demo"


@pytest.fixture
def P():
    return spec.load(DEMO)          # edited in memory only; the demo on disk is never touched


def errors(P, **kw):
    issues, _ = check.run_all(P, **kw)
    return [m for lvl, _, m in issues if lvl == "error"]


def test_demo_is_clean(P):
    issues, r = check.run_all(P)
    assert issues == []
    assert r["ending"] and r["softlocks"] == 0 and not r["capped"]
    assert r["ending_path"] == ["take key", "talk guard", "say 'str_opt_who'", "say 'str_opt_cat'",
                                "use key on door", "walk door", "take cake"]


# --- tier 1: each breaks the demo one way and expects the error that names it

def test_typo_in_a_flag(P):
    P["objects"]["door"]["rules"][0]["if"][0]["flag"] = "guard_gone"
    assert "unknown flag 'guard_gone'" in errors(P)


def test_mark_off_the_walkway(P):
    P["locations"]["street"]["marks"]["key_spot"] = [5, 5]
    assert "mark 'key_spot' is off the walkway" in errors(P)


def test_missing_text(P):
    del P["strings"]["en"]["str_key_look"]
    assert "text 'str_key_look' missing from strings/en.yaml" in errors(P)


def test_unknown_field(P):
    P["objects"]["key"]["colour"] = "gold"
    assert any("colour" in m and "Additional properties" in m for m in errors(P))


def test_item_on_item_rule_on_an_item(P):
    P["objects"]["key"]["rules"].append({"verb": "use", "with": "cake", "do": []})
    assert "item-on-item combinations belong in game.yaml" in errors(P)


def test_dialogue_goto_to_nowhere(P):
    P["dialogue"]["guard"]["global"]["nodes"]["who"]["goto"] = "nowhere"
    assert "goto 'nowhere' doesn't exist here" in errors(P)


def test_hotspot_not_painted_and_with_no_sprite_in_some_state(P):
    P["locations"]["street"]["hotspots"]["door"]["color"] = "#123456"       # the locked door is painted into the background
    assert ("'door' can't be clicked while 'locked': it has no sprite then, and its colour #123456 isn't painted "
            "in the object mask") in errors(P)


def test_an_object_with_a_sprite_needs_no_painted_colour(P):
    P["locations"]["street"]["hotspots"]["key"]["color"] = "#123456"        # clicked on its sprite instead
    assert errors(P) == []


def test_placement_conditions_are_checked(P):
    P["locations"]["street"]["hotspots"]["key"]["if"] = [{"type": "flag", "flag": "no_such_flag"}]
    assert "unknown flag 'no_such_flag'" in errors(P)


def test_a_hidden_object_can_hide_the_ending(P):
    # the key only shows once the game is won: so the door never opens and the game can't be won
    P["locations"]["street"]["hotspots"]["key"]["if"] = [{"type": "flag", "flag": "game_won"}]
    assert "the ending can never be reached" in errors(P)


# --- tier 2: the solver

def test_softlock(P):
    # the guard confiscates the key: do that before opening the door and the game can't be finished
    P["characters"]["guard"]["rules"].append({"verb": "use", "with": "key", "do": [{"type": "remove", "item": "key"}]})
    assert any("softlocked states" in m and "use key on guard" in m for m in errors(P))


def test_unreachable_ending(P):
    P["objects"]["cake"]["rules"][0]["do"] = [a for a in P["objects"]["cake"]["rules"][0]["do"] if a["type"] != "set_flag"]
    assert "the ending can never be reached" in errors(P)


@pytest.mark.parametrize("cap", range(1, 19))
def test_capped_search_reports_no_false_errors(P, cap):
    # the demo is winnable and softlock-free, so however early the search stops, nothing is an error
    assert errors(P, max_states=cap) == []


def test_capped_search_still_finds_certain_softlocks(P):
    P["characters"]["guard"]["rules"].append({"verb": "use", "with": "key", "do": [{"type": "remove", "item": "key"}]})
    full = check.solve(P)
    capped = check.solve(P, max_states=full["states"] - 1)
    assert capped["capped"] and 0 < capped["softlocks"] <= full["softlocks"]


# --- the interface: each style reads only its own settings

def test_labels_are_a_bar_setting(P):
    P["game"]["interface"]["labels"] = "icons"                    # the demo uses the ring
    assert "'labels' is a bar setting; the ring style doesn't use it" in errors(P)


def test_icon_for_a_verb_the_game_lacks(P):
    P["game"]["interface"]["icons"]["dance"] = "ui/walk.png"
    assert "there's an icon for 'dance', which isn't one of the verbs" in errors(P)


def test_icon_file_must_exist(P):
    P["game"]["interface"]["icons"]["look"] = "ui/nope.png"
    assert "unknown file 'ui/nope.png'" in errors(P)


def test_verbs_without_icons_fall_back_to_their_names(P):
    P["game"]["interface"] = {"style": "bar", "labels": "captioned", "icons": {"look": "ui/look.png"}}
    issues, _ = check.run_all(P)
    assert ("warning", "game.yaml interface", "verb 'take' has no icon, so the bar shows its name instead") in issues
    assert not [i for i in issues if i[0] == "error"]


def test_interface_is_stored_with_only_its_styles_settings():
    g = {"verbs": ["walk", "look"], "interface": {"style": "ring", "labels": "icons", "icons": {"look": "a.png", "dance": "b.png"}}}
    assert spec.interface(g) == {"style": "ring", "icons": {"look": "a.png"}, "speech": "top"}
    assert spec.interface({"verbs": []}) == {"style": "bar", "labels": "words", "speech": "top"}


# --- files

def test_spec_files_are_utf8(tmp_path):
    text = "Café → north… “quoted” 猫"
    spec.dump(tmp_path / "s.yaml", {"str_x": text})
    assert (tmp_path / "s.yaml").read_bytes().decode("utf-8")        # what the engine requires
    assert spec.read(tmp_path / "s.yaml") == {"str_x": text}
