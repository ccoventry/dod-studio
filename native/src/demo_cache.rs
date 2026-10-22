//! Caching many demos ahead of time (#569): every demo given, analysed into
//! the analyzer cache unless it is there already, so later loads take ~15 ms
//! instead of a full parse. Everything that reads a demo shares that cache:
//! the Demo Analyzer, the Master Queue scan, and the in-game DoD Studio
//! window's Highlights tab and Player filter.
//!
//! A run first upgrades the entries an older cache schema left for these
//! demos (`analysis::cache::migrations`), milliseconds each, and parses only
//! what is left: after a schema bump, most of a folder is cached again in
//! seconds instead of a parse per demo.

use analysis::cache::{NotUpgraded, WorkPlan};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;

/// Demos analysed at once. An analysis peaks at about 12 times the demo's
/// size in memory, so a few big demos side by side already take gigabytes.
pub const WORKERS: usize = 2;

/// Older entries upgraded at once: a few MB each, and half the time is
/// reading the file, so more than the parse workers.
const UPGRADE_WORKERS: usize = 4;

/// The least time between two progress reports while upgrading, which can
/// finish hundreds of entries a second (CLAUDE.md's ~33 ms throttle).
const UPGRADE_REPORT_MS: u32 = 33;

/// The run's first step: upgrading the older cache entries of the demos the
/// current cache doesn't have.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct UpgradeProgress {
    /// The step is under way: the page says "Upgrading cached analyses".
    pub running: bool,
    /// Demos with an older entry to upgrade, and how many are done.
    pub total: usize,
    pub done: usize,
    /// Of `done`, the ones upgraded and cached now.
    pub upgraded: usize,
    /// Of `done`, the ones that could not be upgraded and are parsed with
    /// the rest, by reason.
    pub needs_reparse: BTreeMap<String, usize>,
}

impl UpgradeProgress {
    /// The upgrades that need a parse instead.
    pub fn reparse_count(&self) -> usize {
        self.needs_reparse.values().sum()
    }
}

/// Where a run is, reported once the cache check is done, after every demo,
/// and once more when the run ends.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct CacheProgress {
    /// Demos finished, whichever way.
    pub done: usize,
    pub total: usize,
    /// Of `done`, the ones that were cached already.
    pub already: usize,
    /// Of `done`, the ones that could not be analysed.
    pub failed: usize,
    /// Demos the analyzer cache had when the run began. They go first.
    pub cached: usize,
    /// Of `done`, the ones that were not in the cache when the run began.
    pub parsed: usize,
    /// The size of the demos the cache didn't have, and how much of it is
    /// done: the page's time left goes by bytes (#687).
    pub bytes_to_parse: u64,
    pub bytes_parsed: u64,
    /// The demo just finished.
    pub last: Option<String>,
    /// The run is over: every demo done, or stopped.
    pub finished: bool,
    pub cancelled: bool,
    /// The upgrade step, done before any of the above counts move.
    pub upgrade: UpgradeProgress,
}

/// Analyses every demo in `paths` into the analyzer cache, `WORKERS` at a
/// time, calling `on_progress` after each and once more when the run ends.
/// Stops taking new demos once `cancel` is set. First the older entries of
/// demos the cache doesn't have are upgraded ([`upgrade_entries_with`]). Then
/// the demos the cache has go first, then the parses largest first
/// ([`WorkPlan`]), so the count jumps to "cached of total" at once instead of
/// waiting behind an early parse.
pub fn cache_demos(
    paths: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&CacheProgress) + Send + Sync + 'static,
) -> CacheProgress {
    let root = crate::analyzer_cache_root();
    let total = paths.len();
    let older: Vec<PathBuf> = paths
        .iter()
        .filter(|p| {
            !analysis::cache::is_fresh(&root, p) && analysis::cache::has_older_entry(&root, p)
        })
        .cloned()
        .collect();
    let on_progress = Arc::new(on_progress);
    let report = on_progress.clone();
    let upgrade = upgrade_entries_with(
        &older,
        UPGRADE_WORKERS,
        &cancel,
        |upgrade| {
            report(&CacheProgress {
                total,
                upgrade: upgrade.clone(),
                ..CacheProgress::default()
            })
        },
        |path| analysis::cache::upgrade(&root, path).map(|_| ()),
    );
    if upgrade.total > 0 {
        let reasons: Vec<String> = upgrade
            .needs_reparse
            .iter()
            .map(|(why, n)| format!("{n}: {why}"))
            .collect();
        crate::log_markdown(&format!(
            "Cache demos: upgraded {} of {} older cache entries{}",
            upgrade.upgraded,
            upgrade.total,
            if reasons.is_empty() {
                String::new()
            } else {
                format!("; parsing the rest ({})", reasons.join(", "))
            }
        ));
    }
    let plan = WorkPlan::new(&root, &paths);
    cache_demos_with(
        paths,
        plan,
        WORKERS,
        cancel,
        upgrade,
        move |progress| on_progress(progress),
        |path| crate::run_analyzer_cached(path, |_, _| {}).map(|(_, _, cached)| cached),
    )
}

/// Upgrades the older entry of each of `paths`, `workers` at a time, with
/// `upgrade`; reports when it starts, at most every [`UPGRADE_REPORT_MS`]
/// while it runs, and after the last one, all with `running` set. Stops
/// taking new ones once `cancel` is set. Returns the totals, not running.
fn upgrade_entries_with(
    paths: &[PathBuf],
    workers: usize,
    cancel: &AtomicBool,
    on_progress: impl Fn(&UpgradeProgress) + Sync,
    upgrade: impl Fn(&PathBuf) -> Result<(), NotUpgraded> + Sync,
) -> UpgradeProgress {
    if paths.is_empty() {
        return UpgradeProgress::default();
    }
    let progress = Mutex::new(UpgradeProgress {
        running: true,
        total: paths.len(),
        ..UpgradeProgress::default()
    });
    on_progress(&progress.lock().unwrap_or_else(|e| e.into_inner()).clone());
    let started = Instant::now();
    // Milliseconds since `started` of the last report, shared by the workers.
    let last_report = AtomicU32::new(0);
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers.clamp(1, paths.len()) {
            scope.spawn(|| {
                while !cancel.load(Ordering::Acquire) {
                    let Some(path) = paths.get(next.fetch_add(1, Ordering::AcqRel)) else {
                        break;
                    };
                    let result = upgrade(path);
                    let snapshot = {
                        let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                        p.done += 1;
                        match result {
                            Ok(()) => p.upgraded += 1,
                            // Another writer saved a current entry first, or
                            // the file changed since: the parse step sorts it
                            // out.
                            Err(NotUpgraded::NoOlderEntry) => {}
                            Err(NotUpgraded::NeedsReparse(why)) => {
                                *p.needs_reparse.entry(why.to_string()).or_default() += 1
                            }
                        }
                        let now = started.elapsed().as_millis() as u32;
                        let due = p.done == p.total
                            || now.saturating_sub(last_report.load(Ordering::Relaxed))
                                >= UPGRADE_REPORT_MS;
                        if due {
                            last_report.store(now, Ordering::Relaxed);
                        }
                        due.then(|| p.clone())
                    };
                    if let Some(snapshot) = snapshot {
                        on_progress(&snapshot);
                    }
                }
            });
        }
    });
    let mut last = progress.into_inner().unwrap_or_else(|e| e.into_inner());
    last.running = false;
    last
}

/// [`cache_demos`] with the plan, the worker count and the analysis given:
/// `Ok(true)` for a demo that was cached already, `Ok(false)` for one
/// analysed now.
fn cache_demos_with(
    paths: Vec<PathBuf>,
    plan: WorkPlan,
    workers: usize,
    cancel: Arc<AtomicBool>,
    upgrade: UpgradeProgress,
    on_progress: impl Fn(&CacheProgress) + Send + Sync + 'static,
    analyse: impl Fn(&PathBuf) -> Result<bool, String> + Send + Sync + 'static,
) -> CacheProgress {
    let total = paths.len();
    let progress = Arc::new(Mutex::new(CacheProgress {
        total,
        cached: plan.cached(),
        bytes_to_parse: plan.bytes_to_parse(),
        upgrade,
        ..CacheProgress::default()
    }));
    // The cache check is done: the page leaves "Checking the analyzer
    // cache…" now rather than after the first demo, which may be a parse.
    on_progress(&progress.lock().unwrap_or_else(|e| e.into_inner()).clone());
    let next = Arc::new(AtomicUsize::new(0));
    let paths = Arc::new(paths);
    let plan = Arc::new(plan);
    let on_progress = Arc::new(on_progress);
    let analyse = Arc::new(analyse);
    let workers: Vec<_> = (0..workers.min(total).max(1))
        .map(|_| {
            let (progress, next, paths, plan, cancel, on_progress, analyse) = (
                progress.clone(),
                next.clone(),
                paths.clone(),
                plan.clone(),
                cancel.clone(),
                on_progress.clone(),
                analyse.clone(),
            );
            std::thread::spawn(move || {
                loop {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let Some(&index) = plan.order.get(next.fetch_add(1, Ordering::AcqRel)) else {
                        break;
                    };
                    let Some(path) = paths.get(index) else { break };
                    let result = analyse(path);
                    let snapshot = {
                        let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                        p.done += 1;
                        if !plan.fresh.get(index).copied().unwrap_or(false) {
                            p.parsed += 1;
                            p.bytes_parsed += plan.sizes.get(index).copied().unwrap_or(0);
                        }
                        match result {
                            Ok(true) => p.already += 1,
                            Ok(false) => {}
                            Err(why) => {
                                p.failed += 1;
                                crate::log_markdown(&format!(
                                    "Cache demos: could not analyse {}: {why}",
                                    path.display()
                                ));
                            }
                        }
                        p.last = path.file_name().map(|n| n.to_string_lossy().into_owned());
                        p.clone()
                    };
                    on_progress(&snapshot);
                }
            })
        })
        .collect();
    for worker in workers {
        let _ = worker.join();
    }
    let mut last = progress.lock().unwrap_or_else(|e| e.into_inner()).clone();
    last.finished = true;
    last.cancelled = cancel.load(Ordering::Acquire) && last.done < last.total;
    on_progress(&last);
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(n: usize) -> Vec<PathBuf> {
        (0..n)
            .map(|i| PathBuf::from(format!("demo{i}.dem")))
            .collect()
    }

    /// A plan for `n` demos none of which the cache has, all the same size:
    /// read in list order.
    fn uncached(n: usize) -> WorkPlan {
        WorkPlan::from_parts(vec![false; n], vec![0; n])
    }

    #[test]
    fn every_demo_is_counted_once() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_progress = seen.clone();
        let result = cache_demos_with(
            paths(7),
            uncached(7),
            WORKERS,
            Arc::new(AtomicBool::new(false)),
            UpgradeProgress::default(),
            move |p| seen_by_progress.lock().unwrap().push(p.clone()),
            |path| match path.to_string_lossy().as_ref() {
                "demo1.dem" | "demo4.dem" => Ok(true),
                "demo5.dem" => Err("bad demo".to_string()),
                _ => Ok(false),
            },
        );
        assert_eq!(
            (result.done, result.total, result.already, result.failed),
            (7, 7, 2, 1)
        );
        assert!(result.finished && !result.cancelled);
        let seen = seen.lock().unwrap();
        // One report once the cache check is done, one per demo, then the
        // closing one.
        assert_eq!(seen.len(), 9);
        assert_eq!(seen[0].done, 0);
        assert!(seen.last().unwrap().finished);
    }

    #[test]
    fn cached_demos_go_first_then_the_largest_parse() {
        // demo1 and demo3 are cached; the parses are 5, 90 and 40 MB.
        let plan = WorkPlan::from_parts(
            vec![false, true, false, true, false],
            vec![5, 80, 90, 10, 40],
        );
        let called = Arc::new(Mutex::new(Vec::new()));
        let called_by_analyse = called.clone();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let reports_by_progress = reports.clone();
        let result = cache_demos_with(
            paths(5),
            plan,
            1,
            Arc::new(AtomicBool::new(false)),
            UpgradeProgress::default(),
            move |p| reports_by_progress.lock().unwrap().push(p.clone()),
            move |path| {
                called_by_analyse
                    .lock()
                    .unwrap()
                    .push(path.to_string_lossy().into_owned());
                Ok(path.to_string_lossy() == "demo1.dem" || path.to_string_lossy() == "demo3.dem")
            },
        );
        assert_eq!(
            *called.lock().unwrap(),
            [
                "demo1.dem",
                "demo3.dem",
                "demo2.dem",
                "demo4.dem",
                "demo0.dem"
            ]
        );
        assert_eq!((result.cached, result.already, result.parsed), (2, 2, 3));
        assert_eq!((result.bytes_to_parse, result.bytes_parsed), (135, 135));
        // After the cached two and the first parse: 90 of 135 bytes parsed.
        let reports = reports.lock().unwrap();
        assert_eq!((reports[2].done, reports[2].parsed), (2, 0));
        assert_eq!((reports[3].parsed, reports[3].bytes_parsed), (1, 90));
    }

    #[test]
    fn a_stop_takes_no_new_demos() {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let result = cache_demos_with(
            paths(50),
            uncached(50),
            WORKERS,
            cancel,
            UpgradeProgress::default(),
            |_| {},
            move |_| {
                stop.store(true, Ordering::Release);
                Ok(false)
            },
        );
        // At most one demo per worker had started when the first one stopped it.
        assert!(result.done <= WORKERS, "{result:?}");
        assert!(result.finished && result.cancelled);
    }

    #[test]
    fn upgrades_are_counted_by_outcome_and_reported_while_running() {
        let reports = Mutex::new(Vec::new());
        let paths = paths(6);
        let result = upgrade_entries_with(
            &paths,
            UPGRADE_WORKERS,
            &AtomicBool::new(false),
            |p| reports.lock().unwrap().push(p.clone()),
            |path| match path.to_string_lossy().as_ref() {
                "demo1.dem" | "demo4.dem" => Err(NotUpgraded::NeedsReparse("left")),
                "demo2.dem" => Err(NotUpgraded::NeedsReparse("bad")),
                "demo3.dem" => Err(NotUpgraded::NoOlderEntry),
                _ => Ok(()),
            },
        );
        assert_eq!((result.total, result.done, result.upgraded), (6, 6, 2));
        assert_eq!(
            result.needs_reparse,
            BTreeMap::from([("bad".to_string(), 1), ("left".to_string(), 2)])
        );
        assert_eq!(result.reparse_count(), 3);
        assert!(!result.running);
        let reports = reports.lock().unwrap();
        // The start, then at least the last one; every one while running.
        assert_eq!(reports[0].done, 0);
        assert_eq!(reports.last().unwrap().done, 6);
        assert!(reports.iter().all(|r| r.running && r.total == 6));
    }

    #[test]
    fn no_older_entries_is_no_upgrade_step() {
        let result = upgrade_entries_with(
            &[],
            UPGRADE_WORKERS,
            &AtomicBool::new(false),
            |_| panic!("nothing to report"),
            |_| Ok(()),
        );
        assert_eq!(result, UpgradeProgress::default());
    }

    #[test]
    fn a_stop_takes_no_new_upgrades() {
        let cancel = AtomicBool::new(false);
        let result = upgrade_entries_with(
            &paths(50),
            UPGRADE_WORKERS,
            &cancel,
            |_| {},
            |_| {
                cancel.store(true, Ordering::Release);
                Ok(())
            },
        );
        assert!(result.done <= UPGRADE_WORKERS, "{result:?}");
    }

    #[test]
    fn the_upgrade_totals_ride_along_with_every_parse_report() {
        let upgrade = UpgradeProgress {
            total: 3,
            done: 3,
            upgraded: 2,
            needs_reparse: BTreeMap::from([("left".to_string(), 1)]),
            running: false,
        };
        let result = cache_demos_with(
            paths(2),
            uncached(2),
            WORKERS,
            Arc::new(AtomicBool::new(false)),
            upgrade.clone(),
            |_| {},
            |_| Ok(false),
        );
        assert_eq!(result.upgrade, upgrade);
    }

    #[test]
    fn nothing_to_do_still_finishes() {
        let result = cache_demos_with(
            Vec::new(),
            uncached(0),
            WORKERS,
            Arc::new(AtomicBool::new(false)),
            UpgradeProgress::default(),
            |_| {},
            |_| Ok(false),
        );
        assert_eq!((result.done, result.total), (0, 0));
        assert!(result.finished && !result.cancelled);
    }
}
