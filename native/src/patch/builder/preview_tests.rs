//! `build_preview_patch_jobs`: a preview demo's Event List numbers its highlights
//! the way the capture job does (#1417, #1449).

use super::tests::streak_with_kills;
use super::*;

fn tagged(tag: &str, player: &str, start_tick: i32, end_tick: i32, kills: &[i32]) -> CaptureStreak {
    let mut s = streak_with_kills(start_tick, end_tick, kills);
    s.target_player = Some(player.to_string());
    s.timeline_string = tag.to_string();
    s
}

/// The highlight labels (`#i/n: ...`) of the job's director events, by tag.
fn labels(streaks: Vec<CaptureStreak>) -> std::collections::BTreeMap<String, String> {
    let jobs = build_preview_patch_jobs(streaks, None);
    assert_eq!(jobs.len(), 1);
    jobs[0]
        .director_events
        .iter()
        .filter(|(_, l)| l.starts_with('#'))
        .map(|(_, l)| (l.rsplit(' ').next().unwrap().to_string(), l.clone()))
        .collect()
}

#[test]
fn highlights_sharing_a_start_tick_are_numbered_the_same_whatever_order_they_were_sent() {
    let long = || tagged("long", "A", 1000, 1400, &[1000, 1200, 1400]);
    let short = || tagged("short", "A", 1000, 1150, &[1000, 1150]);

    let a = labels(vec![long(), short()]);
    let b = labels(vec![short(), long()]);

    assert_eq!(a, b);
    assert!(a["short"].starts_with("#1/2"), "{a:?}");
    assert!(a["long"].starts_with("#2/2"), "{a:?}");
}

#[test]
fn numbers_run_per_player_like_the_capture_jobs() {
    let got = labels(vec![
        tagged("a1", "A", 1000, 1100, &[1000, 1100]),
        tagged("b1", "B", 1500, 1600, &[1500, 1600]),
        tagged("a2", "A", 2000, 2100, &[2000, 2100]),
        tagged("b2", "B", 2500, 2600, &[2500, 2600]),
    ]);

    assert!(got["a1"].starts_with("#1/2"), "{got:?}");
    assert!(got["a2"].starts_with("#2/2"), "{got:?}");
    assert!(got["b1"].starts_with("#1/2"), "{got:?}");
    assert!(got["b2"].starts_with("#2/2"), "{got:?}");
}

#[test]
fn two_kills_on_one_frame_are_ordered_by_kill_range_not_sent_order() {
    let mut first = tagged("first", "A", 1000, 1000, &[1000, 1000]);
    first.start_index = 0;
    first.end_index = 0;
    let mut second = first.clone();
    second.timeline_string = "second".to_string();
    second.start_index = 1;
    second.end_index = 1;

    let sent_first_first = labels(vec![first.clone(), second.clone()]);
    let sent_second_first = labels(vec![second, first]);

    assert_eq!(sent_first_first, sent_second_first);
    assert!(sent_first_first["first"].starts_with("#1/2"));
    assert!(sent_first_first["second"].starts_with("#2/2"));
}

#[test]
fn a_demo_with_one_player_keeps_the_plain_label() {
    let got = labels(vec![tagged("t", "A", 1000, 1100, &[1000, 1100])]);

    assert_eq!(got["t"], "#1/1: 2 kills: t");
}

#[test]
fn a_demo_with_several_players_names_each_one_on_its_labels() {
    let got = labels(vec![
        tagged("a1", "Alpha", 1000, 1100, &[1000, 1100]),
        tagged("b1", "Bravo", 1500, 1600, &[1500, 1600]),
    ]);

    assert_eq!(got["a1"], "#1/1 Alpha: 2 kills: a1");
    assert_eq!(got["b1"], "#1/1 Bravo: 2 kills: b1");
}

#[test]
fn a_long_player_name_is_cut_so_the_kill_list_survives() {
    let long = "N".repeat(100);
    let got = labels(vec![
        tagged("a1", &long, 1000, 1100, &[1000, 1100]),
        tagged("b1", "B", 1500, 1600, &[1500, 1600]),
    ]);

    assert_eq!(got["a1"], format!("#1/1 {}: 2 kills: a1", "N".repeat(24)));
}
