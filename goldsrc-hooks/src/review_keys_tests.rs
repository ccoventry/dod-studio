use super::*;

const Y: i32 = b'y' as i32;

#[test]
fn the_keys_answer_only_while_live() {
    assert_eq!(decide(true, Y, KeyMode::Off, 0), (false, None, 0));
    assert_eq!(
        decide(true, Y, KeyMode::Live, 0),
        (true, Some(Action::Yes), 1)
    );
    assert_eq!(
        decide(true, b'b' as i32, KeyMode::Live, 0),
        (true, Some(Action::Back), 1 << 4)
    );
    // Any other key goes to the game, live or not; ESC included,
    // which vgui2 takes before this hook anyway.
    assert_eq!(decide(true, 27, KeyMode::Live, 0), (false, None, 0));
    assert_eq!(
        decide(true, b'w' as i32, KeyMode::Live, 0),
        (false, None, 0)
    );
}

#[test]
fn e_opens_the_form_only_while_the_keys_are_live() {
    let e = b'e' as i32;
    assert_eq!(
        decide(true, e, KeyMode::Live, 0),
        (true, Some(Action::Form), 1 << KEYS_LIST.len())
    );
    // E is +use with no review highlight on.
    assert_eq!(decide(true, e, KeyMode::Off, 0), (false, None, 0));
    assert_eq!(
        decide(true, e, KeyMode::Locked, 0),
        (true, None, 1 << KEYS_LIST.len())
    );
}

#[test]
fn x_toggles_xray_only_while_the_keys_are_live() {
    let x = b'x' as i32;
    let bit = 1 << (KEYS_LIST.len() + 1);
    assert_eq!(
        decide(true, x, KeyMode::Live, 0),
        (true, Some(Action::Xray), bit)
    );
    assert_eq!(decide(true, x, KeyMode::Off, 0), (false, None, 0));
    assert_eq!(decide(true, x, KeyMode::Locked, 0), (true, None, bit));
    assert_eq!(xray_word(true), "X-ray on");
    assert_eq!(xray_word(false), "X-ray off");
}

#[test]
fn a_taken_press_stays_taken_until_its_release() {
    // Y answered the last highlight and the review ended, but Y is still
    // down: its autorepeat and its release must not reach `messagemode`.
    let (_, _, held) = decide(true, Y, KeyMode::Live, 0);
    assert_eq!(decide(true, Y, KeyMode::Off, held), (true, None, held));
    assert_eq!(decide(false, Y, KeyMode::Off, held), (true, None, 0));
    // The next press, with no review on, is the game's again.
    assert_eq!(decide(true, Y, KeyMode::Off, 0), (false, None, 0));
}

#[test]
fn a_press_during_the_lockout_is_taken_but_does_nothing() {
    // The second press of a double Y: neither an answer nor chat.
    let (swallow, action, held) = decide(true, Y, KeyMode::Locked, 0);
    assert!(swallow);
    assert_eq!(action, None);
    // Still down when the lockout ends: its autorepeat doesn't answer,
    // its release is taken, and only a fresh press answers.
    assert_eq!(decide(true, Y, KeyMode::Live, held), (true, None, held));
    assert_eq!(decide(false, Y, KeyMode::Live, held), (true, None, 0));
    assert_eq!(
        decide(true, Y, KeyMode::Live, 0),
        (true, Some(Action::Yes), 1)
    );
    // Any other key is the game's during the lockout too.
    assert_eq!(
        decide(true, b'w' as i32, KeyMode::Locked, 0),
        (false, None, 0)
    );
}

#[test]
fn up_and_down_change_the_speed_only_while_the_keys_are_live() {
    let bit = 1 << (KEYS_LIST.len() + 2);
    assert_eq!(
        decide(true, KEY_UP_ARROW, KeyMode::Live, 0),
        (true, Some(Action::Faster), bit)
    );
    assert_eq!(
        decide(true, KEY_DOWN_ARROW, KeyMode::Live, 0),
        (true, Some(Action::Slower), bit << 1)
    );
    // The arrows are +forward and +back with no review highlight on.
    assert_eq!(
        decide(true, KEY_UP_ARROW, KeyMode::Off, 0),
        (false, None, 0)
    );
    assert_eq!(
        decide(true, KEY_DOWN_ARROW, KeyMode::Locked, 0),
        (true, None, bit << 1)
    );
    assert_eq!(virtual_key(KEY_UP_ARROW), Some(0x26));
    assert_eq!(virtual_key(KEY_DOWN_ARROW), Some(0x28));
}

#[test]
fn a_key_mode_survives_its_atomic() {
    for mode in [KeyMode::Off, KeyMode::Locked, KeyMode::Live] {
        assert_eq!(KeyMode::from_code(mode.code()), mode);
    }
}

#[test]
fn the_lockout_greys_every_key() {
    let panel = Panel {
        keys: true,
        live: false,
        can_go_back: true,
        playing: true,
        ..Panel::default()
    };
    for k in all_keys() {
        assert!(!panel.key_live(k.action));
    }
}

#[test]
fn a_key_let_go_elsewhere_is_forgotten() {
    // #1320: Space moved the review to another demo, whose load brought
    // the menu up before Space was let go: its release never came.
    let space = b' ' as i32;
    let (_, _, held) = decide(true, space, KeyMode::Live, 0);
    // While it is still down, the bit stays: autorepeat stays taken.
    assert_eq!(released(held, 0, |k| k == space), (0, 0));
    // Up at one poll: kept for a frame, for a release on its way.
    let (drop, up) = released(held, 0, |_| false);
    assert_eq!((drop, up), (0, held));
    // Up at the next poll too: dropped.
    let (drop, up) = released(held, up, |_| false);
    assert_eq!((drop, up), (held, 0));
    let held = held & !drop;
    // So the first press on the next highlight answers.
    assert_eq!(
        decide(true, space, KeyMode::Live, held),
        (true, Some(Action::Next), 1 << 5)
    );
    // Each bit follows its own key.
    let both = (1 << 0) | (1 << 5);
    assert_eq!(up_now(both, |k| k == Y), 1 << 5);
    assert_eq!(released(both, both, |k| k == Y), (1 << 5, 0));
}

#[test]
fn a_release_in_time_clears_its_bit_as_usual() {
    // An ordinary tap: up at one poll, its release arrives before the
    // next, which then has nothing to drop.
    let (_, _, held) = decide(true, Y, KeyMode::Live, 0);
    let (drop, up) = released(held, 0, |_| false);
    assert_eq!(drop, 0);
    let (_, _, held) = decide(false, Y, KeyMode::Live, held);
    assert_eq!(held, 0);
    assert_eq!(released(held, up, |_| false), (0, 0));
    // Down again by the next poll (a new press): nothing is dropped.
    assert_eq!(released(1, 1, |k| k == Y), (0, 0));
}

#[test]
fn a_key_with_no_virtual_key_counts_as_down() {
    assert!(key_is_down(27));
}

#[test]
fn every_key_has_a_virtual_key() {
    for k in all_keys() {
        let vk = virtual_key(k.code).unwrap_or_else(|| panic!("{}", k.label));
        // Windows names a letter by its upper-case ASCII code.
        if k.label.len() == 1 {
            assert_eq!(vk, k.label.as_bytes()[0] as i32);
        }
    }
    assert_eq!(virtual_key(b' ' as i32), Some(0x20));
    assert_eq!(virtual_key(27), None);
    assert_eq!(virtual_key(-1), None);
}

#[test]
fn a_release_alone_is_never_taken() {
    // Pressed before the review took the keys, released after.
    assert_eq!(decide(false, Y, KeyMode::Live, 0), (false, None, 0));
}

#[test]
fn each_key_has_its_own_action() {
    let keys: Vec<_> = b"ymnrb s"
        .iter()
        .map(|&k| action_for(k as i32).map(|(_, a)| a))
        .collect();
    assert_eq!(
        keys,
        vec![
            Some(Action::Yes),
            Some(Action::Maybe),
            Some(Action::No),
            Some(Action::Replay),
            Some(Action::Back),
            Some(Action::Next),
            Some(Action::Stop)
        ]
    );
    // Upper case never comes from the engine; Shift is its own key.
    assert_eq!(action_for(b'Y' as i32), None);
    for k in &KEYS_LIST {
        assert_eq!(Action::from_code(k.action.code()), Some(k.action));
    }
    assert_eq!(Action::from_code(0), None);
}

#[test]
fn next_and_stop_are_taken_only_during_a_review_highlight() {
    let space = b' ' as i32;
    let s = b's' as i32;
    // Space is +jump and S +moveback with no review highlight on.
    assert_eq!(decide(true, space, KeyMode::Off, 0), (false, None, 0));
    assert_eq!(decide(true, s, KeyMode::Off, 0), (false, None, 0));
    assert_eq!(
        decide(true, space, KeyMode::Live, 0),
        (true, Some(Action::Next), 1 << 5)
    );
    assert_eq!(
        decide(true, s, KeyMode::Live, 0),
        (true, Some(Action::Stop), 1 << 6)
    );
    // Next held past the move on: its release is taken, not +jump's.
    assert_eq!(decide(false, space, KeyMode::Off, 1 << 5), (true, None, 0));
}

#[test]
fn every_key_and_label_is_its_own() {
    let keys: Vec<&Key> = all_keys().collect();
    for (i, a) in keys.iter().enumerate() {
        for b in &keys[i + 1..] {
            assert_ne!(a.code, b.code, "{} and {}", a.label, b.label);
            assert_ne!(a.label, b.label);
            assert_ne!(a.action, b.action);
        }
        // A letter's name on the box is the letter itself.
        if a.label.len() == 1 {
            assert_eq!(a.label.to_ascii_lowercase().as_bytes()[0] as i32, a.code);
        }
    }
    // The held set has a bit for each key.
    assert!(keys.len() <= 32);
    assert_eq!(Action::from_code(Action::Form.code()), Some(Action::Form));
}

#[test]
fn the_keys_follow_the_review_tabs_buttons() {
    // Review.res lists its buttons in order; the box lists the keys in
    // the same order, each under the button's own word.
    let res = include_str!("../ui/Review.res");
    let mut at = 0;
    for k in &KEYS_LIST {
        let label = format!("\"labelText\"\t\t\"{}\"", k.word);
        let found = res[at..]
            .find(&label)
            .unwrap_or_else(|| panic!("no {} button after the previous key's", k.word));
        at += found + label.len();
    }
    // And no button is left without a key, but the two that start a
    // review from the game (#1142), which have none.
    assert_eq!(
        res.matches("\"Button\"").count(),
        KEYS_LIST.len() + crate::review::START_BUTTONS.len()
    );
}
