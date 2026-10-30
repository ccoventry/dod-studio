//! Running one FFmpeg render: spawning it, turning its `-progress` lines
//! into updates, stopping it on Cancel, and reporting how it ended. FFmpeg
//! is polled, never waited on blindly: the cancel flag is checked every
//! 200 ms while it reads frames and while it exits.

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::time::Interval;

use super::{RenderJob, RenderUpdate, record_output_size, remove_partial_output};

/// Runs FFmpeg with `args` for `job` and sends every update up to its
/// `Finished`. `out_file` is the file the last argument names: removed if
/// the render is cancelled or fails, unless it was there before.
pub(super) async fn run_ffmpeg(job: &RenderJob, args: Vec<String>, out_file: &Path) {
    let mut cmd = ffmpeg_command(job, args);

    job.send(RenderUpdate::Status(
        job.job_id.clone(),
        "Rendering".to_string(),
    ));

    let existed_before = out_file.exists();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            job.refuse(spawn_error_text(&e));
            return;
        }
    };

    // Both pipes are set to piped by `ffmpeg_command`, so `take()` has them.
    // Stderr is read on its own task so a full pipe can't stall FFmpeg.
    let stderr_log = collect_stderr(child.stderr.take().unwrap());
    // Held until the job ends: FFmpeg's stdout stays open while it exits.
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(200));

    let exit_status = match read_progress(job, &mut reader, &mut interval).await {
        Polled::Cancelled => None,
        Polled::Done(()) => match wait_for_exit(job, &mut child, &mut interval).await {
            Polled::Cancelled => None,
            Polled::Done(status) => Some(status),
        },
    };
    let Some(exit_status) = exit_status else {
        let _ = child.kill().await;
        let _ = child.wait().await;
        remove_partial_output(out_file, existed_before).await;
        job.finish_cancelled();
        return;
    };

    let err_log = stderr_text(&stderr_log);

    match exit_status {
        Ok(status) if status.success() => {
            if let Ok(meta) = std::fs::metadata(out_file) {
                record_output_size(
                    &job.clip,
                    job.config.target_codec,
                    &job.config.custom_codec_args,
                    meta.len(),
                );
            }
            job.finish_ok(out_file.to_string_lossy().into_owned());
        }
        Ok(status) => {
            let exit_code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let error_msg = format!(
                "FFmpeg failed with exit code: {}. Log: {}",
                exit_code,
                err_log.as_deref().unwrap_or("")
            );
            remove_partial_output(out_file, existed_before).await;
            job.finish_error(error_msg);
        }
        Err(e) => {
            job.finish_error(format!("Failed to wait for FFmpeg: {}", e));
        }
    }
}

/// The FFmpeg process for `args`: run in the take folder (the inputs are
/// relative to it), no console window, both pipes read, and killed if the
/// job is dropped.
fn ffmpeg_command(job: &RenderJob, args: Vec<String>) -> Command {
    let mut cmd = Command::new(&job.config.ffmpeg_path);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.kill_on_drop(true);
    cmd.args(args);
    cmd.current_dir(&job.take_folder);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd
}

/// What the user is told when FFmpeg won't start: where to set it up when
/// there is no FFmpeg at all, the OS error otherwise.
fn spawn_error_text(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "FFmpeg not found. Please install FFmpeg or set a custom path in Settings.".to_string()
    } else {
        format!("Failed to spawn FFmpeg process: {}", e)
    }
}

/// Collects FFmpeg's stderr on a task of its own, so a full pipe can never
/// deadlock the render; the text is the failure's log.
fn collect_stderr(mut stderr_handle: ChildStderr) -> Arc<Mutex<String>> {
    let stderr_log = Arc::new(Mutex::new(String::new()));
    let stderr_log_clone = Arc::clone(&stderr_log);
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];
        while let Ok(n) = stderr_handle.read(&mut buf).await {
            if n == 0 {
                break;
            }
            if let Ok(s) = std::str::from_utf8(&buf[..n])
                && let Ok(mut log) = stderr_log_clone.lock()
            {
                log.push_str(s);
            }
        }
    });
    stderr_log
}

/// What stderr collected so far, `None` when it is blank.
fn stderr_text(stderr_log: &Mutex<String>) -> Option<String> {
    if let Ok(log) = stderr_log.lock() {
        if log.trim().is_empty() {
            None
        } else {
            Some(log.clone())
        }
    } else {
        None
    }
}

/// How a polling loop ended: its result, or Cancel seen on a tick.
enum Polled<T> {
    Done(T),
    Cancelled,
}

/// Sends progress and speed updates from FFmpeg's `-progress` output until
/// it closes, checking the cancel flag on every tick of `interval`.
async fn read_progress(
    job: &RenderJob,
    reader: &mut BufReader<ChildStdout>,
    interval: &mut Interval,
) -> Polled<()> {
    let mut line = String::new();
    let mut progress = ProgressLines::default();
    loop {
        line.clear();
        tokio::select! {
            _ = interval.tick() => {
                if job.cancel.load(Ordering::Relaxed) {
                    return Polled::Cancelled;
                }
            }
            read_res = reader.read_line(&mut line) => {
                match read_res {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Some(update) = progress.update_for(&job.job_id, &line, job.clip.frame_count) {
                            job.send(update);
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
    Polled::Done(())
}

/// Waits for FFmpeg to exit, checking the cancel flag on every tick of
/// `interval`.
async fn wait_for_exit(
    job: &RenderJob,
    child: &mut Child,
    interval: &mut Interval,
) -> Polled<std::io::Result<std::process::ExitStatus>> {
    loop {
        tokio::select! {
            res = child.wait() => {
                return Polled::Done(res);
            }
            _ = interval.tick() => {
                if job.cancel.load(Ordering::Relaxed) {
                    return Polled::Cancelled;
                }
            }
        }
    }
}

/// Reads FFmpeg's `-progress` lines (`key=value`): `frame` becomes a
/// percentage of the take's frames, `fps` and `speed` one speed line that
/// shows the latest of both.
struct ProgressLines {
    fps: String,
    speed: String,
}

impl Default for ProgressLines {
    fn default() -> Self {
        Self {
            fps: "0".to_string(),
            speed: "0x".to_string(),
        }
    }
}

impl ProgressLines {
    /// The update one line makes, if any. A take with no frame count gets
    /// no percentage.
    fn update_for(&mut self, job_id: &str, line: &str, frame_count: usize) -> Option<RenderUpdate> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let pos = trimmed.find('=')?;
        let key = trimmed[..pos].trim();
        let val = trimmed[pos + 1..].trim();
        match key {
            "frame" => {
                let current_frame = val.parse::<usize>().ok()?;
                let ratio = (current_frame * 100).checked_div(frame_count)?;
                let percent = std::cmp::min(100, ratio) as u32;
                Some(RenderUpdate::Progress(job_id.to_string(), percent))
            }
            "fps" => {
                self.fps = val.to_string();
                Some(self.speed_update(job_id))
            }
            "speed" => {
                self.speed = val.to_string();
                Some(self.speed_update(job_id))
            }
            _ => None,
        }
    }

    fn speed_update(&self, job_id: &str) -> RenderUpdate {
        RenderUpdate::Speed(
            job_id.to_string(),
            format!("{} fps ({})", self.fps, self.speed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(lines: &[&str], frame_count: usize) -> Vec<String> {
        let mut progress = ProgressLines::default();
        lines
            .iter()
            .filter_map(|l| progress.update_for("3", l, frame_count))
            .map(|u| match u {
                RenderUpdate::Progress(id, p) => format!("{id} progress {p}"),
                RenderUpdate::Speed(id, s) => format!("{id} speed {s}"),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn frame_lines_become_a_percentage_capped_at_100() {
        assert_eq!(
            read(
                &["frame=0\n", "frame=25", " frame = 50 \r\n", "frame=120"],
                100
            ),
            [
                "3 progress 0",
                "3 progress 25",
                "3 progress 50",
                "3 progress 100"
            ]
        );
        // 2 of 3 frames rounds down.
        assert_eq!(read(&["frame=2"], 3), ["3 progress 66"]);
    }

    #[test]
    fn a_take_with_no_frame_count_or_a_bad_frame_gets_no_percentage() {
        assert!(read(&["frame=10"], 0).is_empty());
        assert!(read(&["frame=N/A", "frame=-1"], 100).is_empty());
    }

    #[test]
    fn fps_and_speed_each_send_the_latest_of_both() {
        assert_eq!(
            read(&["fps=0.0", "speed=1.5x", "fps=240"], 100),
            [
                "3 speed 0.0 fps (0x)",
                "3 speed 0.0 fps (1.5x)",
                "3 speed 240 fps (1.5x)",
            ]
        );
    }

    #[test]
    fn other_keys_blank_lines_and_lines_without_a_value_are_ignored() {
        assert!(
            read(
                &[
                    "",
                    "  \n",
                    "progress=continue",
                    "out_time=00:00:01",
                    "frame"
                ],
                100
            )
            .is_empty()
        );
    }

    #[test]
    fn a_missing_ffmpeg_points_at_settings() {
        let not_found = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(
            spawn_error_text(&not_found),
            "FFmpeg not found. Please install FFmpeg or set a custom path in Settings."
        );
        let other = std::io::Error::other("boom");
        assert_eq!(
            spawn_error_text(&other),
            "Failed to spawn FFmpeg process: boom"
        );
    }

    #[test]
    fn a_blank_stderr_is_no_log() {
        assert_eq!(stderr_text(&Mutex::new(" \n".to_string())), None);
        assert_eq!(
            stderr_text(&Mutex::new("Unknown encoder\n".to_string())),
            Some("Unknown encoder\n".to_string())
        );
    }
}
