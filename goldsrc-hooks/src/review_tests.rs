use super::*;
use crate::review_keys::clock_text;

fn queue_text(lines: &[&str]) -> String {
    std::iter::once(QUEUE_HEADER)
        .chain(lines.iter().copied())
        .collect::<Vec<_>>()
        .join("\n")
}

fn highlight(demo: &str, verdict: Option<Verdict>) -> Highlight {
    Highlight {
        demo: demo.to_string(),
        key: "0".to_string(),
        player: String::new(),
        kills: vec![10.0],
        from: 1,
        to: 1,
        verdict,
        note: String::new(),
        spectate: None,
        details: Vec::new(),
    }
}

fn kill_detail(victim: &str, weapon: &str) -> KillDetail {
    KillDetail {
        victim: victim.to_string(),
        weapon: weapon.to_string(),
    }
}

#[test]
fn a_queue_line_carries_each_kills_victim_and_weapon() {
    let line = format!(
        "p.dem\t1\t\t5,9\t1\t2\t-\t\t-\tgskiLL{PART_SEP}K98{KILL_SEP}{{$T}} Brain{PART_SEP}Colt"
    );
    let (queue, _) = parse_queue(&queue_text(&[&line])).unwrap();
    assert_eq!(
        queue[0].details,
        vec![
            kill_detail("gskiLL", "K98"),
            kill_detail("{$T} Brain", "Colt")
        ]
    );
    assert_eq!(queue[0].spectate, None);
}

#[test]
fn details_that_are_not_one_per_kill_are_dropped() {
    assert_eq!(parse_details(Some("a"), 2), vec![]);
    assert_eq!(parse_details(None, 1), vec![]);
    assert_eq!(parse_details(Some(" "), 1), vec![]);
    // A kill with no weapon part keeps its victim.
    assert_eq!(parse_details(Some("a"), 1), vec![kill_detail("a", "")]);
}

#[test]
fn a_kill_time_reads_as_the_vcr_bar_shows_it() {
    assert_eq!(clock_text(356.7), "5:56");
    assert_eq!(clock_text(5.0), "0:05");
    assert_eq!(clock_text(-1.0), "0:00");
}

#[test]
fn the_kill_list_stands_out_the_next_kill() {
    use crate::review_keys::KillState::{Later, Next, Past};
    let mut h = highlight("a", None);
    h.kills = vec![356.0, 363.0, 370.0];
    h.details = vec![
        kill_detail("gskiLL", "K98"),
        kill_detail("Brain", ""),
        kill_detail("", ""),
    ];
    let states = |clock| {
        kill_lines(&h, clock)
            .into_iter()
            .map(|l| l.state)
            .collect::<Vec<_>>()
    };
    assert_eq!(states(KillClock::At(352.0)), vec![Next, Later, Later]);
    // A kill counts as come once the clock reaches it.
    assert_eq!(states(KillClock::At(356.0)), vec![Past, Next, Later]);
    assert_eq!(states(KillClock::At(371.0)), vec![Past, Past, Past]);
    assert_eq!(states(KillClock::Ended), vec![Past, Past, Past]);
    assert_eq!(states(KillClock::Unknown), vec![Later, Later, Later]);
    let texts: Vec<String> = kill_lines(&h, KillClock::Ended)
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert_eq!(
        texts,
        vec!["1  5:56  gskiLL  K98", "2  6:03  Brain", "3  6:10"]
    );
}

#[test]
fn the_box_lists_the_kills_while_playing_and_at_the_end() {
    use crate::review_keys::KillState::{Next, Past};
    let mut review = review_of(vec![highlight("a", None)], playing());
    let now = review.keys_from;
    let kills = |r: &Review, clock| panel_view(r, now, clock, 1.0).0.expect("a panel").kills;
    assert_eq!(kills(&review, Some(8.0))[0].state, Next);
    review.phase = Phase::Waiting;
    assert_eq!(kills(&review, Some(8.0))[0].state, Past);
    review.phase = Phase::Loading {
        since: Instant::now(),
        frames: 0,
        seen_loading: false,
    };
    assert!(kills(&review, None).is_empty());
}

/// The box's header, one string per line.
fn header_text(review: &Review, clock: Option<f64>, speed: f32) -> Vec<String> {
    let now = review.keys_from;
    panel_view(review, now, clock, speed)
        .0
        .expect("a panel")
        .header
        .iter()
        .map(|l| l.text())
        .collect()
}

#[test]
fn the_box_shows_the_clock_beside_the_kills() {
    let mut review = review_of(vec![highlight("a", None)], playing());
    let last = |r: &Review, clock, speed| header_text(r, clock, speed).pop();
    assert_eq!(last(&review, Some(341.6), 1.0).as_deref(), Some("Now 5:41"));
    assert_eq!(last(&review, None, 1.0).as_deref(), Some("Kill 1"));
    review.phase = Phase::Waiting;
    assert_eq!(last(&review, Some(370.0), 1.0).as_deref(), Some("Now 6:10"));
}

#[test]
fn the_box_shows_the_speed_when_it_isnt_normal() {
    let review = review_of(vec![highlight("a", None)], playing());
    assert_eq!(
        header_text(&review, Some(341.6), 2.0).pop().as_deref(),
        Some("Now 5:41 | Speed 2x")
    );
}

#[test]
fn the_box_shows_the_header_in_every_phase() {
    let mut h = highlight("a", Some(Verdict::Yes));
    h.player = "m00cat".to_string();
    h.kills = vec![10.0, 12.0, 14.0, 16.0];
    (h.from, h.to) = (2, 3);
    h.note = "nice flick".to_string();
    let mut review = review_of(vec![h, highlight("a", None)], playing());
    let expected = vec![
        "Highlight 1 of 2 | 4 kills | answered yes",
        "Player m00cat",
        "Kills 2-3 (was 1-4)",
        "Note: nice flick",
    ];
    let with_clock = |r: &Review| header_text(r, Some(11.0), 1.0);
    let mut playing_header = expected.clone();
    playing_header.push("Now 0:11");
    assert_eq!(with_clock(&review), playing_header);
    review.phase = Phase::Waiting;
    assert_eq!(with_clock(&review), playing_header);
    // Loading: no clock yet.
    review.phase = Phase::Loading {
        since: Instant::now(),
        frames: 0,
        seen_loading: false,
    };
    assert_eq!(with_clock(&review), expected);
}

#[test]
fn a_long_note_takes_more_lines_in_the_header() {
    let mut h = highlight("a", None);
    h.note = "word ".repeat(30);
    let review = review_of(vec![h], playing());
    let header = header_text(&review, None, 1.0);
    // Title, range and three note lines; no player in the queue.
    assert_eq!(header.len(), 5);
    assert!(header[2].starts_with("Note: word"));
}

#[test]
fn the_speed_keys_work_only_while_a_highlight_plays() {
    use crate::review_keys::Action;
    let mut review = review_of(vec![highlight("a", None)], playing());
    let now = review.keys_from;
    let panel = |r: &Review| panel_view(r, now, None, 1.0).0.expect("a panel");
    assert!(panel(&review).key_live(Action::Faster));
    assert!(panel(&review).key_live(Action::Slower));
    // Paused at the end: nothing to speed up.
    review.phase = Phase::Waiting;
    assert!(!panel(&review).key_live(Action::Faster));
    assert!(panel(&review).key_live(Action::Yes));
}

#[test]
fn a_chosen_speed_counts_as_sped_up() {
    let mut review = review_of(vec![highlight("a", None)], playing());
    assert!(!sped(&review));
    review.speed = Some(0.5);
    assert!(sped(&review));
    review.speed = None;
    review.phase = Phase::Playing {
        until: 20.0,
        last: 6.0,
        beat: Instant::now(),
        spans: Vec::new(),
        fast: true,
    };
    assert!(sped(&review));
}

#[test]
fn an_hltv_highlight_names_the_player_the_camera_goes_on() {
    let (queue, _) = parse_queue(&queue_text(&[
        "h.dem\t1\tgskiLL\t5\t1\t1\t-\t\t7",
        "h.dem\t2\tp\t9\t1\t1\t-\tnote\t-",
        "p.dem\t3\tp\t9\t1\t1\t-\tnote",
        "h.dem\t4\tp\t9\t1\t1\t-\t\t40",
    ]))
    .unwrap();
    assert_eq!(queue[0].spectate, Some(7));
    assert_eq!(
        queue[0].camera_line().as_deref(),
        Some("spec_mode 4; ds_spec_target 7")
    );
    // A POV demo's line has no number, or `-`; a number out of range is none.
    assert_eq!(queue[1].spectate, None);
    assert_eq!(queue[2].spectate, None);
    assert_eq!(queue[3].spectate, None);
    assert_eq!(queue[2].camera_line(), None);
}

#[test]
fn a_queue_reads_every_field() {
    let text = queue_text(&[
        "C:/demos/a.dem\t3\tm00cat\t100.5,104,110.25\t2\t3\t-\tnice flick",
        "C:/demos/a.dem\t7\tm00cat\t200\t1\t1\tyes\t",
    ]);
    let (queue, options) = parse_queue(&text).unwrap();
    assert_eq!(options, Options::default());
    assert_eq!(queue.len(), 2);
    assert_eq!(queue[0].demo, "C:/demos/a.dem");
    assert_eq!(queue[0].key, "3");
    assert_eq!(queue[0].player, "m00cat");
    assert_eq!(queue[0].kills, vec![100.5, 104.0, 110.25]);
    assert_eq!((queue[0].from, queue[0].to), (2, 3));
    assert_eq!(queue[0].verdict, None);
    assert_eq!(queue[0].note, "nice flick");
    assert_eq!(queue[1].verdict, Some(Verdict::Yes));
    assert_eq!(queue[1].note, "");
}

#[test]
fn a_trailing_empty_note_may_be_missing() {
    let (queue, _) = parse_queue(&queue_text(&["d.dem\t1\tp\t5\t1\t1\tno"])).unwrap();
    assert_eq!(queue[0].note, "");
    assert_eq!(queue[0].verdict, Some(Verdict::No));
}

#[test]
fn a_range_outside_the_kills_is_pulled_in() {
    let (queue, _) = parse_queue(&queue_text(&["d.dem\t1\tp\t5,6\t0\t9\t-\t"])).unwrap();
    assert_eq!((queue[0].from, queue[0].to), (1, 2));
}

#[test]
fn the_header_carries_the_fast_forward_options() {
    let text = format!("{QUEUE_HEADER}\tgap=8\tspeed=6\tnew=1\nd.dem\t1\tp\t5\t1\t1\t-\t");
    let (_, options) = parse_queue(&text).unwrap();
    assert_eq!(
        options,
        Options {
            gap: 8.0,
            speed: 6.0,
            from: None,
        }
    );
}

#[test]
fn from_here_on_the_header_starts_at_the_first_highlight_after_it() {
    let text = format!("{QUEUE_HEADER}\tfrom=400.5\nd.dem\t1\tp\t5\t1\t1\t-\t");
    let (_, options) = parse_queue(&text).unwrap();
    assert_eq!(options.from, Some(400.5));
    assert_eq!(parse_options("from=-1\tfrom=x".split('\t')).from, None);
    let mut queue = vec![
        highlight("a", None),
        highlight("a", None),
        highlight("a", None),
    ];
    queue[0].kills = vec![356.0, 363.0];
    queue[1].kills = vec![402.9];
    queue[2].kills = vec![428.0];
    assert_eq!(start_index(&queue, None), Ok(0));
    // In the middle of the first highlight: the next one.
    assert_eq!(start_index(&queue, Some(360.0)), Ok(1));
    // At a first kill exactly: that one.
    assert_eq!(start_index(&queue, Some(428.0)), Ok(2));
    // Past every highlight: the first, said so.
    assert_eq!(start_index(&queue, Some(500.0)), Err(0));
}

#[test]
fn the_start_buttons_need_studio_a_demo_and_no_request_on_its_way() {
    use crate::review_ask::Status;
    let facts = |studio, demo, ask| IdleFacts { studio, demo, ask };
    assert!(facts(true, true, Status::Idle).can_ask());
    assert!(facts(true, true, Status::Failed("x".into())).can_ask());
    assert!(facts(true, true, Status::NotAsked("x".into())).can_ask());
    assert!(!facts(false, true, Status::Idle).can_ask());
    assert!(!facts(true, false, Status::Idle).can_ask());
    assert!(!facts(true, true, Status::Asking("Scanning".into())).can_ask());
    assert!(!facts(true, true, Status::Sent(std::time::Instant::now())).can_ask());
}

#[test]
fn the_idle_tab_says_why_it_cant_start_or_how_the_request_goes() {
    use crate::review_ask::Status;
    let facts = |studio, demo, ask| IdleFacts { studio, demo, ask };
    assert!(
        facts(false, true, Status::Idle)
            .detail()
            .starts_with("Start DoD Studio")
    );
    assert!(
        facts(true, false, Status::Idle)
            .detail()
            .starts_with("Play a demo")
    );
    assert!(
        facts(true, true, Status::Idle)
            .detail()
            .contains("From here")
    );
    assert_eq!(
        facts(true, true, Status::Asking("Scanning the demo...".into())).detail(),
        "Scanning the demo..."
    );
    assert_eq!(
        facts(true, true, Status::Failed("no highlights".into())).detail(),
        "DoD Studio couldn't start it: no highlights"
    );
    // A wait that gave up in the game doesn't blame Studio.
    let gave_up = "the demo did not finish loading in three minutes";
    let line = facts(true, true, Status::NotAsked(gave_up.into())).detail();
    assert_eq!(line, format!("Not sent to DoD Studio: {gave_up}"));
    assert!(line.len() <= 88, "{line}");
    // A failure from before Studio closed gives way to why nothing works.
    assert!(
        facts(false, true, Status::Failed("x".into()))
            .detail()
            .starts_with("Start DoD Studio")
    );
    // Each fits the default window's line; a longer one was cut off.
    for studio in [false, true] {
        for demo in [false, true] {
            let line = facts(studio, demo, Status::Idle).detail();
            assert!(line.len() <= 88, "{line}");
        }
    }
}

#[test]
fn the_start_buttons_are_in_the_review_tab_and_ask_studio() {
    let res = include_str!("../ui/Review.res");
    for (name, what) in START_BUTTONS.iter().zip(["start", "here"]) {
        let at = res
            .find(&format!("\"fieldName\"\t\t\"{name}\""))
            .expect(name);
        let block = &res[at..at + res[at..].find('}').unwrap()];
        assert!(
            block.contains(&format!("\"engine {NAME} ask {what}\"")),
            "{name}"
        );
    }
}

#[test]
fn bad_fast_forward_options_fall_back_or_clamp() {
    let parse = |fields: &str| parse_options(fields.split('\t'));
    assert_eq!(parse("gap=x\tspeed=NaN"), Options::default());
    assert_eq!(
        parse("gap=-3\tspeed=99"),
        Options {
            gap: 0.0,
            speed: 16.0,
            from: None,
        }
    );
    assert_eq!(parse("gap=inf"), Options::default());
}

#[test]
fn a_bad_queue_says_why() {
    assert!(
        parse_queue("hello")
            .unwrap_err()
            .contains("not a review queue")
    );
    assert!(
        parse_queue(QUEUE_HEADER)
            .unwrap_err()
            .contains("no highlights")
    );
    assert!(
        parse_queue(&queue_text(&["d.dem\t1\tp\tx\t1\t1\t-\t"]))
            .unwrap_err()
            .contains("kill times")
    );
    assert!(
        parse_queue(&queue_text(&["d.dem\t1"]))
            .unwrap_err()
            .contains("fields")
    );
}

#[test]
fn maybe_is_read_and_sent_like_any_answer() {
    let (queue, _) = parse_queue(&queue_text(&[
        "d.dem\t1\tp\t5\t1\t1\tmaybe\t",
        "d.dem\t2\tp\t9\t1\t1\t-\t",
    ]))
    .unwrap();
    assert_eq!(queue[0].verdict, Some(Verdict::Maybe));
    assert_eq!(queue[1].verdict, None);
    assert_eq!(
        answer_label(&queue[0], Verdict::Maybe),
        "REVIEW\td.dem\t1\tmaybe\t1\t1\t"
    );
}

#[test]
fn playback_runs_from_before_the_first_kill_to_after_the_last() {
    let mut h = highlight("a", None);
    h.kills = vec![100.0, 112.0];
    assert_eq!(h.window(), (100.0 - LEAD, 112.0 + TAIL));
    h.kills = vec![1.0];
    assert_eq!(h.window(), (0.0, 1.0 + TAIL));
}

#[test]
fn an_answer_is_one_tab_separated_line() {
    let mut h = highlight("C:/d/a.dem", None);
    h.key = "4".to_string();
    h.kills = vec![1.0, 2.0, 3.0];
    (h.from, h.to) = (2, 3);
    h.note = "two\tlines\nhere ".to_string();
    assert_eq!(
        answer_label(&h, Verdict::Yes),
        "REVIEW\tC:/d/a.dem\t4\tyes\t2\t3\ttwo lines here"
    );
}

#[test]
fn a_note_is_cut_to_its_limit() {
    assert_eq!(clean_note(&"x".repeat(NOTE_MAX + 50)).len(), NOTE_MAX);
}

#[test]
fn a_typed_kill_must_be_one_of_the_kills() {
    assert_eq!(typed_kill(" 2 ", 3), Some(2));
    assert_eq!(typed_kill("0", 3), None);
    assert_eq!(typed_kill("4", 3), None);
    assert_eq!(typed_kill("two", 3), None);
}

#[test]
fn viewdemo_reaches_a_demo_from_the_game_folder() {
    let dod = r"C:\Steam\common\Half-Life\dod";
    assert_eq!(
        viewdemo_line(dod, r"C:\Steam\common\Half-Life\dod\demos\a b.dem").unwrap(),
        "viewdemo \"demos/a b.dem\"\n"
    );
    assert_eq!(
        viewdemo_line(dod, r"c:/steam/common/Half-Life/dod/x.dem").unwrap(),
        "viewdemo \"x.dem\"\n"
    );
    assert_eq!(
        viewdemo_line(dod, r"C:\Users\me\Downloads\m.dem").unwrap(),
        "viewdemo \"../../../../Users/me/Downloads/m.dem\"\n"
    );
}

#[test]
fn a_demo_on_another_drive_goes_by_its_full_path_when_the_hook_opens_it() {
    let dod = r"C:\Steam\common\Half-Life\dod";
    assert_eq!(
        viewdemo_line_with(dod, r"F:\WSOD 25\demos\m.dem", Ok(())).unwrap(),
        "viewdemo \"F:/WSOD 25/demos/m.dem\"\n"
    );
    assert!(
        viewdemo_line_with(dod, r"F:\Grüße\m.dem", Ok(()))
            .unwrap_err()
            .contains("can't open")
    );
    // A demo at the drive's root.
    assert_eq!(
        viewdemo_line_with(dod, "d:/m.dem", Ok(())).unwrap(),
        "viewdemo \"d:/m.dem\"\n"
    );
    let off = viewdemo_line_with(dod, r"F:\Downloads\m.dem", Err("it is off".into()));
    assert!(
        off.unwrap_err()
            .contains("another drive than the game, and it is off")
    );
    // No drive letter to open it by: a network path.
    assert!(
        viewdemo_line_with(dod, r"\\server\share\m.dem", Ok(()))
            .unwrap_err()
            .contains("another drive")
    );
    let long = format!(r"F:\{}\m.dem", "d".repeat(240));
    assert!(
        viewdemo_line_with(dod, &long, Ok(()))
            .unwrap_err()
            .contains("too long")
    );
}

#[test]
fn the_heading_counts_demos_and_what_is_left() {
    let queue = vec![
        highlight("a", Some(Verdict::Yes)),
        highlight("a", None),
        highlight("b", None),
    ];
    assert_eq!(
        heading(&queue, 2),
        "Demo 2 of 2 | highlight 3 of 3 | 2 left to answer"
    );
    assert_eq!(
        heading(&queue, 0),
        "Demo 1 of 2 | highlight 1 of 3 | 2 left to answer"
    );
}

#[test]
fn the_panel_title_names_the_highlight_the_player_and_the_kills() {
    let mut queue = vec![highlight("a", None), highlight("a", None)];
    queue[1].player = "gorilla".to_string();
    queue[1].kills = vec![1.0, 2.0, 3.0, 4.0];
    assert_eq!(panel_title(&queue, 0), "Highlight 1 of 2 | 1 kill");
    assert_eq!(
        panel_title(&queue, 1),
        "Highlight 2 of 2 | gorilla | 4 kills"
    );
    queue[1].verdict = Some(Verdict::Maybe);
    assert_eq!(
        panel_title(&queue, 1),
        "Highlight 2 of 2 | gorilla | 4 kills | answered maybe"
    );
}

fn review_of(queue: Vec<Highlight>, phase: Phase) -> Review {
    Review {
        queue,
        options: Options::default(),
        at: 0,
        phase,
        loaded: Some("a".to_string()),
        keys_from: Instant::now(),
        hushed: true,
        speed: None,
    }
}

fn playing() -> Phase {
    Phase::Playing {
        until: 20.0,
        last: 6.0,
        beat: Instant::now(),
        spans: Vec::new(),
        fast: false,
    }
}

#[test]
fn the_keys_answer_while_a_highlight_plays_and_at_its_end() {
    // The settings' defaults (both on): no cvar in a host build.
    let mut review = review_of(vec![highlight("a", None)], Phase::Waiting);
    let now = review.keys_from;
    let (panel, mode) = panel_view(&review, now, None, 1.0);
    assert_eq!(mode, KeyMode::Live);
    assert!(panel.is_some_and(|p| p.keys && p.live));
    review.phase = playing();
    let (panel, mode) = panel_view(&review, now, None, 1.0);
    assert_eq!(mode, KeyMode::Live);
    assert!(panel.is_some_and(|p| p.keys && p.live && p.status.is_empty()));
}

#[test]
fn the_keys_do_nothing_while_the_form_shows() {
    // Typing a Y in the note must not answer, should it reach the game.
    assert_eq!(under_form(KeyMode::Live, true), KeyMode::Locked);
    assert_eq!(under_form(KeyMode::Live, false), KeyMode::Live);
    // Off stays off: the form doesn't take keys the review doesn't.
    assert_eq!(under_form(KeyMode::Off, true), KeyMode::Off);
}

#[test]
fn a_form_save_is_one_tab_separated_line_without_a_verdict() {
    let mut h = highlight("C:/d/a.dem", Some(Verdict::Yes));
    h.key = "4".to_string();
    h.kills = vec![1.0, 2.0, 3.0];
    (h.from, h.to) = (2, 3);
    h.note = "nice\tflick ".to_string();
    assert_eq!(
        edit_label(&h),
        "REVIEW_EDIT\tC:/d/a.dem\t4\t2\t3\tnice flick"
    );
}

#[test]
fn the_keys_are_locked_while_loading_and_off_on_a_failed_highlight() {
    let mut review = review_of(
        vec![highlight("a", None)],
        Phase::Loading {
            since: Instant::now(),
            frames: 0,
            seen_loading: false,
        },
    );
    let now = review.keys_from;
    let (panel, mode) = panel_view(&review, now, None, 1.0);
    assert_eq!(mode, KeyMode::Locked);
    let panel = panel.expect("a panel");
    assert_eq!(panel.status, "Loading the demo...");
    assert!(panel.keys && !panel.live);
    // A failed highlight has the Review tab open instead.
    review.phase = Phase::Failed("gone".into());
    assert_eq!(panel_view(&review, now, None, 1.0), (None, KeyMode::Off));
}

#[test]
fn the_keys_do_nothing_for_the_lockout_after_a_highlight_starts() {
    let mut review = review_of(vec![highlight("a", None)], playing());
    let started = Instant::now();
    review.keys_from = started + LOCKOUT;
    let (panel, mode) = panel_view(&review, started, None, 1.0);
    assert_eq!(mode, KeyMode::Locked);
    assert!(panel.is_some_and(|p| p.keys && !p.live));
    let half = started + LOCKOUT / 2;
    assert_eq!(key_mode(&review, true, half), KeyMode::Locked);
    assert_eq!(key_mode(&review, true, started + LOCKOUT), KeyMode::Live);
}

#[test]
fn with_the_keys_off_they_are_the_games_throughout() {
    let mut review = review_of(vec![highlight("a", None)], playing());
    let now = review.keys_from;
    assert_eq!(key_mode(&review, false, now), KeyMode::Off);
    review.phase = Phase::Waiting;
    assert_eq!(key_mode(&review, false, now), KeyMode::Off);
}

#[test]
fn back_is_greyed_on_the_first_highlight_only() {
    use crate::review_keys::Action;
    let mut review = review_of(
        vec![highlight("a", None), highlight("a", None)],
        Phase::Waiting,
    );
    let now = review.keys_from;
    let first = panel_view(&review, now, None, 1.0).0.expect("a panel");
    assert!(!first.key_live(Action::Back));
    assert!(first.key_live(Action::Next));
    review.at = 1;
    let second = panel_view(&review, now, None, 1.0).0.expect("a panel");
    assert!(second.key_live(Action::Back));
}

#[test]
fn only_a_menu_that_just_came_up_on_its_own_opens_the_review_tab() {
    // ESC: the menu up, nothing in it.
    assert!(menu_opens_tab(false, (true, false, false)));
    // Already up last frame, or brought up around our window or the
    // console window (the console key), or not up at all: left alone.
    assert!(!menu_opens_tab(true, (true, false, false)));
    assert!(!menu_opens_tab(false, (true, true, false)));
    assert!(!menu_opens_tab(false, (true, false, true)));
    assert!(!menu_opens_tab(false, (false, false, false)));
}

#[test]
fn the_detail_says_what_to_do() {
    let mut h = highlight("a", None);
    h.player = "m00cat".to_string();
    h.kills = vec![1.0, 2.0];
    assert_eq!(
        detail(&h, &Phase::Waiting),
        "m00cat, 2 kills -- Yes, Maybe or No?"
    );
    h.verdict = Some(Verdict::No);
    assert_eq!(
        detail(&h, &Phase::Failed("gone".into())),
        "m00cat, 2 kills (answered no) -- can't play this one: gone"
    );
}
