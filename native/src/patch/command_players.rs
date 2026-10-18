// patch/command_players.rs
// Which clips a Scheduled Command runs around when it is limited to some
// players (a `cl_xhair_style` per teammate in a team movie). Pure, so both
// capture routes share it: `build_batch_queue` schedules a command only where
// this says, and the hook-run batch (`runtime_batch`) is built from that same
// schedule.

use crate::patch::types::{CaptureStreak, CommandPlayer};

/// Who a clip is of: a POV demo's highlights are its recorder's alone (#573),
/// an HLTV demo's are the player the camera follows (`spectated`). Either
/// way that is the highlight's own player, so the streak says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipPlayer<'a> {
    pub id: Option<&'a str>,
    pub name: Option<&'a str>,
}

impl<'a> ClipPlayer<'a> {
    pub fn of(streak: &'a CaptureStreak) -> Self {
        Self {
            id: non_empty(streak.steam_id.as_deref()),
            name: non_empty(streak.target_player.as_deref()),
        }
    }
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// Whether `entry` is the clip's player: by SteamID when both carry one (a
/// player can change name mid-event, #429), else by name, ignoring case.
pub fn is_player(entry: &CommandPlayer, clip: ClipPlayer<'_>) -> bool {
    if let (Some(want), Some(have)) = (non_empty(entry.id.as_deref()), clip.id) {
        return want == have;
    }
    match (non_empty(Some(&entry.name)), non_empty(clip.name)) {
        (Some(want), Some(have)) => want.to_lowercase() == have.to_lowercase(),
        _ => false,
    }
}

/// Whether a command limited to `players` runs for the clip. No players is
/// everyone.
pub fn runs_for(players: &[CommandPlayer], clip: ClipPlayer<'_>) -> bool {
    players.is_empty() || players.iter().any(|p| is_player(p, clip))
}

/// Where a Scheduled Command lands in one recording block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAnchor {
    /// The block's own bounds, as for an unlimited command: every clip in the
    /// block is one of its players.
    Block,
    /// Only some of the block's clips are: "Before" counts back from the first
    /// kill of the first of those, "After" on from the last kill of the last.
    Clips { first_frame: i32, last_frame: i32 },
    /// None of them are: the command is left out of this block.
    Skip,
}

/// The anchor for a command limited to `players` in a block holding `clips`,
/// each with the frames of its first and last kill.
///
/// A block holds several clips when their recordings overlap and were merged
/// into one take. `build_batch_queue` groups a batch by demo and player first,
/// so today a block is one player's; should one ever hold two players' clips,
/// each player's command still fires around that player's own clips rather
/// than the whole take, so the other player's footage keeps its own setting
/// until their command lands.
pub fn command_anchor<'a>(
    players: &[CommandPlayer],
    clips: impl IntoIterator<Item = (ClipPlayer<'a>, i32, i32)>,
) -> CommandAnchor {
    if players.is_empty() {
        return CommandAnchor::Block;
    }
    let mut all = true;
    let mut span: Option<(i32, i32)> = None;
    for (clip, first, last) in clips {
        if !runs_for(players, clip) {
            all = false;
            continue;
        }
        span = Some(match span {
            None => (first, last),
            Some((f, l)) => (f.min(first), l.max(last)),
        });
    }
    match span {
        None => CommandAnchor::Skip,
        Some(_) if all => CommandAnchor::Block,
        Some((first_frame, last_frame)) => CommandAnchor::Clips {
            first_frame,
            last_frame,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: Option<&str>, name: &str) -> CommandPlayer {
        CommandPlayer {
            id: id.map(str::to_string),
            name: name.to_string(),
        }
    }

    fn clip<'a>(id: Option<&'a str>, name: &'a str) -> ClipPlayer<'a> {
        ClipPlayer {
            id,
            name: Some(name),
        }
    }

    const A: &str = "76561197961831029";
    const B: &str = "76561197972575011";

    #[test]
    fn steam_id_decides_when_both_sides_have_one() {
        let entry = player(Some(A), "m00cat");
        assert!(is_player(&entry, clip(Some(A), "renamed")));
        // Same name, different SteamID: not the player.
        assert!(!is_player(&entry, clip(Some(B), "m00cat")));
    }

    #[test]
    fn name_decides_without_an_id_ignoring_case() {
        assert!(is_player(&player(None, "M00cat"), clip(Some(A), "m00cat ")));
        assert!(is_player(&player(Some(A), "m00cat"), clip(None, "M00CAT")));
        assert!(!is_player(&player(None, "m00cat"), clip(None, "Brain")));
        // An empty entry never matches a clip with no name.
        assert!(!is_player(
            &player(None, ""),
            ClipPlayer {
                id: None,
                name: None
            }
        ));
    }

    #[test]
    fn no_players_is_everyone() {
        assert!(runs_for(&[], clip(None, "anyone")));
        assert_eq!(
            command_anchor(&[], [(clip(None, "x"), 10, 20)]),
            CommandAnchor::Block
        );
    }

    #[test]
    fn a_block_of_only_other_players_is_skipped() {
        let players = [player(Some(A), "m00cat")];
        assert_eq!(
            command_anchor(&players, [(clip(Some(B), "Brain"), 10, 20)]),
            CommandAnchor::Skip
        );
    }

    #[test]
    fn a_block_of_only_the_players_clips_uses_the_block() {
        let players = [player(Some(A), "m00cat")];
        assert_eq!(
            command_anchor(
                &players,
                [
                    (clip(Some(A), "m00cat"), 10, 20),
                    (clip(Some(A), "m00cat"), 30, 40)
                ]
            ),
            CommandAnchor::Block
        );
    }

    #[test]
    fn a_merged_block_of_two_players_anchors_on_each_players_own_clips() {
        let a = [player(Some(A), "m00cat")];
        let b = [player(None, "brain")];
        let block = [
            (clip(Some(A), "m00cat"), 100, 150),
            (clip(Some(B), "Brain"), 140, 220),
            (clip(Some(A), "m00cat"), 200, 260),
        ];
        assert_eq!(
            command_anchor(&a, block),
            CommandAnchor::Clips {
                first_frame: 100,
                last_frame: 260
            }
        );
        assert_eq!(
            command_anchor(&b, block),
            CommandAnchor::Clips {
                first_frame: 140,
                last_frame: 220
            }
        );
    }

    #[test]
    fn a_pov_and_an_hltv_clip_are_both_the_highlights_player() {
        let mut pov = CaptureStreak {
            start_tick: 0,
            end_tick: 0,
            source_demo: "pov.dem".into(),
            target_player: Some("m00cat".into()),
            kill_count: 0,
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 3,
            kills: Vec::new(),
            start_index: 0,
            end_index: 0,
            total_demo_frames: 0,
            demo_fps: 100.0,
            viewdemo_times: Vec::new(),
            frame_times: Default::default(),
            status: Default::default(),
            match_start_tick: None,
            source_key: None,
            spectated: false,
            steam_id: Some(A.into()),
            recorded_this_demo: true,
        };
        assert_eq!(
            ClipPlayer::of(&pov),
            ClipPlayer {
                id: Some(A),
                name: Some("m00cat")
            }
        );
        pov.spectated = true;
        pov.recorded_this_demo = false;
        pov.steam_id = Some(String::new());
        assert_eq!(
            ClipPlayer::of(&pov),
            ClipPlayer {
                id: None,
                name: Some("m00cat")
            }
        );
    }

    #[test]
    fn a_command_from_before_the_field_runs_for_everyone() {
        let old: crate::patch::CustomCommand = serde_json::from_str(
            r#"{"command":"cl_xhair_style 1","offset":5.0,"relation":"Before"}"#,
        )
        .unwrap();
        assert!(old.players.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cl_xhair_style_is_allowed_as_a_scheduled_command() {
        use crate::patch::cfg_scan;
        let cmd = vec!["cl_xhair_style 2".to_string()];
        assert!(cfg_scan::banned_commands(&cmd).is_empty());
        assert!(cfg_scan::scheduled_banned_commands(&cmd).is_empty());
        assert!(cfg_scan::mid_demo_hazards(&cmd).is_empty());
        assert!(cfg_scan::demo_filtered_commands(&cmd).is_empty());
    }
}
