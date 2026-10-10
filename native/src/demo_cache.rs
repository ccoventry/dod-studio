//! Caching many demos ahead of time (#569): every demo given, analysed into
//! the analyzer cache unless it is there already, so later loads take ~15 ms
//! instead of a full parse. Everything that reads a demo shares that cache:
//! the Demo Analyzer, the Master Queue scan, and the in-game DoD Studio
//! window's Highlights tab and Player filter.

use analysis::cache::WorkPlan;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Demos analysed at once. An analysis peaks at about 12 times the demo's
/// size in memory, so a few big demos side by side already take gigabytes.
pub const WORKERS: usize = 2;

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
}

/// Analyses every demo in `paths` into the analyzer cache, `WORKERS` at a
/// time, calling `on_progress` after each and once more when the run ends.
/// Stops taking new demos once `cancel` is set. The demos the cache has go
/// first, then the parses largest first ([`WorkPlan`]), so the count jumps to
/// "cached of total" at once instead of waiting behind an early parse.
pub fn cache_demos(
    paths: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&CacheProgress) + Send + Sync + 'static,
) -> CacheProgress {
    let plan = WorkPlan::new(&crate::analyzer_cache_root(), &paths);
    cache_demos_with(paths, plan, WORKERS, cancel, on_progress, |path| {
        crate::run_analyzer_cached(path, |_, _| {}).map(|(_, _, cached)| cached)
    })
}

/// [`cache_demos`] with the plan, the worker count and the analysis given:
/// `Ok(true)` for a demo that was cached already, `Ok(false)` for one
/// analysed now.
fn cache_demos_with(
    paths: Vec<PathBuf>,
    plan: WorkPlan,
    workers: usize,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&CacheProgress) + Send + Sync + 'static,
    analyse: impl Fn(&PathBuf) -> Result<bool, String> + Send + Sync + 'static,
) -> CacheProgress {
    let total = paths.len();
    let progress = Arc::new(Mutex::new(CacheProgress {
        total,
        cached: plan.cached(),
        bytes_to_parse: plan.bytes_to_parse(),
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
    fn nothing_to_do_still_finishes() {
        let result = cache_demos_with(
            Vec::new(),
            uncached(0),
            WORKERS,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| Ok(false),
        );
        assert_eq!((result.done, result.total), (0, 0));
        assert!(result.finished && !result.cancelled);
    }
}
