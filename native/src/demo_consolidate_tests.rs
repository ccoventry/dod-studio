use super::*;
use crate::test_support::Scratch;

fn demo(path: &str, size: u64) -> AuditedDemo {
    AuditedDemo {
        path: path.into(),
        size,
        ..AuditedDemo::default()
    }
}

fn game(path: &str) -> AuditedDemo {
    AuditedDemo {
        game_folder: true,
        ..demo(path, 1)
    }
}

fn request(keep: Vec<AuditedDemo>, recycle: Vec<AuditedDemo>) -> Request {
    Request {
        destination: r"F:\Demos".into(),
        keep,
        recycle,
        ..Request::default()
    }
}

fn targets(plan: &Plan) -> Vec<(&str, &str)> {
    plan.moves
        .iter()
        .map(|m| (m.from.as_str(), m.to.as_str()))
        .collect()
}

fn recycled(plan: &Plan) -> Vec<&str> {
    plan.recycle.iter().map(|r| r.path.as_str()).collect()
}

fn nothing_exists(_: &Path) -> bool {
    false
}

#[test]
fn two_different_demos_with_one_name_get_numbered() {
    let req = request(
        vec![
            demo(r"\\?\C:\a\match.dem", 10),
            demo(r"\\?\D:\b\match.dem", 20),
            demo(r"\\?\E:\c\MATCH.dem", 30),
        ],
        vec![],
    );
    let plan = plan(&req, &nothing_exists, u64::MAX).unwrap();
    assert_eq!(
        targets(&plan),
        [
            (r"\\?\C:\a\match.dem", r"F:\Demos\match.dem"),
            (r"\\?\D:\b\match.dem", r"F:\Demos\match_2.dem"),
            (r"\\?\E:\c\MATCH.dem", r"F:\Demos\MATCH_3.dem"),
        ]
    );
    assert_eq!(plan.renamed, 2);
    assert_eq!((plan.move_bytes, plan.copy_bytes), (60, 60));
}

#[test]
fn a_file_already_in_the_destination_is_never_replaced() {
    let req = request(vec![demo(r"C:\x\anzio.dem", 1)], vec![]);
    let taken = |p: &Path| p == Path::new(r"F:\Demos\anzio.dem");
    let plan = plan(&req, &taken, u64::MAX).unwrap();
    assert_eq!(
        targets(&plan),
        [(r"C:\x\anzio.dem", r"F:\Demos\anzio_2.dem")]
    );
}

#[test]
fn a_demo_already_in_the_destination_stays_and_keeps_its_name() {
    let req = request(
        vec![demo(r"F:\Demos\old\m.dem", 5), demo(r"C:\m.dem", 5)],
        vec![],
    );
    let plan = plan(
        &Request {
            layout: Layout::Subfolders,
            roots: vec![r"C:\".into(), r"F:\Demos".into()],
            ..req
        },
        &nothing_exists,
        u64::MAX,
    )
    .unwrap();
    assert_eq!(plan.already_there, 1);
    assert_eq!(targets(&plan), [(r"C:\m.dem", r"F:\Demos\m.dem")]);

    // Flat: the one already there claims its name first.
    let req = request(
        vec![demo(r"C:\anzio.dem", 1), demo(r"F:\Demos\anzio.dem", 1)],
        vec![],
    );
    let flat = super::plan(&req, &nothing_exists, u64::MAX).unwrap();
    assert_eq!(flat.already_there, 1);
    assert_eq!(targets(&flat), [(r"C:\anzio.dem", r"F:\Demos\anzio_2.dem")]);
}

#[test]
fn subfolders_are_kept_below_the_deepest_audited_folder() {
    let req = Request {
        layout: Layout::Subfolders,
        roots: vec![r"C:\".into(), r"C:\Users\x\Demos".into(), r"E:\".into()],
        ..request(
            vec![
                demo(r"\\?\C:\Users\x\Demos\wsod\day1\a.dem", 1),
                demo(r"\\?\C:\other\b.dem", 1),
                demo(r"\\?\E:\c.dem", 1),
                demo(r"\\?\G:\stray\d.dem", 1),
            ],
            vec![],
        )
    };
    let plan = plan(&req, &nothing_exists, u64::MAX).unwrap();
    assert_eq!(
        targets(&plan),
        [
            (r"\\?\C:\other\b.dem", r"F:\Demos\other\b.dem"),
            (
                r"\\?\C:\Users\x\Demos\wsod\day1\a.dem",
                r"F:\Demos\wsod\day1\a.dem"
            ),
            (r"\\?\E:\c.dem", r"F:\Demos\c.dem"),
            // Under no audited folder: straight in the destination.
            (r"\\?\G:\stray\d.dem", r"F:\Demos\d.dem"),
        ]
    );
}

#[test]
fn only_kept_copies_move_and_the_others_are_recycled() {
    let req = request(
        vec![demo(r"C:\keep.dem", 1)],
        vec![
            demo(r"D:\copy.dem", 1),
            // Listed as kept too: never recycled.
            demo(r"c:\KEEP.dem", 1),
            demo(r"D:\notes.txt", 1),
        ],
    );
    let plan = plan(&req, &nothing_exists, u64::MAX).unwrap();
    assert_eq!(targets(&plan), [(r"C:\keep.dem", r"F:\Demos\keep.dem")]);
    assert_eq!(recycled(&plan), [r"D:\copy.dem"]);
}

#[test]
fn game_folder_demos_stay_unless_asked_for() {
    let req = request(
        vec![game(r"C:\HL\dod\pov.dem"), demo(r"C:\x\b.dem", 1)],
        vec![game(r"C:\HL\dod\copy.dem")],
    );
    let plan_default = plan(&req, &nothing_exists, u64::MAX).unwrap();
    assert_eq!(targets(&plan_default), [(r"C:\x\b.dem", r"F:\Demos\b.dem")]);
    assert_eq!(plan_default.stay_in_game_folders, 1);
    assert_eq!(plan_default.copies_in_game_folders, 1);
    assert!(plan_default.recycle.is_empty());

    let ticked = Request {
        include_game_folders: true,
        ..req
    };
    let plan_ticked = super::plan(&ticked, &nothing_exists, u64::MAX).unwrap();
    assert_eq!(plan_ticked.moves.len(), 2);
    assert_eq!(recycled(&plan_ticked), [r"C:\HL\dod\copy.dem"]);
}

#[test]
fn only_moves_between_drives_need_space() {
    let req = request(
        vec![demo(r"F:\x\a.dem", 100), demo(r"C:\b.dem", 50)],
        vec![],
    );
    let plan = plan(&req, &nothing_exists, 60).unwrap();
    assert_eq!(plan.move_bytes, 150);
    assert_eq!(plan.copy_bytes, 50);
    assert!(plan.enough_space);
    assert!(
        !plan
            .moves
            .iter()
            .find(|m| m.from == r"F:\x\a.dem")
            .unwrap()
            .cross_drive
    );
    let tight = super::plan(&req, &nothing_exists, 49).unwrap();
    assert!(!tight.enough_space);
}

#[test]
fn no_destination_is_refused() {
    let req = Request {
        destination: "  ".into(),
        ..request(vec![], vec![])
    };
    assert!(plan(&req, &nothing_exists, 0).is_err());
}

#[test]
fn drives_compare_by_letter_or_share() {
    assert_eq!(drive_of(&comparable(r"\\?\C:\a\b.dem")), "c:");
    assert_eq!(drive_of(&comparable(r"\\?\UNC\nas\d\x.dem")), r"\\nas\d");
    assert_eq!(drive_of(&comparable(r"\\nas\d\y\z.dem")), r"\\nas\d");
}

#[test]
fn a_batch_moves_logs_recycles_and_undoes() {
    let dir = Scratch::new("consolidate_batch");
    let logs = dir.path().join("logs");
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("sub").join("a.dem"), b"aaaa").unwrap();
    std::fs::write(src.join("b.dem"), b"bb").unwrap();
    std::fs::write(src.join("copy.dem"), b"bb").unwrap();
    let dest = dir.path().join("all");
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    let req = Request {
        destination: path(dest.clone()),
        layout: Layout::Subfolders,
        roots: vec![path(src.clone())],
        keep: vec![
            demo(&path(src.join("sub").join("a.dem")), 4),
            demo(&path(src.join("b.dem")), 2),
        ],
        recycle: vec![],
        include_game_folders: false,
    };
    let mut reports = Vec::new();
    let outcome = run(&req, &logs, &AtomicBool::new(false), &mut |p| {
        reports.push(p.clone())
    })
    .unwrap();
    assert_eq!(outcome.moved.len(), 2, "{outcome:?}");
    assert!(outcome.failed.is_empty());
    assert_eq!(
        std::fs::read(dest.join("sub").join("a.dem")).unwrap(),
        b"aaaa"
    );
    assert!(!src.join("b.dem").exists());
    let last = reports.last().unwrap();
    assert_eq!((last.files_done, last.bytes_done), (2, 6));

    let batch = undoable(&logs).unwrap();
    assert_eq!(batch.count, 2);
    let undone = undo_last(&logs, &AtomicBool::new(false), &mut |_| {});
    assert_eq!(undone.moved.len(), 2);
    assert!(src.join("sub").join("a.dem").exists());
    assert!(src.join("b.dem").exists());
    assert_eq!(undoable(&logs), None);
    assert_eq!(
        undo_last(&logs, &AtomicBool::new(false), &mut |_| {}),
        Outcome::default()
    );
}

/// A batch whose kept copy moved into the destination: the copies are
/// compared with it where it is now.
fn moved_batch(dir: &Scratch) -> (Plan, Outcome, PathBuf, PathBuf, PathBuf) {
    use crate::demo_compare::tests::demo_bytes;
    let src = dir.path().join("src");
    let dest = dir.path().join("all");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let kept_was = src.join("m.dem");
    let kept_now = dest.join("m.dem");
    let same = src.join("same.dem");
    let other = src.join("other.dem");
    std::fs::write(&kept_now, demo_bytes(1)).unwrap();
    std::fs::write(&same, demo_bytes(1)).unwrap();
    std::fs::write(&other, demo_bytes(2)).unwrap();
    let text = |p: &Path| p.to_string_lossy().into_owned();
    let copy = |p: &Path| PlannedRecycle {
        path: text(p),
        keep: text(&kept_was),
    };
    let plan = Plan {
        recycle: vec![copy(&same), copy(&other)],
        ..Plan::default()
    };
    let outcome = Outcome {
        moved: vec![MovedPair {
            from: text(&kept_was),
            to: text(&kept_now),
        }],
        ..Outcome::default()
    };
    (plan, outcome, same, other, kept_now)
}

#[test]
fn only_a_byte_for_byte_copy_of_the_moved_kept_demo_is_recycled() {
    let dir = Scratch::new("consolidate_check");
    let (plan, mut outcome, same, other, _) = moved_batch(&dir);
    let mut steps = Vec::new();
    let recyclable = check_copies(&plan, &mut outcome, &AtomicBool::new(false), &mut |p| {
        steps.push((p.step, p.files_done, p.files_total))
    });
    assert_eq!(recyclable, [same.to_string_lossy().into_owned()]);
    assert_eq!(outcome.recycle_failed.len(), 1);
    let refused = &outcome.recycle_failed[0];
    assert_eq!(refused.path, other.to_string_lossy());
    assert!(refused.not_a_copy);
    assert!(
        refused.error.contains("not an exact copy"),
        "{}",
        refused.error
    );
    assert_eq!(steps.last(), Some(&(Step::Checking, 2, 2)));
}

#[test]
fn a_copy_whose_kept_demo_is_gone_or_is_itself_is_not_recycled() {
    let dir = Scratch::new("consolidate_check_gone");
    let (mut plan, mut outcome, same, _, kept_now) = moved_batch(&dir);
    std::fs::remove_file(&kept_now).unwrap();
    plan.recycle.truncate(1);
    let itself = dir.path().join("itself.dem");
    std::fs::write(&itself, b"x").unwrap();
    let itself = itself.to_string_lossy().into_owned();
    plan.recycle.push(PlannedRecycle {
        path: itself.clone(),
        keep: itself,
    });
    let no = AtomicBool::new(false);
    assert!(check_copies(&plan, &mut outcome, &no, &mut |_| {}).is_empty());
    let errors: Vec<(&str, bool)> = outcome
        .recycle_failed
        .iter()
        .map(|f| (f.error.as_str(), f.not_a_copy))
        .collect();
    assert!(errors[0].0.contains("no longer there"), "{errors:?}");
    assert!(errors[1].0.contains("is the copy being kept"), "{errors:?}");
    assert!(errors.iter().all(|(_, not_a_copy)| !not_a_copy));
    assert!(same.exists());
}

#[test]
fn a_cancel_before_the_check_recycles_nothing() {
    let dir = Scratch::new("consolidate_check_cancel");
    let (plan, mut outcome, _, _, _) = moved_batch(&dir);
    let recyclable = check_copies(&plan, &mut outcome, &AtomicBool::new(true), &mut |_| {});
    assert!(recyclable.is_empty());
    assert!(outcome.cancelled);
}

#[test]
fn a_run_keeps_a_copy_that_differs_and_says_why() {
    use crate::demo_compare::tests::demo_bytes;
    let dir = Scratch::new("consolidate_differs");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("m.dem"), demo_bytes(1)).unwrap();
    std::fs::write(src.join("m_other.dem"), demo_bytes(2)).unwrap();
    let dest = dir.path().join("all");
    let path = |p: PathBuf| p.to_string_lossy().into_owned();
    let size = demo_bytes(1).len() as u64;
    let req = Request {
        destination: path(dest.clone()),
        keep: vec![demo(&path(src.join("m.dem")), size)],
        recycle: vec![AuditedDemo {
            keep: path(src.join("m.dem")),
            ..demo(&path(src.join("m_other.dem")), size)
        }],
        ..Request::default()
    };
    let outcome = run(
        &req,
        &dir.path().join("logs"),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.moved.len(), 1, "{outcome:?}");
    assert!(outcome.recycled.is_empty());
    assert_eq!(outcome.recycle_failed.len(), 1);
    assert!(outcome.recycle_failed[0].not_a_copy);
    assert!(src.join("m_other.dem").exists());
    assert!(dest.join("m.dem").exists());
}

#[test]
fn a_copy_keeps_the_modified_time_and_never_replaces() {
    let dir = Scratch::new("consolidate_copy");
    let from = dir.path().join("a.dem");
    let to = dir.path().join("b.dem");
    std::fs::write(&from, vec![7u8; COPY_CHUNK + 10]).unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    std::fs::File::options()
        .write(true)
        .open(&from)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let mut bytes = 0;
    assert!(copy_file(&from, &to, &AtomicBool::new(false), &mut |n| bytes += n).is_ok());
    assert_eq!(bytes, COPY_CHUNK as u64 + 10);
    assert_eq!(std::fs::metadata(&to).unwrap().modified().unwrap(), old);
    // A second copy onto it is refused.
    assert!(copy_file(&from, &to, &AtomicBool::new(false), &mut |_| {}).is_err());
    // A cancelled copy leaves nothing behind.
    let c = dir.path().join("c.dem");
    assert!(matches!(
        copy_file(&from, &c, &AtomicBool::new(true), &mut |_| {}),
        Err(MoveError::Cancelled)
    ));
    assert!(!c.exists());
}

#[test]
fn a_move_never_replaces_and_reports_a_missing_source() {
    let dir = Scratch::new("consolidate_refuse");
    let a = dir.path().join("a.dem");
    let b = dir.path().join("b.dem");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let no = AtomicBool::new(false);
    assert!(matches!(
        move_one(&a, &b, &no, &mut |_| {}),
        Err(MoveError::Failed(_))
    ));
    assert_eq!(std::fs::read(&b).unwrap(), b"b");
    let gone = dir.path().join("gone.dem");
    assert!(matches!(
        move_one(&gone, &dir.path().join("x.dem"), &no, &mut |_| {}),
        Err(MoveError::Failed(_))
    ));
}

/// The toast shows only the first failure's text, so it names the demo.
#[test]
fn a_failed_move_names_the_demo() {
    let dir = Scratch::new("consolidate_fail_names");
    let gone = dir.path().join("gone.dem");
    let plan = Plan {
        moves: vec![PlannedMove {
            from: gone.to_string_lossy().into_owned(),
            to: dir
                .path()
                .join("all")
                .join("gone.dem")
                .to_string_lossy()
                .into_owned(),
            size: 1,
            cross_drive: false,
            renamed: false,
        }],
        ..Plan::default()
    };
    let outcome = run_plan(
        &plan,
        &dir.path().join("logs"),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    let error = &outcome.failed[0].error;
    assert!(error.contains(&gone.display().to_string()), "{error}");
    assert!(
        error.contains(crate::messages::CONSOLIDATE_SOURCE_GONE),
        "{error}"
    );
}

#[test]
fn a_cancelled_undo_keeps_what_it_did_not_reach() {
    let dir = Scratch::new("consolidate_undo_cancel");
    let logs = dir.path().join("logs");
    let log = ConsolidateLog {
        created_unix_secs: 1,
        destination: "x".into(),
        moves: vec![MovedPair {
            from: "a".into(),
            to: "b".into(),
        }],
        undone: false,
    };
    let p = new_log_path(&logs, 1).unwrap();
    save_log(&p, &log);
    let outcome = undo_last(&logs, &AtomicBool::new(true), &mut |_| {});
    assert!(outcome.cancelled);
    assert_eq!(undoable(&logs).unwrap().count, 1);
}
