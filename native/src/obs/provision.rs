//! Auto-provisions and repairs dod-studio' own OBS profile/scene/sources.
//!
//! **Why a dedicated, dod-studio-owned profile and scene rather than letting
//! the user point at their own.** An earlier design had scene/profile pickers
//! — dropdowns over whatever the user already had — and switched into
//! whichever one was chosen for the batch. That still left the batch
//! rewriting *that* profile's canvas/output resolution and recording
//! directory out from under it, which is exactly the kind of clobbering
//! `cfg_scan.rs`'s "detect and warn, never write" policy exists to prevent
//! for the game's own `.cfg` files. A profile/scene dod-studio creates and
//! names itself sidesteps the whole problem: nothing it writes here can ever
//! touch a setting or source the user actually built for something else.
//!
//! **Re-verified and repaired on every connect, not created once.** Nothing
//! stops a user opening the dod-studio profile/scene in OBS's own UI and
//! poking around — a setting drifting back is corrected the next time this
//! runs rather than trusted to have stayed put.
//!
//! **The window target.** `HL_WINDOW` was read directly off a real, working
//! OBS scene collection (`%APPDATA%\obs-studio\basic\scenes\*.json`) rather
//! than guessed — the window class in particular (`SDL_app`) is not
//! discoverable from documentation. Game/Window Capture sources re-resolve
//! their target on every tick, which is why this can be set before hl.exe has
//! ever launched: it's the same mechanism that lets a source configured once
//! keep finding the game across every later relaunch.

use serde_json::json;

use super::client::{ObsClient, ObsError};
use crate::log_markdown;

/// The profile dod-studio owns. Never the user's own — see module docs.
pub const PROFILE_NAME: &str = "[DoD-Studio]";
/// The scene dod-studio owns, inside whatever scene collection is current.
/// Deliberately not a dedicated scene collection of its own — a collection is
/// the coarser "which show am I running" concept; adding one scene to
/// whatever the user already has open is the smaller, less disruptive move.
pub const SCENE_NAME: &str = "[DoD-Studio] Capture";
// Bracket-prefixed to match PROFILE_NAME/SCENE_NAME's convention, and to
// keep these from colliding with a source someone already made by hand —
// source names are global across a whole scene collection, not scoped to
// one scene, so an unprefixed "Game Capture" is exactly the kind of name a
// manual setup would already be using. ensure_scene_item below still covers
// a collision if one somehow happens anyway (reuses/re-places the existing
// source rather than failing), this just makes it far less likely to.
const GAME_CAPTURE_SOURCE: &str = "[DoD-Studio] Game Capture";
const GAME_AUDIO_SOURCE: &str = "[DoD-Studio] Game Audio";
/// `title:class:executable` — DoD 1.3's hl.exe window, confirmed empirically
/// (see module docs). If a future engine/launcher build changes any of the
/// three, this stops matching and needs re-capturing the same way.
const HL_WINDOW: &str = "Day of Defeat:SDL_app:hl.exe";

/// What was active before provisioning switched it, so a caller running a
/// real batch (as opposed to just checking the connection) can restore it
/// once the batch ends. `None` means "already on it" — nothing to restore.
pub struct ProvisionResult {
    pub previous_profile: Option<String>,
    pub previous_scene: Option<String>,
}

/// Ensures the dod-studio-owned profile, scene, sources, video settings and
/// mute state all exist and are correct — creating anything missing,
/// repairing anything drifted — then switches into the profile and scene.
///
/// `width`/`height`/`fps_num`/`fps_den` are the game's own capture
/// resolution and the OBS-mode output rate (the same "Capture FPS" field
/// frame-sequence mode uses) — canvas and output both get set to the game's
/// resolution, avoiding the double-scale `ObsClient::preflight` otherwise
/// warns about.
///
/// Callers must call `ObsClient::refuse_if_busy` first — this function
/// assumes that's already been checked and switches live state
/// unconditionally.
pub fn ensure_dod_studio_setup(
    client: &mut ObsClient,
    width: i32,
    height: i32,
    fps_num: i32,
    fps_den: i32,
) -> Result<ProvisionResult, ObsError> {
    let previous_profile = ensure_profile(client)?;
    // Before the video settings: rebuilding the outputs (see
    // ensure_recording_settings) switches profiles, which reloads the
    // profile's own canvas and frame rate. Best-effort: OBS still records if
    // these can't be set, just at whatever quality the profile was left on.
    if let Err(e) = ensure_recording_settings(client, previous_profile.as_deref()) {
        log_markdown(&format!(
            "⚠️ **OBS** — couldn't set the `{PROFILE_NAME}` profile's recording quality ({e}). \
             Clips record at whatever OBS's Settings → Output has for it."
        ));
        // A failure between the rebuild's two switches leaves OBS on the
        // other profile, often the user's own, and everything below writes to
        // whichever profile is current.
        ensure_on_own_profile(client)?;
    }
    client.set_video_settings(width, height, fps_num, fps_den)?;
    let previous_scene = ensure_scene_and_sources(client, width, height)?;
    // Best-effort: muting a renamed or removed default input should not fail
    // provisioning over — see set_input_mute's own doc comment.
    let _ = client.set_input_mute("Desktop Audio", true);
    let _ = client.set_input_mute("Mic/Aux", true);
    Ok(ProvisionResult {
        previous_profile,
        previous_scene,
    })
}

fn ensure_profile(client: &mut ObsClient) -> Result<Option<String>, ObsError> {
    let (profiles, current) = client.profile_list()?;
    if !profiles.iter().any(|p| p == PROFILE_NAME) {
        client.create_profile(PROFILE_NAME)?;
        log_markdown(&format!(
            "🎬 **OBS** — created the `{PROFILE_NAME}` profile (first run on this OBS install)."
        ));
    }
    if current == PROFILE_NAME {
        return Ok(None);
    }
    client.set_profile(PROFILE_NAME)?;
    log_markdown(&format!(
        "🎬 **OBS** — switched to profile `{PROFILE_NAME}` (restored afterwards)."
    ));
    Ok(Some(current))
}

/// Switches back to `PROFILE_NAME` if OBS is on any other profile, and fails
/// if it can't: provisioning must never write the user's own profile.
fn ensure_on_own_profile(client: &mut ObsClient) -> Result<(), ObsError> {
    let (_, current) = client.profile_list()?;
    if current != PROFILE_NAME {
        client.set_profile(PROFILE_NAME)?;
    }
    Ok(())
}

/// The recording settings DoD Studio's profile records with (#645), as
/// `(category, name, value)` for `SetProfileParameter`.
///
/// - **Simple output mode**, because its settings live in the profile's
///   `basic.ini`, which obs-websocket can write. Advanced mode keeps the
///   encoder's settings in `recordEncoder.json`, which it can't.
/// - **"Indistinguishable quality"** (`HQ`): constant QP (16 on NVENC at
///   2560x1440, read from OBS 32.2.2's log), about 390 Mbps for a 120 fps
///   clip. A new profile records "Same as stream" instead: 6 Mbps CBR, a
///   sixtieth of that for the same clip.
/// - **The encoder OBS picked for this PC** (`default_encoder`, OBS's own
///   default for a new profile): the hardware encoder when there is one.
///   Setting it back undoes a change to x264, which can't keep up with a
///   high-rate capture on most CPUs.
/// - **Hybrid MP4** where OBS has it (30.2 and later): a file OBS was killed
///   while writing still plays to the moment it died, and the finished file is
///   a plain MP4 every editor opens. Older OBS keeps whatever it had.
/// - **No replay buffer**: it would hold the recording output.
fn recording_settings(
    obs_version: &str,
    default_encoder: &str,
) -> Vec<(&'static str, &'static str, String)> {
    let mut wanted = vec![
        ("Output", "Mode", "Simple".to_string()),
        ("SimpleOutput", "RecQuality", "HQ".to_string()),
        ("SimpleOutput", "RecRB", "false".to_string()),
    ];
    if !default_encoder.is_empty() {
        wanted.push(("SimpleOutput", "RecEncoder", default_encoder.to_string()));
    }
    if version_at_least(obs_version, (30, 2)) {
        wanted.push(("SimpleOutput", "RecFormat2", "hybrid_mp4".to_string()));
    }
    wanted
}

/// Whether `version` (`32.2.2`, `30.2.0-beta1`) is at least `min`.
fn version_at_least(version: &str, min: (u32, u32)) -> bool {
    let mut parts = version
        .split(|c: char| !c.is_ascii_digit())
        .map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    (major, minor) >= min
}

/// Puts the profile's recording settings to [`recording_settings`], on the
/// profile that is current (ours: called right after `ensure_profile`).
///
/// OBS reads them only when it rebuilds its outputs, and a write through
/// obs-websocket doesn't rebuild them: measured on OBS 32.2.2, changing the
/// quality preset that way made the next `StartRecord` fail with an error box
/// in OBS. A profile switch rebuilds them, so after any change this switches
/// to another profile and straight back. Nothing is written when everything
/// already matches, which is every connect after the first.
fn ensure_recording_settings(
    client: &mut ObsClient,
    previous_profile: Option<&str>,
) -> Result<(), ObsError> {
    let version = client.obs_version()?;
    let (_, default_encoder) = client.profile_parameter("SimpleOutput", "RecEncoder")?;
    let mut changed = Vec::new();
    for (category, name, value) in recording_settings(&version, &default_encoder) {
        let (current, default) = client.profile_parameter(category, name)?;
        let effective = if current.is_empty() { default } else { current };
        if effective != value {
            client.set_profile_parameter(category, name, &value)?;
            changed.push(format!("{name} {effective} → {value}"));
        }
    }
    if changed.is_empty() {
        return Ok(());
    }
    let (profiles, _) = client.profile_list()?;
    let other = previous_profile
        .map(str::to_string)
        .or_else(|| profiles.into_iter().find(|p| p != PROFILE_NAME));
    match other {
        Some(other) => {
            client.set_profile(&other)?;
            client.set_profile(PROFILE_NAME)?;
        }
        None => log_markdown(&format!(
            "⚠️ **OBS** — `{PROFILE_NAME}` is OBS's only profile, so its new recording settings \
             apply from OBS's next start."
        )),
    }
    log_markdown(&format!(
        "🎬 **OBS** — set the `{PROFILE_NAME}` profile's recording settings: {}.",
        changed.join(", ")
    ));
    Ok(())
}

fn ensure_scene_and_sources(
    client: &mut ObsClient,
    width: i32,
    height: i32,
) -> Result<Option<String>, ObsError> {
    let scenes = client.scene_names()?;
    if !scenes.iter().any(|s| s == SCENE_NAME) {
        client.create_scene(SCENE_NAME)?;
        log_markdown(&format!(
            "🎬 **OBS** — created the `{SCENE_NAME}` scene (first run in this scene collection)."
        ));
    }

    ensure_game_capture_source(client, width, height)?;
    ensure_game_audio_source(client)?;

    let current = client.current_scene()?;
    if current == SCENE_NAME {
        return Ok(None);
    }
    client.set_scene(SCENE_NAME)?;
    log_markdown(&format!(
        "🎬 **OBS** — switched to scene `{SCENE_NAME}` (restored afterwards)."
    ));
    Ok(Some(current))
}

fn ensure_game_capture_source(
    client: &mut ObsClient,
    width: i32,
    height: i32,
) -> Result<(), ObsError> {
    let settings = json!({
        "window": HL_WINDOW,
        "capture_mode": "window",
        "priority": 0,
        "anti_cheat_hook": false,
    });
    let existed = client
        .input_names()?
        .iter()
        .any(|i| i == GAME_CAPTURE_SOURCE);
    if existed {
        client.set_input_settings(GAME_CAPTURE_SOURCE, settings)?;
    } else {
        client.create_input(SCENE_NAME, GAME_CAPTURE_SOURCE, "game_capture", settings)?;
    }
    ensure_scene_item(client, GAME_CAPTURE_SOURCE, existed)?;
    // Best-effort: a canvas already matching the game's resolution (set just
    // above, in ensure_dod_studio_setup) makes this a no-op in practice, and a
    // source that's now in the scene is worth more than failing provisioning
    // over its bounding box specifically.
    if let Ok(item_id) = client.scene_item_id(SCENE_NAME, GAME_CAPTURE_SOURCE) {
        let _ = client.set_scene_item_bounds(SCENE_NAME, item_id, width as f64, height as f64);
    }
    Ok(())
}

fn ensure_game_audio_source(client: &mut ObsClient) -> Result<(), ObsError> {
    let settings = json!({
        "window": HL_WINDOW,
        "priority": 0,
    });
    let existed = client.input_names()?.iter().any(|i| i == GAME_AUDIO_SOURCE);
    if existed {
        client.set_input_settings(GAME_AUDIO_SOURCE, settings)?;
    } else {
        client.create_input(
            SCENE_NAME,
            GAME_AUDIO_SOURCE,
            "wasapi_process_output_capture",
            settings,
        )?;
    }
    ensure_scene_item(client, GAME_AUDIO_SOURCE, existed)?;
    // Best-effort, like the bounds above: a clip a quarter second out of sync
    // still beats no clip.
    let _ = client.set_input_audio_sync_offset(GAME_AUDIO_SOURCE, super::GAME_AUDIO_SYNC_OFFSET_MS);
    Ok(())
}

/// Makes sure `source` is actually placed in `SCENE_NAME` — only relevant
/// when the input `existed` already (inputs are global objects in
/// obs-websocket's model, so `create_input` above never ran for one, and
/// never added it to *this* scene). Confirmed necessary, not theoretical: an
/// input matching one of these exact names from an earlier manual setup
/// left `[DoD-Studio] Capture` created with no sources in it at all, since
/// `set_input_settings` repairs a global input's settings but places it
/// nowhere.
fn ensure_scene_item(client: &mut ObsClient, source: &str, existed: bool) -> Result<(), ObsError> {
    if !existed {
        return Ok(()); // create_input already added it to SCENE_NAME
    }
    if client.scene_item_id(SCENE_NAME, source).is_ok() {
        return Ok(()); // already placed here, nothing to do
    }
    client.create_scene_item(SCENE_NAME, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obs::fake_obs::{FakeObs, Reply, serve};
    use std::sync::{Arc, Mutex};

    /// An OBS on the user's own profile, with ours recording "Same as
    /// stream", whose switch back to ours fails `failed_returns` times. Gives
    /// the profile each `SetVideoSettings` landed on.
    fn obs_whose_switch_back_fails(failed_returns: usize) -> (FakeObs, Arc<Mutex<Vec<String>>>) {
        let video_writes = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::clone(&video_writes);
        let mut current = "Mine".to_string();
        let mut switches_to_ours = 0;
        let obs = serve(move |kind, data| match kind {
            "GetProfileList" => Reply::ok(json!({
                "profiles": ["Mine", PROFILE_NAME],
                "currentProfileName": current,
            })),
            "SetCurrentProfile" => {
                let to = data["profileName"].as_str().unwrap_or_default();
                if to == PROFILE_NAME {
                    switches_to_ours += 1;
                    // The first is ensure_profile's; the rest come back from
                    // the rebuild's switch away.
                    if switches_to_ours > 1 && switches_to_ours - 1 <= failed_returns {
                        return Reply::err("profile switch failed");
                    }
                }
                current = to.to_string();
                Reply::ok(json!({}))
            }
            "GetVersion" => Reply::ok(json!({ "obsVersion": "32.2.2" })),
            "GetProfileParameter" => {
                let value = match data["parameterName"].as_str() {
                    Some("RecQuality") => "Stream",
                    _ => "",
                };
                Reply::ok(json!({ "parameterValue": value, "defaultParameterValue": "" }))
            }
            "SetVideoSettings" => {
                writes.lock().unwrap().push(current.clone());
                Reply::ok(json!({}))
            }
            _ => Reply::ok(json!({})),
        });
        (obs, video_writes)
    }

    /// The quality rebuild switches to the user's profile and back; when the
    /// way back fails, the video settings must still land on ours.
    #[test]
    fn a_failed_switch_back_never_writes_the_users_profile() {
        let (obs, video_writes) = obs_whose_switch_back_fails(1);
        let mut client = ObsClient::connect(&obs.url, "").expect("connect to the fake");
        let result = ensure_dod_studio_setup(&mut client, 2560, 1440, 120, 1).expect("provisions");
        assert_eq!(result.previous_profile.as_deref(), Some("Mine"));
        assert_eq!(
            *video_writes.lock().unwrap(),
            vec![PROFILE_NAME.to_string()]
        );

        let (obs, video_writes) = obs_whose_switch_back_fails(usize::MAX);
        let mut client = ObsClient::connect(&obs.url, "").expect("connect to the fake");
        assert!(ensure_dod_studio_setup(&mut client, 2560, 1440, 120, 1).is_err());
        assert!(video_writes.lock().unwrap().is_empty());
    }

    /// The settings that changed are written, then OBS switches away and
    /// back so it rebuilds its outputs with them.
    #[test]
    fn changed_settings_are_written_and_applied_by_a_round_trip() {
        let (obs, video_writes) = obs_whose_switch_back_fails(0);
        let mut client = ObsClient::connect(&obs.url, "").expect("connect to the fake");
        ensure_dod_studio_setup(&mut client, 2560, 1440, 120, 1).expect("provisions");
        assert_eq!(
            *video_writes.lock().unwrap(),
            vec![PROFILE_NAME.to_string()]
        );
        assert_eq!(obs.sent("SetCurrentProfile"), 3);
        assert!(obs.sent("SetProfileParameter") >= 1);
        assert_eq!(obs.sent("SetInputAudioSyncOffset"), 1);
    }

    fn value<'a>(wanted: &'a [(&str, &str, String)], name: &str) -> Option<&'a str> {
        wanted
            .iter()
            .find(|(_, n, _)| *n == name)
            .map(|(_, _, v)| v.as_str())
    }

    /// "Same as stream" (a new profile's default) is 6 Mbps CBR: the profile
    /// must ask for the constant-quality preset instead, in Simple mode,
    /// whose settings obs-websocket can write.
    #[test]
    fn records_at_constant_quality_in_simple_mode() {
        let wanted = recording_settings("32.2.2", "nvenc");
        assert_eq!(value(&wanted, "Mode"), Some("Simple"));
        assert_eq!(value(&wanted, "RecQuality"), Some("HQ"));
        assert_eq!(value(&wanted, "RecRB"), Some("false"));
    }

    /// The encoder is whatever OBS picked for this PC, never one named here.
    #[test]
    fn keeps_the_encoder_obs_picked_for_this_pc() {
        let amd = recording_settings("32.2.2", "amd");
        assert_eq!(value(&amd, "RecEncoder"), Some("amd"));
        let unknown = recording_settings("32.2.2", "");
        assert_eq!(value(&unknown, "RecEncoder"), None);
    }

    /// Hybrid MP4 arrived in OBS 30.2; an older OBS keeps its container.
    #[test]
    fn hybrid_mp4_only_where_obs_has_it() {
        assert_eq!(
            value(&recording_settings("30.2.0", "nvenc"), "RecFormat2"),
            Some("hybrid_mp4")
        );
        assert_eq!(
            value(&recording_settings("31.0.0-beta1", "nvenc"), "RecFormat2"),
            Some("hybrid_mp4")
        );
        assert_eq!(
            value(&recording_settings("30.1.2", "nvenc"), "RecFormat2"),
            None
        );
        assert_eq!(
            value(&recording_settings("29.1.3", "nvenc"), "RecFormat2"),
            None
        );
        assert_eq!(value(&recording_settings("?", "nvenc"), "RecFormat2"), None);
    }
}
