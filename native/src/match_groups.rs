//! Which match, and which half of it, each demo of a list records, numbered
//! for the `{match}` and `{half}` placeholders and the Master Queue's match
//! grouping (#685). The rules are `analysis::group_matches`'s; this side
//! loads the analyses, numbers the matches and flattens the result for the
//! frontend.
//!
//! A match's number is its place by date: the match whose earliest demo
//! file is oldest is match 1. Undated demos come after dated ones, and ties
//! go by the order the demos were given in, so a list grouped twice numbers
//! the same way.

use analysis::{Analysis, HalfReason, MatchGroup, group_matches, trim_for_grouping};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// What one demo's `{match}` and `{half}` come from.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DemoMatch {
    pub path: String,
    /// 1-based, by the match's earliest demo file date.
    pub match_number: u32,
    /// 1 or 2; `None` when nothing in the demos decided it. Never a guess.
    pub half: Option<u8>,
    pub half_reason: HalfReason,
    /// What every half signal saw, for a tooltip.
    pub half_detail: String,
}

/// One half of a match: the demos (paths) that recorded it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HalfSummary {
    pub half: Option<u8>,
    pub demos: Vec<String>,
    pub reason: HalfReason,
    pub detail: String,
}

/// One match: its number, map, the two teams' clan tags (team 0 is the
/// Allied side of the first half listed) and its halves, the first half
/// first when known.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchSummary {
    pub number: u32,
    pub map: String,
    pub tags: [Option<String>; 2],
    pub halves: Vec<HalfSummary>,
}

/// Every demo's match and half, and every match, in number order.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Grouping {
    pub matches: Vec<MatchSummary>,
    /// In the order the demos were given in.
    pub demos: Vec<DemoMatch>,
}

/// The order to number `groups` in (indices into `groups`): by the earliest
/// `modified` time among a group's demos (each demo an index into
/// `modified`), undated groups last, ties by the group's lowest demo index.
pub fn match_order(groups: &[MatchGroup<usize>], modified: &[Option<u64>]) -> Vec<usize> {
    let key = |g: &MatchGroup<usize>| {
        let demos = g.halves.iter().flat_map(|h| h.demos.iter().copied());
        let earliest = demos
            .clone()
            .filter_map(|i| modified.get(i).copied().flatten())
            .min();
        let first = demos.min().unwrap_or(usize::MAX);
        (earliest.is_none(), earliest.unwrap_or(0), first)
    };
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_key(|&i| key(&groups[i]));
    order
}

/// Groups `demos` (`(path, analysis, file modified time in Unix seconds)`)
/// into numbered matches.
pub fn group_demos(demos: &[(String, &Analysis, Option<u64>)]) -> Grouping {
    let input: Vec<(usize, &Analysis, Option<u64>)> = demos
        .iter()
        .enumerate()
        .map(|(i, (_, a, t))| (i, *a, *t))
        .collect();
    let modified: Vec<Option<u64>> = demos.iter().map(|(_, _, t)| *t).collect();
    let groups = group_matches(&input);
    let path = |i: usize| demos[i].0.clone();

    let mut out = Grouping {
        demos: Vec::with_capacity(demos.len()),
        ..Default::default()
    };
    let mut per_demo: Vec<Option<DemoMatch>> = vec![None; demos.len()];
    for (place, &gi) in match_order(&groups, &modified).iter().enumerate() {
        let g = &groups[gi];
        let number = place as u32 + 1;
        for h in &g.halves {
            for &i in &h.demos {
                per_demo[i] = Some(DemoMatch {
                    path: path(i),
                    match_number: number,
                    half: h.call.half.number(),
                    half_reason: h.call.reason,
                    half_detail: h.call.detail.clone(),
                });
            }
        }
        out.matches.push(MatchSummary {
            number,
            map: g.map.clone(),
            tags: [g.teams[0].tag.clone(), g.teams[1].tag.clone()],
            halves: g
                .halves
                .iter()
                .map(|h| HalfSummary {
                    half: h.call.half.number(),
                    demos: h.demos.iter().map(|&i| path(i)).collect(),
                    reason: h.call.reason,
                    detail: h.call.detail.clone(),
                })
                .collect(),
        });
    }
    // Every demo lands in a group, so none is missing here.
    out.demos = per_demo.into_iter().flatten().collect();
    out
}

/// A demo's analysis for grouping, trimmed to what the grouping reads:
/// from the analyzer cache, else parsed (and cached) when `parse_missing`,
/// else `None`. The second value is the file's modified time.
pub fn load_for_grouping(path: &Path, parse_missing: bool) -> (Option<Analysis>, Option<u64>) {
    let modified = analysis::cache::stamp(path).map(|(_, m)| m);
    let loaded = analysis::cache::load(&crate::analyzer_cache_root(), path)
        .map(|(_, a)| a)
        .or_else(|| {
            parse_missing
                .then(|| crate::run_analyzer_cached(&path.to_path_buf(), |_, _| {}).ok())
                .flatten()
                .map(|(_, a, _)| a)
        })
        .map(|mut a| {
            trim_for_grouping(&mut a);
            a
        });
    (loaded, modified)
}

/// Loads every path on up to `workers` threads, cached demos first (the
/// analyzer cache's `WorkPlan`), and calls `on_done(done so far)` after each.
/// `cancel` stops the loading; the demos loaded so far are returned, the
/// rest `None`. Results come back in `paths` order.
pub fn load_all_for_grouping(
    paths: &[PathBuf],
    parse_missing: bool,
    workers: usize,
    cancel: &AtomicBool,
    on_done: &(dyn Fn(usize) + Sync),
) -> Vec<(Option<Analysis>, Option<u64>)> {
    let plan = analysis::cache::WorkPlan::new(&crate::analyzer_cache_root(), paths);
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let mut loaded: Vec<(usize, (Option<Analysis>, Option<u64>))> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.clamp(1, paths.len().max(1)))
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let Some(&i) = plan.order.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        mine.push((i, load_for_grouping(&paths[i], parse_missing)));
                        on_done(done.fetch_add(1, Ordering::Relaxed) + 1);
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    loaded.sort_by_key(|(i, _)| *i);
    let mut out: Vec<(Option<Analysis>, Option<u64>)> =
        (0..paths.len()).map(|_| (None, None)).collect();
    for (i, item) in loaded {
        out[i] = item;
    }
    out
}

/// Groups the demos at `paths` that could be loaded. Demos that couldn't
/// are left out of the result's `demos`, so the frontend treats them as
/// unknown.
pub fn group_paths(
    paths: &[PathBuf],
    parse_missing: bool,
    workers: usize,
    cancel: &AtomicBool,
    on_done: &(dyn Fn(usize) + Sync),
) -> Grouping {
    let loaded = load_all_for_grouping(paths, parse_missing, workers, cancel, on_done);
    let demos: Vec<(String, &Analysis, Option<u64>)> = paths
        .iter()
        .zip(&loaded)
        .filter_map(|(p, (a, t))| Some((p.to_string_lossy().to_string(), a.as_ref()?, *t)))
        .collect();
    group_demos(&demos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use analysis::{Half, HalfCall, MatchHalf, MatchTeam, Side};

    fn group(demos: &[usize]) -> MatchGroup<usize> {
        MatchGroup {
            map: "dod_anzio".into(),
            teams: [MatchTeam::default(), MatchTeam::default()],
            halves: vec![MatchHalf {
                demos: demos.to_vec(),
                sides: [Side::Allied, Side::Axis],
                call: HalfCall {
                    half: Half::Unknown,
                    reason: HalfReason::Undecided,
                    detail: String::new(),
                },
            }],
        }
    }

    #[test]
    fn matches_are_numbered_by_their_earliest_file_date() {
        // Group 0 has the newest files, group 2 the oldest.
        let groups = [group(&[0, 1]), group(&[2]), group(&[3, 4])];
        let modified = [Some(300), Some(250), Some(200), Some(100), Some(400)];
        assert_eq!(match_order(&groups, &modified), [2, 1, 0]);
    }

    #[test]
    fn undated_matches_come_last_and_ties_go_by_input_order() {
        let groups = [group(&[0]), group(&[1]), group(&[2]), group(&[3])];
        let modified = [None, Some(100), None, Some(100)];
        assert_eq!(match_order(&groups, &modified), [1, 3, 0, 2]);
    }

    #[test]
    fn the_fixture_alone_is_match_one_with_an_unknown_half() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../test-fixtures/ci_fixture.dem");
        let (_, analysis) = crate::run_analyzer_with_progress(&PathBuf::from(path), |_, _| {})
            .expect("fixture parses");
        let grouping = group_demos(&[(path.to_string(), &analysis, Some(1))]);
        assert_eq!(grouping.matches.len(), 1);
        assert_eq!(grouping.matches[0].number, 1);
        assert_eq!(grouping.demos.len(), 1);
        assert_eq!(grouping.demos[0].match_number, 1);
        assert_eq!(grouping.demos[0].path, path);
        // A lone recording of a bot game: no half signal.
        assert_eq!(grouping.demos[0].half, None);
    }

    #[test]
    fn a_path_that_cannot_be_loaded_is_left_out() {
        let cancel = AtomicBool::new(false);
        let paths = [PathBuf::from("C:/nowhere/missing.dem")];
        let grouping = group_paths(&paths, true, 2, &cancel, &|_| {});
        assert!(grouping.demos.is_empty());
        assert!(grouping.matches.is_empty());
    }
}
