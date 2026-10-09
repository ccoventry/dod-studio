//! Caching many demos ahead of time (#569): every demo given, analysed into
//! the analyzer cache unless it is there already, so later loads take ~15 ms
//! instead of a full parse. Everything that reads a demo shares that cache:
//! the Demo Analyzer, the Master Queue scan, and the in-game DoD Studio
//! window's Highlights tab and Player filter.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Demos analysed at once. An analysis peaks at about 12 times the demo's
/// size in memory, so a few big demos side by side already take gigabytes.
pub const WORKERS: usize = 2;

/// Where a run is, reported after every demo.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct CacheProgress {
    /// Demos finished, whichever way.
    pub done: usize,
    pub total: usize,
    /// Of `done`, the ones that were cached already.
    pub already: usize,
    /// Of `done`, the ones that could not be analysed.
    pub failed: usize,
    /// The demo just finished.
    pub last: Option<String>,
    /// The run is over: every demo done, or stopped.
    pub finished: bool,
    pub cancelled: bool,
}

/// Analyses every demo in `paths` into the analyzer cache, `WORKERS` at a
/// time, calling `on_progress` after each and once more when the run ends.
/// Stops taking new demos once `cancel` is set.
pub fn cache_demos(
    paths: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&CacheProgress) + Send + Sync + 'static,
) -> CacheProgress {
    cache_demos_with(paths, cancel, on_progress, |path| {
        crate::run_analyzer_cached(path, |_, _| {}).map(|(_, _, cached)| cached)
    })
}

/// [`cache_demos`] with the analysis given: `Ok(true)` for a demo that was
/// cached already, `Ok(false)` for one analysed now.
fn cache_demos_with(
    paths: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(&CacheProgress) + Send + Sync + 'static,
    analyse: impl Fn(&PathBuf) -> Result<bool, String> + Send + Sync + 'static,
) -> CacheProgress {
    let total = paths.len();
    let progress = Arc::new(Mutex::new(CacheProgress {
        total,
        ..CacheProgress::default()
    }));
    let next = Arc::new(AtomicUsize::new(0));
    let paths = Arc::new(paths);
    let on_progress = Arc::new(on_progress);
    let analyse = Arc::new(analyse);
    let workers: Vec<_> = (0..WORKERS.min(total.max(1)))
        .map(|_| {
            let (progress, next, paths, cancel, on_progress, analyse) = (
                progress.clone(),
                next.clone(),
                paths.clone(),
                cancel.clone(),
                on_progress.clone(),
                analyse.clone(),
            );
            std::thread::spawn(move || {
                loop {
                    if cancel.load(Ordering::Acquire) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::AcqRel);
                    let Some(path) = paths.get(index) else { break };
                    let result = analyse(path);
                    let snapshot = {
                        let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                        p.done += 1;
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

    #[test]
    fn every_demo_is_counted_once() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_progress = seen.clone();
        let result = cache_demos_with(
            paths(7),
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
        // One report per demo, then the closing one.
        assert_eq!(seen.len(), 8);
        assert!(seen.last().unwrap().finished);
    }

    #[test]
    fn a_stop_takes_no_new_demos() {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let result = cache_demos_with(
            paths(50),
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
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |_| Ok(false),
        );
        assert_eq!((result.done, result.total), (0, 0));
        assert!(result.finished && !result.cancelled);
    }
}
