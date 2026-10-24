//! Byte counts for a download, sent at most once per ~33 ms (CLAUDE.md's
//! telemetry throttle): a fast line delivers hundreds of chunks a second, and
//! one event per chunk floods the event loop (#957). The first chunk goes out
//! at once, and the caller always sends [`DownloadMeter::progress`] when the
//! download ends, so the bar finishes full.

use serde::Serialize;
use std::time::{Duration, Instant};

pub(crate) const EMIT_EVERY: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct DownloadProgress {
    pub downloaded: u64,
    /// The size the server stated, when it stated one.
    pub total: Option<u64>,
}

#[derive(Debug, Default)]
pub(crate) struct DownloadMeter {
    downloaded: u64,
    total: Option<u64>,
    last_emit: Option<Instant>,
}

impl DownloadMeter {
    /// Counts a chunk; returns the progress to send when one is due at `now`.
    pub fn chunk(
        &mut self,
        len: u64,
        total: Option<u64>,
        now: Instant,
    ) -> Option<DownloadProgress> {
        self.downloaded = self.downloaded.saturating_add(len);
        if total.is_some() {
            self.total = total;
        }
        let due = self
            .last_emit
            .is_none_or(|t| now.saturating_duration_since(t) >= EMIT_EVERY);
        if !due {
            return None;
        }
        self.last_emit = Some(now);
        Some(self.progress())
    }

    /// Everything counted so far, throttle or not: the event to send at the end.
    pub fn progress(&self) -> DownloadProgress {
        DownloadProgress {
            downloaded: self.downloaded,
            total: self.total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake stream: `count` chunks of `len` bytes, one every `gap`.
    fn run(count: u64, len: u64, gap: Duration) -> (Vec<DownloadProgress>, DownloadMeter) {
        let start = Instant::now();
        let mut meter = DownloadMeter::default();
        let sent = (0..count)
            .filter_map(|i| meter.chunk(len, Some(count * len), start + gap * i as u32))
            .collect();
        (sent, meter)
    }

    #[test]
    fn a_chunk_every_millisecond_sends_about_one_event_per_33_ms() {
        // One second of a fast line: 1000 chunks of 16 KB.
        let (sent, meter) = run(1000, 16_384, Duration::from_millis(1));
        assert!(
            (30..=31).contains(&sent.len()),
            "{} events for 1 s of chunks",
            sent.len()
        );
        // The first chunk goes out at once.
        assert_eq!(sent[0].downloaded, 16_384);
        assert_eq!(sent[0].total, Some(16_384_000));
        // Counts only ever go up.
        assert!(sent.windows(2).all(|w| w[0].downloaded < w[1].downloaded));
        // The final event carries every byte, though the throttle held back the
        // last chunk's.
        assert!(sent.last().unwrap().downloaded < 16_384_000);
        assert_eq!(
            meter.progress(),
            DownloadProgress {
                downloaded: 16_384_000,
                total: Some(16_384_000),
            }
        );
    }

    #[test]
    fn slow_chunks_each_go_out() {
        let (sent, _) = run(5, 100, Duration::from_millis(50));
        assert_eq!(
            sent.iter().map(|p| p.downloaded).collect::<Vec<_>>(),
            vec![100, 200, 300, 400, 500]
        );
    }

    #[test]
    fn a_server_that_states_no_size_still_counts_bytes() {
        let mut meter = DownloadMeter::default();
        let first = meter.chunk(10, None, Instant::now()).unwrap();
        assert_eq!(first.total, None);
        assert_eq!(first.downloaded, 10);
    }

    #[test]
    fn the_event_is_shaped_as_the_frontend_reads_it() {
        let json = serde_json::to_value(DownloadProgress {
            downloaded: 12,
            total: Some(45),
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "downloaded": 12, "total": 45 }));
    }
}
