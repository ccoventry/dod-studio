//! `build_preview_patch_jobs`: a preview demo's Event List numbers its highlights
//! the way the capture job does (#1417, #1449).

use super::blocks::group_highlights;
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

    let order = |h: Vec<CaptureStreak>| -> Vec<String> {
        group_highlights(&h)[0]
            .highlights
            .iter()
            .map(|(_, s)| s.timeline_string.clone())
            .collect()
    };

    assert_eq!(
        order(vec![first.clone(), second.clone()]),
        order(vec![second, first])
    );
}
