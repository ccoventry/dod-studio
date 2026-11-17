use super::*;

#[test]
fn cache_and_builder_lines_match_their_original_inline_text() {
    assert_eq!(
        cache_entries_upgraded(3, 5, ""),
        "Cache demos: upgraded 3 of 5 older cache entries"
    );
    assert_eq!(
        cache_entries_upgraded(
            3,
            5,
            &cache_parsing_the_rest(&["2: a".into(), "1: b".into()])
        ),
        "Cache demos: upgraded 3 of 5 older cache entries; parsing the rest (2: a, 1: b)"
    );
    assert_eq!(
        cache_could_not_analyse("a.dem", "bad"),
        "Cache demos: could not analyse a.dem: bad"
    );
    assert_eq!(
        direct_to_video_capture("-c:v x", "video.mkv"),
        "🎬 **Direct-to-video capture** — HLAE pipes frames to FFmpeg (`-c:v x`) instead of writing a BMP sequence, one `video.mkv` per stream folder. Render Studio reads these the same way it reads frame sequences."
    );
    assert_eq!(
        agr_capture(60),
        "🦴 **AGR capture** — HLAE records no video. Each clip is saved as one `.agr` file (models and camera, for the Blender page) at 60 fps, in its take folder, with the game's sound for the same stretch beside it as a `.wav` of the same name (HLAE's movie recorder, its picture sent to FFmpeg's null muxer)."
    );
}

#[test]
fn scanner_and_hook_copy_errors_match_their_original_inline_text() {
    assert_eq!(
        scanner_alignment_lost(9, 4),
        "Scanner alignment lost! Read impossible packet size: 9 bytes at pos 4"
    );
    assert_eq!(scan_failed_to_parse_demo("x"), "Failed to parse demo: x");
    assert_eq!(scan_failed_to_read_file("x"), "Failed to read file: x");
    assert_eq!(
        could_not_read_hook_dll("a.dll", "x"),
        "could not read the hook DLL a.dll: x"
    );
    assert_eq!(hook_dll_names_no_file("a"), "a names no file");
    assert_eq!(
        could_not_make_hook_folder("d", "x"),
        "could not make the folder d: x"
    );
    assert_eq!(
        could_not_copy_hook_dll("c", "x"),
        "could not copy the hook DLL to c: x"
    );
    assert_eq!(
        could_not_write_hook_source("src.txt", "d", "x"),
        "could not write src.txt in d: x"
    );
}

#[test]
fn no_killstreaks_names_the_player_the_ask_and_the_longest_streak() {
    assert_eq!(
        cli_no_killstreaks("Pvt. Clopton", 3, Some("k98"), 2),
        "Pvt. Clopton has no killstreak of 3 or more kills with a weapon matching 'k98' \
             in this demo (longest streak: 2), so there is nothing to patch. \
             Lower --min-streak to patch a shorter one."
    );
}

#[test]
fn map_fetch_messages_match_their_original_inline_text() {
    assert_eq!(
        not_a_usable_map_name("bad name"),
        format!("`{}` is not a usable map name", "bad name")
    );
    assert_eq!(MAP_MIRRORS_MUST_BE_HTTPS, "map mirrors must be https");
    assert_eq!(
        labeled("some/path", "boom"),
        format!("{}: {}", "some/path", "boom")
    );
    assert_eq!(
        served_unreadable_bsp("http://x", "bad bsp"),
        format!(
            "what {} served is not a readable BSP: {}",
            "http://x", "bad bsp"
        )
    );
    assert_eq!(
        served_unparseable_map("http://x", "bad map"),
        format!(
            "what {} served does not parse as a map: {}",
            "http://x", "bad map"
        )
    );
    assert_eq!(
        served_wrong_build("http://x", 0x1234, 0x5678),
        format!(
            "{} served build {:08x}, but the demo needs {:08x} — not installing it",
            "http://x", 0x1234u32, 0x5678u32
        )
    );
    assert_eq!(
        could_not_move_existing_aside("old.bsp", "in use"),
        format!(
            "could not move the existing {} aside: {}",
            "old.bsp", "in use"
        )
    );
    assert_eq!(
        url_returned_status("http://x", 404),
        format!("{} returned {}", "http://x", 404)
    );
}

#[test]
fn hlae_ffmpeg_messages_match_their_original_inline_text() {
    assert_eq!(
        not_a_file("ffmpeg.exe"),
        format!("{} is not a file", "ffmpeg.exe")
    );
    assert_eq!(
        could_not_run("ffmpeg.exe", "access denied"),
        format!("could not run {}: {}", "ffmpeg.exe", "access denied")
    );
    assert_eq!(
        did_not_respond_to_version_flag("ffmpeg.exe"),
        format!("{} did not respond to -version", "ffmpeg.exe")
    );
    assert_eq!(
        did_not_identify_as_ffmpeg("ffmpeg.exe"),
        format!("{} did not identify itself as FFmpeg", "ffmpeg.exe")
    );
    assert_eq!(
        reports_itself_as("ffmpeg.exe", "handbrake version 1.0"),
        format!(
            "{} is not FFmpeg — it reports itself as \"{}\"",
            "ffmpeg.exe", "handbrake version 1.0"
        )
    );
}

#[test]
fn bin_messages_match_their_original_inline_text() {
    assert_eq!(
        cfgs_setting_row("r_decals", 9, "movie.cfg", 1),
        "  r_decals     9        movie.cfg:1"
    );
    assert_eq!(
        cfgs_fatal_row("cl_x", 1, 0, "a.cfg", 3),
        "  cl_x         1        DoD requires 0 (a.cfg:3)"
    );
    assert_eq!(cfgs_value_from_init(3), "3 (init)");
    assert_eq!(
        maps_to_fetch_row("dod_anzio", 0x801b771f, 1, "u"),
        "  dod_anzio                build 801b771f, 1 demo(s)  u"
    );
    assert_eq!(
        maps_installed("m", 0xab, 1.25, ""),
        "  m                        installed 000000ab (1.2 MB)"
    );
    assert_eq!(
        preview_roster_row(2, 4, 3, "x"),
        "      2    4 kills in  3 streaks   x"
    );
    assert_eq!(
        preview_batch_complete(1, 2),
        "\nBatch Complete\n  Processed: 1  |  Skipped: 2"
    );
    assert_eq!(
        cli_selected_streak(3, 1.0, 2.5),
        "Selected Streak: 3 kills starting at 1.00s, ending at 2.50s"
    );
    assert_eq!(cli_error("boom"), "Error: boom");
}

#[test]
fn obs_session_messages_match_their_original_inline_text() {
    assert_eq!(NOT_CONNECTED_TO_OBS, "not connected to OBS");
    assert_eq!(
        obs_reported_file_missing("clip.mp4"),
        format!("OBS reported {} but no file is there", "clip.mp4")
    );
    let recorded = "clip.mp4";
    let target = "video.mp4";
    let retry_secs = 5.0_f64;
    let last = "sharing violation";
    assert_eq!(
        could_not_move_after_retries(recorded, target, retry_secs, last),
        format!("could not move {recorded} to {target} after 5.0 s of retries: {last}")
    );
}

#[test]
fn obs_log_lines_match_their_original_inline_text() {
    assert_eq!(
        obs_profile_switched("[DoD-Studio]"),
        "🎬 **OBS** — switched to profile `[DoD-Studio]` (restored afterwards)."
    );
    assert_eq!(
        obs_started_late(120, "C:/takes/a"),
        "⚠️ **OBS** — started recording 120 ms after the clip's first frame, so `C:/takes/a` is \
             missing the clip's opening; it won't count as captured."
    );
    assert_eq!(
        obs_frames_dropped(5, 20, 25.0, "the recorded clip is visibly choppy"),
        "⚠️ **OBS** — this block dropped 5/20 frames (25.0%) due to encoding lag: the recorded \
             clip is visibly choppy. The encoder can't keep up with OBS Capture FPS on this PC; \
             lower it (Configuration → Output Format), or the resolution."
    );
}

#[test]
fn decal_flush_log_lines_match_their_original_inline_text() {
    assert_eq!(
        decal_flush_short_burst(10, 3, 8),
        "⚠️ **Short decal burst** before frame 10 — placed 3 of 8. That clip is not \
             guaranteed to start clean."
    );
    assert_eq!(
        decal_flush_skipped_unreadable("a.dem", "gone"),
        "⚠️ **Decal flush skipped** — could not read `a.dem`: gone"
    );
    assert_eq!(
        decal_flush_spots_close_to_camera(12.4, 40.0),
        "ℹ️ **Decal flush spots are closer to the camera than preferred** — nearest \
             approach 12 units against a 40-unit preference. The surface each one is \
             drawn on cleared the line-of-sight test, so none should be in shot; this is the \
             margin narrowing, not a decal on screen."
    );
}

#[test]
fn crosshair_and_overview_errors_match_their_original_inline_text() {
    assert_eq!(
        crosshair_zip_many(2, "a.spr, b.spr"),
        "the ZIP holds 2 sprites (a.spr, b.spr): unzip it and add the one you want"
    );
    assert_eq!(
        crosshair_name_taken("customxhair-dot.spr"),
        "there's already a crosshair called customxhair-dot.spr: pick another name"
    );
    assert_eq!(
        crosshair_source_too_big("C:/x/a.zip", 9),
        "C:/x/a.zip is 9 MB, too big to be a crosshair file"
    );
    assert_eq!(path_failed("C:/x/a.txt", "denied"), "C:/x/a.txt: denied");
    assert_eq!(
        overview_drawing_incomplete("bad"),
        "the drawing did not arrive whole: bad"
    );
}

#[test]
fn native_log_lines_match_their_original_inline_text() {
    assert_eq!(
        demo_buffer_eof("Frame Header", 8, 100),
        "Buffer Error [Frame Header]: Expected 8 bytes at offset 100, but hit EOF."
    );
    assert_eq!(
        decal_atlas_not_saved("write", "a.json.tmp", "denied"),
        "⚠️ **Decal atlas not saved** — could not write `a.json.tmp`: denied"
    );
    assert_eq!(
        agr_sound_placed(2, 3),
        "🦴 **AGR sound** — 2 of 3 `.agr` file(s) have their sound beside them as a \
             `.wav` of the same name."
    );
    assert_eq!(
        pre_roll_too_short(1.0_f32, 2.5_f32, "the mixer"),
        "⚠️ **Pre-roll is shorter than this capture needs** — 1.0s, against a 2.5s floor \
             set by the mixer. Playback returns to real time 1.0s before recording, which is not enough \
             for it: the engine's audio buffers are left unflushed by the fast-forward. Raise the \
             pre-roll to at least 2.5s."
    );
    assert_eq!(
        decal_flush_map_other_build("dod_x", 0xab, 0xcd),
        "⚠️ **Decal flush is ignoring `dod_x`** — the map here is a different build than \
             the demo was recorded on (demo wants `000000ab`, this is `000000cd`). Its \
             geometry describes a different world, so it cannot be trusted to say what a \
             camera can see. Falling back to the frame cone alone."
    );
    assert_eq!(
        review_from_game("a.dem", &Some(5)),
        "Review from the game: a.dem (Some(5))"
    );
}
