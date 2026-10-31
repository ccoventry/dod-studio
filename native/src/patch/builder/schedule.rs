//! A chain job's scheduled commands: the stages each clip plays through
//! (speed drop, sound flush, record start and stop, post-roll end), the
//! console markers Studio reads back, the user's Scheduled Commands, the
//! breadcrumbs and the hand-over to the next demo. Also the director events,
//! the job's other tick-keyed list.

use super::*;

/// One svc_director STUFFTEXT event per highlight, labelled as the highlight
/// table shows it ("#<row>: <kill_count> kills: <timeline_string>"), plus the
/// MATCH_START and DEMO_END markers, sorted by tick.
pub(super) fn director_events(group: &HighlightGroup) -> Vec<(i32, String)> {
    let mut events: Vec<(i32, String)> = group
        .highlights
        .iter()
        .enumerate()
        .map(|(i, (_, s))| {
            let label = format!("#{}: {} kills: {}", i + 1, s.kill_count, s.timeline_string);
            (s.start_tick, label)
        })
        .collect();

    let first = group.first();
    let match_tick = first.match_start_tick.unwrap_or(0);
    events.push((match_tick, "echo [dod-studio] MATCH_START".to_string()));
    events.push((
        first.total_demo_frames,
        "echo [dod-studio] DEMO_END".to_string(),
    ));
    events.sort_by_key(|e| e.0);
    events
}

/// The frame times a highlight's ticks are placed by: the batch's shared
/// array for its demo when there is one, else the highlight's own.
pub(super) fn frame_times_for<'a>(
    streak: &'a CaptureStreak,
    global_arrays: &'a std::collections::HashMap<std::path::PathBuf, Arc<Vec<f32>>>,
) -> &'a [f32] {
    global_arrays
        .get(std::path::Path::new(&streak.source_demo))
        .map(|a| a.as_slice())
        .unwrap_or_else(|| streak.frame_times.as_slice())
}

/// Where one block's stages land, in demo ticks.
pub(super) struct ClipWindow {
    /// The block's first and last kill, as frame indices into the frame
    /// times: what the record bounds and the Scheduled Commands measure from.
    pub first_kill_frame: usize,
    pub last_kill_frame: usize,
    /// The pre-roll's start, where playback drops back to real time.
    pub speed_drop_tick: i32,
    /// The stopsound flush, never ahead of the speed drop.
    pub sound_flush_tick: i32,
    pub record_start_tick: i32,
    pub record_stop_tick: i32,
    /// The post-roll's end, where fast-forward resumes.
    pub post_roll_end_tick: i32,
    /// Five frames before the demo's last: nothing is scheduled past it.
    pub exit_frame: i32,
    /// The demo's last frame: a command scheduled past it is never written.
    pub last_frame: i32,
    /// The post-roll reached the demo's last ten frames and was cut to
    /// `exit_frame`; the hand-over then runs at `exit_frame` too.
    pub cut_at_demo_end: bool,
}

/// Places one block's stages: the record bounds a start lead before its first
/// kill and a stop trail after its last, the pre-roll and the sound flush
/// before the record start, the post-roll after the record stop, everything
/// kept clear of the demo's end.
pub(super) fn clip_window(
    streak: &CaptureStreak,
    frame_times: &[f32],
    config: &PatcherConfig,
    demo_fps: f32,
) -> ClipWindow {
    // Always use streak.total_demo_frames (which maps to the demo's true
    // playback_frames from the header) as the authoritative final frame count.
    // Do not trust frame_times.len() on its own, as it is truncated
    // at the demo section boundary (type_byte == 5) during scanning.
    let absolute_final_frame = streak.total_demo_frames.max(frame_times.len() as i32);
    let exit_frame = absolute_final_frame.saturating_sub(5);
    let danger_zone = absolute_final_frame.saturating_sub(10);

    // Resolve the physical frame index for the start/end kills directly from
    // kills[i].0 (= frame_index, the 1-based frame counter used during scanning),
    // which is aligned with frame_times[].
    //
    // NOTE: Do NOT apply a .min(frame_times.len()-1) clamp here.
    // When frame_times is empty that saturates to 0 and destroys the index.
    // find_tick_backwards / find_tick_forwards already clamp start_frame
    // internally, and return start_frame as-is when frame_times is empty.
    //
    // NOTE: Do NOT use viewdemo_times here. viewdemo_times[i] stores
    // viewdemo_offset (= SVC_TIME server game clock, e.g. 300.0 s from server
    // start), while frame_times[] contains demo-playback timestamps starting
    // near 0.0. The two domains are incompatible — a position() search would
    // always return None → unwrap_or(0) → every command collapsed to tick 0.
    let physical_frame = first_kill_frame(streak).max(0) as usize;
    let physical_end_frame = last_kill_frame(streak).max(0) as usize;

    let record_start_tick = find_tick_backwards(
        physical_frame,
        config.record_start_lead,
        frame_times,
        demo_fps,
    );
    // Pre-roll is the settle window: playback drops back to normal speed
    // this far ahead of the record start so audio isn't glitched by the
    // fast-forward, then stopsound flushes just before recording begins.
    // The sound flush is clamped so it can never precede the speed drop
    // (which it would for any pre-roll under a second).
    let speed_drop_tick = find_tick_backwards(
        record_start_tick.max(0) as usize,
        config.pre_roll_seconds,
        frame_times,
        demo_fps,
    );
    let sound_lead = config.pre_roll_seconds.min(SOUND_FLUSH_LEAD_SECONDS);
    let sound_flush_tick = find_tick_backwards(
        record_start_tick.max(0) as usize,
        sound_lead,
        frame_times,
        demo_fps,
    );
    let record_stop_tick = find_tick_forwards(
        physical_end_frame,
        config.record_stop_trail,
        frame_times,
        demo_fps,
    );
    let post_roll_end_tick = find_tick_forwards(
        record_stop_tick.max(0) as usize,
        config.post_roll_seconds,
        frame_times,
        demo_fps,
    );

    let cut_at_demo_end = post_roll_end_tick >= danger_zone;
    ClipWindow {
        first_kill_frame: physical_frame,
        last_kill_frame: physical_end_frame,
        speed_drop_tick,
        sound_flush_tick,
        record_start_tick,
        record_stop_tick: record_stop_tick.min(exit_frame),
        post_roll_end_tick: if cut_at_demo_end {
            exit_frame
        } else {
            post_roll_end_tick.min(exit_frame)
        },
        exit_frame,
        last_frame: absolute_final_frame,
        cut_at_demo_end,
    }
}

/// Logs that highlight `index` (0-based) had its post-roll cut at the demo's end.
pub(super) fn log_post_roll_cut(index: usize, streak: &CaptureStreak) {
    let demo_file_name = std::path::Path::new(&streak.source_demo)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    crate::log_markdown(&format!(
        "⚠️ **EOF Boundary Reached:** Highlight #{} in demo '{}' (Player: {}) has a post-roll that exceeds the demo's end frame. Post-roll truncated to save batch.",
        index + 1,
        demo_file_name,
        streak.target_player.as_deref().unwrap_or("Unknown")
    ));
}

/// The upper-case word a Scheduled Command's markers name its relation by.
fn relation_label(relation: &CommandRelation) -> &'static str {
    match relation {
        CommandRelation::Before => "BEFORE",
        CommandRelation::After => "AFTER",
    }
}

/// The tick a Scheduled Command runs at: its offset before the block's first
/// kill or after its last. One that lands exactly on a stage's tick (the
/// speed drop, the sound flush or the record start before; the record stop
/// after) runs one tick later instead.
pub(super) fn custom_command_tick(
    custom: &crate::patch::CustomCommand,
    window: &ClipWindow,
    frame_times: &[f32],
    demo_fps: f32,
) -> i32 {
    match custom.relation {
        CommandRelation::Before => {
            let mut t = find_tick_backwards(
                window.first_kill_frame,
                custom.offset,
                frame_times,
                demo_fps,
            );
            if t == window.speed_drop_tick
                || t == window.sound_flush_tick
                || t == window.record_start_tick
            {
                t += 1;
            }
            t
        }
        CommandRelation::After => {
            let mut t =
                find_tick_forwards(window.last_kill_frame, custom.offset, frame_times, demo_fps);
            if t == window.record_stop_tick {
                t += 1;
            }
            t
        }
    }
}

/// What to tell the player when a Scheduled Command's tick falls past the
/// demo's last frame, where the patcher cannot write it (#1192); `None` when
/// it lands inside the demo. The offset is the cause, not the highlights.
pub(super) fn past_demo_end_note(
    custom: &crate::patch::CustomCommand,
    target_tick: i32,
    window: &ClipWindow,
    streak: &CaptureStreak,
) -> Option<String> {
    if target_tick <= window.last_frame {
        return None;
    }
    let (how, anchor) = match custom.relation {
        CommandRelation::Before => ("before", window.first_kill_frame),
        CommandRelation::After => ("after", window.last_kill_frame),
    };
    Some(format!(
        "⚠️ **Scheduled command not written:** `{}` was set {:.1}s {} the highlight at frame {}, \
         which is past the end of `{}` (it ends at frame {}). The highlights match the file; \
         shorten the offset to have it run.",
        custom.command,
        custom.offset,
        how,
        anchor,
        std::path::Path::new(&streak.source_demo).display(),
        window.last_frame
    ))
}

/// Logs a note when a Scheduled Command at `target_tick` lands outside the
/// block's real-time window.
///
/// Playback runs at `host_framerate 0.05`, uncapped and with nothing drawn
/// (`r_norefresh 1`, #724), until the pre-roll drops it back to real time at
/// the speed drop, and resumes fast-forwarding once the post-roll ends. A
/// command landing outside that window still executes, but it executes while
/// the engine is racing through frames with its audio buffers in a bad state
/// — so anything about sound, timing or rendering does something other than
/// what it looks like it does, and nothing in the captured video explains why.
fn note_if_runs_during_fast_forward(
    config: &PatcherConfig,
    custom: &crate::patch::CustomCommand,
    target_tick: i32,
    window: &ClipWindow,
) {
    if !runs_during_fast_forward(
        target_tick,
        window.speed_drop_tick,
        window.post_roll_end_tick,
    ) {
        return;
    }
    let (where_, fix) = if target_tick < window.speed_drop_tick {
        (
            "before playback drops back to real time",
            format!(
                "keep the offset under the {:.1}s pre-roll, or raise the pre-roll",
                config.pre_roll_seconds
            ),
        )
    } else {
        (
            "after the post-roll ends and fast-forward resumes",
            format!(
                "keep the offset under the {:.1}s post-roll, or raise the post-roll",
                config.post_roll_seconds
            ),
        )
    };
    crate::log_markdown(&format!(
        "ℹ️ **Scheduled command runs during fast-forward** — `{}` is set {} {:.1}s \
         {} the highlight, which lands at tick {}, {}. Setting a cvar there is \
         usually fine — it simply takes effect early. It is worth checking only \
         for commands that depend on playback running at real speed: anything \
         touching sound, recording, or rendering, since the engine is at \
         `host_framerate {}`, drawing nothing, with its audio buffers unflushed. \
         To have it run at normal speed instead, {}.",
        custom.command,
        relation_label(&custom.relation).to_lowercase(),
        custom.offset,
        if matches!(custom.relation, CommandRelation::Before) {
            "before"
        } else {
            "after"
        },
        target_tick,
        where_,
        fast_forward_host_framerate(config),
        fix
    ));
}

/// What a chain job's schedule needs to know about the job itself.
pub(super) struct ChainJob<'a> {
    /// The patched demo's stem, `dodstudio_chain_NN`: its aliases' prefix.
    pub demo_name: &'a str,
    /// 0-based place in the batch, and the batch's chain job count.
    pub index: usize,
    pub total: usize,
    /// How many blocks the job records.
    pub clip_count: usize,
    pub demo_fps: f32,
    /// `ds_spec_target <n>` for an HLTV demo, which follows no one by itself
    /// (#247): every clip puts the camera on the highlight's player.
    pub spectate: Option<String>,
}

/// A chain job's scheduled commands as they are built.
///
/// Commands are kept in the order they are pushed, and [`Self::into_sorted`]
/// sorts by tick only, stably: commands sharing a tick run in push order, so
/// the order of the `push_` calls is part of the schedule.
pub(super) struct JobSchedule<'a> {
    config: &'a PatcherConfig,
    job: ChainJob<'a>,
    commands: Vec<(i32, String)>,
}

impl<'a> JobSchedule<'a> {
    /// Starts the schedule with what runs once the demo has loaded: the
    /// fast-forward towards clip 1, the camera on an HLTV demo's player, and
    /// the DEMO_START and first NEXT_CLIP markers.
    pub fn start(config: &'a PatcherConfig, job: ChainJob<'a>) -> Self {
        let mut schedule = JobSchedule {
            config,
            job,
            commands: Vec::new(),
        };
        let initial_delay_ticks = (config.initial_delay * schedule.job.demo_fps) as i32;
        schedule.push(initial_delay_ticks, "sys_fast_forward");
        schedule.push_spectate(initial_delay_ticks);

        // One marker per demo, at its very first scheduled tick — the engine's
        // own live signal that this demo has started playing. Read back by
        // `native/src/obs/log_tail.rs` as `MarkerKind::DemoStart`, carrying
        // (job_idx, total_jobs, clip_count) 1-based, and forwarded up as
        // `EngineEvent::DemoLoading` for the "loading demo N of M" OS
        // notification (issue #98).
        let demo_start = format!(
            "DEMO_START {} {} {}",
            schedule.job.index + 1,
            schedule.job.total,
            schedule.job.clip_count
        );
        schedule.push_echos(initial_delay_ticks, &demo_start);

        // Fast-forward towards clip 1 starts at the very same tick as
        // DEMO_START above — playback has nothing to do before it. Read back
        // as `MarkerKind::NextClip` for the "fast-forwarding to clip N of M"
        // OS notification (issue #98).
        schedule.push_next_clip(initial_delay_ticks, 1);
        schedule
    }

    fn push(&mut self, tick: i32, command: impl Into<String>) {
        self.commands.push((tick, command.into()));
    }

    /// `text` as `[dod-studio]` echoes, split to fit ConsoleCommand frames.
    fn push_echos(&mut self, tick: i32, text: &str) {
        self.commands.extend(build_safe_echos(tick, text));
    }

    /// First person on an HLTV demo's player (nothing on a POV demo).
    fn push_spectate(&mut self, tick: i32) {
        if let Some(target) = self.job.spectate.clone() {
            self.push(tick, IN_EYE);
            self.push(tick, target);
        }
    }

    /// The NEXT_CLIP marker announcing clip `clip_number` (1-based).
    fn push_next_clip(&mut self, tick: i32, clip_number: usize) {
        let text = format!(
            "NEXT_CLIP {} {} {} {}",
            self.job.index + 1,
            self.job.total,
            clip_number,
            self.job.clip_count
        );
        self.push_echos(tick, &text);
    }

    /// Every Scheduled Command for one block, each after its CUSTOM_CMD
    /// marker, logging a note for any that runs during fast-forward.
    ///
    /// One that lands past the demo's last frame is left out, with its marker,
    /// and logged: the patcher could not write it anyway (#1192).
    pub fn push_custom_commands(
        &mut self,
        window: &ClipWindow,
        frame_times: &[f32],
        streak: &CaptureStreak,
    ) {
        let config = self.config;
        for (idx, custom) in config.custom_commands.iter().enumerate() {
            let target_tick = custom_command_tick(custom, window, frame_times, self.job.demo_fps);
            if let Some(note) = past_demo_end_note(custom, target_tick, window, streak) {
                crate::log_markdown(&note);
                continue;
            }
            let marker = format!(
                "CUSTOM_CMD{}_{} - Tick {}",
                idx + 1,
                relation_label(&custom.relation),
                target_tick
            );
            self.push_echos(target_tick, &marker);
            note_if_runs_during_fast_forward(config, custom, target_tick, window);
            self.push(target_tick, custom.command.clone());
        }
    }

    /// Block `clip_index`'s stages, from the drop out of fast-forward to the
    /// post-roll's end, and after the job's last block the hand-over to the
    /// next demo (or the batch's end).
    ///
    /// `chained_to_previous` is the job's whole list ([`Blocks`]): a block
    /// chained to the one before never left normal speed, and one followed by
    /// a chained block never returns to fast-forward.
    pub fn push_clip_stages(
        &mut self,
        clip_index: usize,
        window: &ClipWindow,
        chained_to_previous: &[bool],
        ramps: &[kill_gaps::Ramp],
    ) {
        let i = clip_index;
        let demo_name = self.job.demo_name;
        let speed_drop = window.speed_drop_tick;
        let sound_flush = window.sound_flush_tick;
        let record_start = window.record_start_tick;
        let record_stop = window.record_stop_tick;
        let post_roll_end = window.post_roll_end_tick;

        // Stages 1 and 1.5 are the exit from fast-forward. A block chained
        // to the one before it never left normal speed, so there's nothing
        // to drop back to and no fast-forward-induced audio drift to flush.
        let resumes_from_fast_forward = !chained_to_previous.get(i).copied().unwrap_or(false);

        if resumes_from_fast_forward {
            // Back on the player as playback drops to real time: the
            // camera may have been moved while fast-forwarding.
            self.push_spectate(speed_drop);

            // At Speed Flush (Stage 1)
            self.push(speed_drop, "sys_normal_speed");
            self.push(speed_drop + 1, "sys_normal_speed");
            self.push(speed_drop + 2, "sys_normal_speed");
            self.push_echos(speed_drop, &format!("SPEED_FLUSH - Tick {}", speed_drop));

            // At Sound Flush (Stage 1.5)
            self.push(sound_flush, "sys_sound");
        }

        // OBS starts each take's recording on its AUDIO_SYNC
        // (`obs::TRIGGER_MARKER`) and hands out take folders in that
        // order, so a chained block needs one too, without the sound
        // flush, which would cut the sound playing on from the clip
        // before (#1169). It lands at least a second after that clip's
        // STOP_RECORD: chained blocks sit OBS's take separation apart.
        if resumes_from_fast_forward || self.config.capture_mode == crate::patch::CaptureMode::Obs {
            self.push_echos(sound_flush, &format!("AUDIO_SYNC - Tick {}", sound_flush));
        }

        // At Start Frame (Stage 2)
        self.push(record_start, format!("{}_route_{}", demo_name, i));
        self.push(record_start, "sys_record_start");
        self.push_echos(
            record_start,
            &format!("START_RECORD - Tick {}", record_start),
        );

        for ramp in ramps {
            self.push(ramp.on_tick, kill_gaps::RAMP_ON);
            self.push(ramp.off_tick, kill_gaps::RAMP_OFF);
        }

        // At End Frame (Stage 3)
        self.push(record_stop, "sys_record_stop");
        self.push_echos(record_stop, &format!("STOP_RECORD - Tick {}", record_stop));

        // At Post-Roll End (Stage 4).
        // Skipped when the next block starts too soon for the round trip to
        // fit — playback just stays at normal speed into it instead.
        let next_block_chained = chained_to_previous.get(i + 1).copied().unwrap_or(false);
        if !next_block_chained {
            self.push(post_roll_end, "sys_fast_forward");
            self.push_echos(
                post_roll_end,
                &format!("FAST_FORWARD - Tick {}", post_roll_end),
            );
            // `chained_to_previous.get(i + 1)` also returns the "false"
            // default when there simply is no block i+1 (the last block
            // in the demo) -- that's not a real next clip to announce, so
            // this needs its own bounds check rather than reusing
            // `!next_block_chained` alone.
            if i + 1 < self.job.clip_count {
                self.push_next_clip(post_roll_end, i + 2);
            }
        }

        if i == self.job.clip_count - 1 {
            self.push_hand_over(window);
        }
    }

    /// After the job's last block: its `<demo>_next` alias, which plays the
    /// next chain demo or ends the batch. The batch's last job announces
    /// BATCH_COMPLETE first and hands over once those echoes have run.
    fn push_hand_over(&mut self, window: &ClipWindow) {
        let post_roll_end = window.post_roll_end_tick;
        let next_alias = format!("{}_next", self.job.demo_name);
        // At Absolute EOF
        if self.job.index == self.job.total - 1 {
            let echos = build_safe_echos(post_roll_end, "BATCH_COMPLETE");
            let echos_len = echos.len() as i32;
            self.commands.extend(echos);
            let final_tick = if window.cut_at_demo_end {
                window.exit_frame
            } else {
                post_roll_end + echos_len
            };
            self.push(final_tick, next_alias);
        } else {
            let final_tick = if window.cut_at_demo_end {
                window.exit_frame
            } else {
                post_roll_end
            };
            self.push(final_tick, next_alias);
        }
    }

    /// A BREADCRUMB echo every [`crate::patch::BREADCRUMB_INTERVAL_TICKS`]
    /// from tick 0 to short of `total_demo_frames`.
    pub fn push_breadcrumbs(&mut self, total_demo_frames: i32) {
        let mut step = 0;
        while step < total_demo_frames {
            self.push(
                step,
                format!("echo \"[dod-studio] BREADCRUMB - Tick {}\"", step),
            );
            step += crate::patch::BREADCRUMB_INTERVAL_TICKS;
        }
    }

    /// The finished schedule, sorted by tick (stably, see [`JobSchedule`]).
    pub fn into_sorted(mut self) -> Vec<(i32, String)> {
        self.commands.sort_by_key(|(tick, _)| *tick);
        self.commands
    }
}
