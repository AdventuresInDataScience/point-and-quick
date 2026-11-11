//! logic.rs — the game rules: conditions, verb dispatch, the action queue and dialogue.
//! editor/check.py simulates exactly this to find softlocks; change the two together.
use crate::spec::{Action, Cond, Node, Opt, Rule, State};
use crate::G;
use std::collections::HashMap;

/// `x in v`, Python style.
pub fn has(v: &Vec<String>, x: &str) -> bool {
    v.iter().any(|i| i == x)
}

/// True if every condition holds.
pub fn ok(st: &State, conds: &Vec<Cond>) -> bool {
    for c in conds {
        let pass = match c {
            Cond::Flag { flag, is } => st.flags.get(flag).copied().unwrap_or(false) == *is,
            Cond::State { object, is } => st.objects.get(object) == Some(is),
            Cond::Has { item, is } => has(&st.inventory, item) == *is,
            Cond::At { character, location, is } => {
                st.characters.get(character).map(|p| &p.location == location).unwrap_or(false) == *is
            }
            Cond::Said { line, is } => has(&st.said, line) == *is,
        };
        if !pass {
            return false;
        }
    }
    true
}

/// Objects are drawn and clickable unless they're in the "gone" state.
pub fn active(g: &G, id: &str) -> bool {
    g.st.objects.get(id).map(|s| s != "gone").unwrap_or(true)
}

/// A target's rules and fallback lines (objects and characters both have them).
fn sheet(g: &G, id: &str) -> (Vec<Rule>, HashMap<String, String>) {
    if let Some(o) = g.spec.objects.get(id) {
        return (o.rules.clone(), o.fallback.clone());
    }
    if let Some(c) = g.spec.characters.get(id) {
        return (c.rules.clone(), c.fallback.clone());
    }
    (vec![], HashMap::new())
}

/// Verb dispatch, in this order:
/// 1. using an item on another carried item: the combinations in game.yaml
/// 2. the target owns the interaction: the first matching rule on its sheet wins
/// 3. talking to a character with no talk rule opens its dialogue
/// 4. fallback lines: the used item's, then the target's, then the game's default for the verb
pub fn interact(g: &mut G, verb: &str, target: &str, item: &str) {
    if verb == "use" && !item.is_empty() && item != target && has(&g.st.inventory, target) {
        for r in g.spec.game.combinations.clone() {
            if has(&r.items, target) && has(&r.items, item) && ok(&g.st, &r.cond) {
                g.queue.extend(r.act);
                return;
            }
        }
    }
    let (rules, fallback) = sheet(g, target);
    for r in rules {
        if r.verb == verb && r.with == item && ok(&g.st, &r.cond) {
            g.queue.extend(r.act);
            return;
        }
    }
    if verb == "talk" && g.spec.dialogue.contains_key(target) {
        g.queue.push(Action::Talk { character: target.to_string(), node: String::new() });
        return;
    }
    let mut lines = vec![];
    if !item.is_empty() {
        lines.push(sheet(g, item).1.get(verb).cloned().unwrap_or_default());
    }
    lines.push(fallback.get(verb).cloned().unwrap_or_default());
    lines.push(g.spec.game.defaults.get(verb).cloned().unwrap_or_default());
    for line in lines {
        if !line.is_empty() {
            g.queue.push(Action::Say { line, who: String::new() });
            return;
        }
    }
}

/// The player clicked an inventory item with a verb. With "use" and nothing picked yet, the item is
/// picked for "use X with ..."; clicking the picked item again uses it on its own. check.py's solver
/// offers exactly these moves: "use X (inventory)" is X alone, "use X on Y" is X picked, then Y.
pub fn click_item(g: &mut G, verb: &str, it: &str) {
    if verb == "use" && g.item.is_empty() {
        g.item = it.to_string();
        return;
    }
    let item = if verb == "use" && g.item != it { g.item.clone() } else { String::new() };
    interact(g, verb, it, &item);
    g.verb = "walk".to_string();
    g.item.clear();
}

/// The player has walked up to a target. Walking onto a hotspot that triggers an exit takes it
/// if the hotspot's object is in the exit's state (a locked door gates its exit); otherwise
/// it's a normal interaction (so a "walk" rule on a locked door can say "It's locked.").
pub fn arrive(g: &mut G, verb: &str, target: &str, item: &str) {
    if verb == "walk" {
        let exits = g.spec.locations[&g.st.location].exits.clone();
        for (edge, x) in exits {
            if x.hotspot == target && (x.state.is_empty() || g.st.objects.get(target) == Some(&x.state)) {
                g.queue.push(Action::Go { edge });
                return;
            }
        }
    }
    interact(g, verb, target, item);
}

/// Something is still playing out, so the next queued action has to wait.
pub fn busy(g: &G) -> bool {
    g.speech.is_some()
        || g.wait > 0.0
        || !g.fade_edge.is_empty()
        || !g.rooms.contains_key(&g.st.location)
        || (g.cut_walk && crate::walking(g, &g.spec.game.player))
}

/// Run queued actions in order until one blocks (a line being said, a walk, a wait, a fade).
/// This is also how cutscenes work: a rule's action list is the script.
pub fn run_queue(g: &mut G) {
    while !busy(g) && !g.queue.is_empty() {
        let a = g.queue.remove(0);
        match a {
            Action::Say { line, who } => {
                let who = if who.is_empty() { g.spec.game.player.clone() } else { who };
                let text = crate::text(g, &line);
                let secs = 1.5 + 0.06 * text.len() as f32;
                g.speech = Some((who, text, secs));
                if !has(&g.st.said, &line) {
                    g.st.said.push(line.clone());
                }
                crate::play_sound(g, &format!("voice/{line}.wav")); // the editor's TTS for this line, if exported
            }
            Action::SetFlag { flag, value } => {
                g.st.flags.insert(flag, value);
            }
            Action::SetState { object, state } => {
                g.st.objects.insert(object, state);
            }
            Action::Give { item } => {
                if !has(&g.st.inventory, &item) {
                    g.st.inventory.push(item);
                }
            }
            Action::Remove { item } => g.st.inventory.retain(|i| *i != item),
            Action::Move { character, location, mark } => crate::move_char(g, &character, &location, &mark),
            Action::CharState { character, state } => {
                if let Some(p) = g.st.characters.get_mut(&character) {
                    p.state = state;
                }
            }
            Action::Go { edge } => g.fade_edge = edge, // the main loop fades out, then calls take_edge
            Action::Walk { mark } => {
                let target = crate::mark_pos(g, &mark);
                let player = g.spec.game.player.clone();
                crate::walk_to(g, &player, target);
                g.cut_walk = true;
            }
            Action::Talk { character, node } => talk(g, &character, &node),
            Action::PlayMusic { file, fade } => {
                g.st.music = file.clone();
                crate::play_music(g, &file, fade);
            }
            Action::PlaySound { file } => crate::play_sound(g, &file),
            Action::Wait { seconds } => g.wait = seconds,
        }
    }
}

/// A character's dialogue as seen from the current location: that location's file first, the
/// global file as fallback (nodes merge, the location's win). Returns (start node, nodes).
pub fn dialogue(g: &G, character: &str) -> (String, HashMap<String, Node>) {
    let mut start = String::new();
    let mut nodes = HashMap::new();
    if let Some(files) = g.spec.dialogue.get(character) {
        for name in ["global", g.st.location.as_str()] {
            if let Some(d) = files.get(name) {
                if !d.start.is_empty() {
                    start = d.start.clone();
                }
                for (id, n) in &d.nodes {
                    nodes.insert(id.clone(), n.clone());
                }
            }
        }
    }
    (start, nodes)
}

/// Play a dialogue node: the character says its line and its actions run, ahead of anything
/// else queued. The player's options open once all of that has finished (see update_menu).
pub fn talk(g: &mut G, character: &str, node: &str) {
    let (start, nodes) = dialogue(g, character);
    let id = if node.is_empty() { start } else { node.to_string() };
    let Some(n) = nodes.get(&id) else { return };
    let mut front = vec![];
    if !n.say.is_empty() {
        front.push(Action::Say { line: n.say.clone(), who: character.to_string() });
    }
    front.extend(n.act.clone());
    front.extend(g.queue.drain(..));
    g.queue = front;
    g.menu = Some((character.to_string(), id));
}

/// The player's options at the open node, minus hidden ones and "once" ones already said.
pub fn options(g: &G) -> Vec<Opt> {
    let mut out = vec![];
    let Some((character, id)) = &g.menu else { return out };
    if let Some(n) = dialogue(g, character).1.get(id) {
        for o in &n.options {
            if ok(&g.st, &o.cond) && !(o.once && has(&g.st.said, &o.line)) {
                out.push(o.clone());
            }
        }
    }
    out
}

/// Each frame, once nothing is busy: a node with options stays open for the player; one
/// without follows its goto, or the conversation ends.
pub fn update_menu(g: &mut G) {
    let Some((character, id)) = g.menu.clone() else { return };
    if busy(g) || !g.queue.is_empty() || !options(g).is_empty() {
        return;
    }
    g.menu = None;
    let goto = dialogue(g, &character).1.get(&id).map(|n| n.goto.clone()).unwrap_or_default();
    if !goto.is_empty() {
        talk(g, &character, &goto);
    }
}

/// The player picked an option: they say it, then the conversation moves to its node.
pub fn choose(g: &mut G, o: &Opt) {
    let Some((character, _)) = g.menu.take() else { return };
    g.queue.push(Action::Say { line: o.line.clone(), who: String::new() });
    if !o.goto.is_empty() {
        g.queue.push(Action::Talk { character, node: o.goto.clone() });
    }
}

/// Arrive in a location at a mark. The room's music, characters and assets are set up, then
/// the first matching "enter" rule runs ahead of anything else queued.
pub fn enter(g: &mut G, location: &str, mark: &str) {
    g.st.location = location.to_string();
    g.st.mark = mark.to_string();
    g.st.music = String::new();
    crate::room_changed(g);
    for r in g.spec.locations[location].rules.clone() {
        if r.verb == "enter" && ok(&g.st, &r.cond) {
            let mut front = r.act;
            front.extend(g.queue.drain(..));
            g.queue = front;
            return;
        }
    }
}

/// Take a map edge (called at the bottom of the fade-out).
pub fn take_edge(g: &mut G, edge: &str) {
    let Some(e) = g.spec.edges.iter().find(|e| e.id == edge).cloned() else { return };
    enter(g, &e.to, &e.spawn);
}

/// NPCs standing in the current room: the state says they're here and their placement's
/// condition (if any) holds. Returns (character, mark, facing), sorted for a stable order.
pub fn npcs_here(g: &G) -> Vec<(String, String, String)> {
    let loc = &g.spec.locations[&g.st.location];
    let mut out = vec![];
    for (c, p) in &g.st.characters {
        if p.location != g.st.location || *c == g.spec.game.player {
            continue;
        }
        let placed = loc.npcs.iter().find(|n| n.character == *c);
        if let Some(n) = placed {
            if !ok(&g.st, &n.cond) {
                continue;
            }
        }
        let mark = if !p.mark.is_empty() { p.mark.clone() } else { placed.map(|n| n.mark.clone()).unwrap_or_default() };
        let facing = placed.map(|n| n.facing.clone()).unwrap_or_default();
        out.push((c.clone(), mark, facing));
    }
    out.sort();
    out
}
