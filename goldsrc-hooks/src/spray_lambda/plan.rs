//! Which lambdas in a POV demo are the recorder's own (#1322), read out of
//! the `.dem` ahead of playback.
//!
//! The server draws a lambda with no player attached, so the only witness is
//! the recorder's view: the spray can traces 128 units along the aim, so a
//! lambda the recorder sprayed lands in front of them, and the frames say
//! where that was. Reading the file rather than the game's own frames has
//! two gains. The view history doesn't depend on which frames the game drew
//! (a seek replays messages without drawing them), and the rule's tie-break
//! knows about sprays still to come ([`Plan::verdict`]).
//!
//! The rule, measured on 42 demos and 636 lambdas (6 WSOD25 POV players,
//! their HLTV demos and solifotw-pb2):
//!
//! - **Look back [`LOOK_BACK_SECONDS`].** The lambda reaches the demo about
//!   0.1 s after the key and the player may flick away meanwhile: m00cat's
//!   20:53:93 one is 25.8 degrees off his view on arrival and 3.7 before.
//! - **Inside [`AIM_CONE_DEGREES`] and [`REACH_UNITS`]** at some moment of
//!   that look-back. His 27:27:35 one is 6.3 degrees off at 104 units.
//! - **Never a player who sprays for real.** A player with a working spray
//!   sprays `TE_PLAYERDECAL`s, not lambdas. Without this, stealthlol takes
//!   m00cat's lambda in `wsod25_grp4_h1_stealth.dem`: he stands beside him,
//!   93 units away and 6.4 degrees off. The other 39 demos had no false
//!   match. The player byte is the entity index, one more than the slot.

use std::collections::VecDeque;
use std::io::{Read, Seek};

use dem::types::{
    Aux, EngineMessage, FrameData, MessageData, NetMessage, NetworkMessage, TempEntity,
};

use crate::lookahead::frames::Frames;
use crate::lookahead::timeline::ReadError;

/// The message's decal index for `}lambda06`. The decal pool's slot for it
/// changes by session, so only this index names it, and only in the message.
pub const LAMBDA_DECAL: i32 = 0x6a;
/// The spray can's 128-unit trace, and a margin: it starts at the eye of a
/// frame the demo doesn't always hold.
const REACH_UNITS: f32 = 136.0;
const AIM_CONE_DEGREES: f32 = 8.0;
const LOOK_BACK_SECONDS: f32 = 0.3;
/// Two positions this close are one decal: a coord is 1/8 of a unit on the
/// wire, and the game reads the file's value.
const SAME_SPOT_UNITS: f32 = 1.0;
/// A coord on the wire is an `i16` of this many to the unit.
const COORD_SCALE: f32 = 8.0;
/// `svc_time` going back more than this is a new map, not the HLTV clock's
/// millisecond step-backs (`timeline::TURNED_BACK_SECONDS`): `viewdemo` plays
/// only the first, and a slot's player may differ on the next.
const TURNED_BACK_SECONDS: f32 = 1.0;

pub type Vec3 = [f32; 3];

/// The recorder's view at one demo frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub time: f32,
    pub eye: Vec3,
    /// A unit vector along the aim.
    pub forward: Vec3,
}

fn delta(from: Vec3, to: Vec3) -> Vec3 {
    [to[0] - from[0], to[1] - from[1], to[2] - from[2]]
}

fn length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// How far `spot` is off `view`'s aim, in degrees. 180 with the eye on it.
fn degrees_off_aim(view: &View, spot: Vec3) -> f32 {
    let to = delta(view.eye, spot);
    let distance = length(to);
    let aim = length(view.forward);
    if distance == 0.0 || aim == 0.0 {
        return 180.0;
    }
    let dot = to[0] * view.forward[0] + to[1] * view.forward[1] + to[2] * view.forward[2];
    (dot / (distance * aim))
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// Whether a spray can in `view` could have put a decal on `spot`.
fn in_reach_along_aim(view: &View, spot: Vec3) -> bool {
    length(delta(view.eye, spot)) <= REACH_UNITS && degrees_off_aim(view, spot) <= AIM_CONE_DEGREES
}

/// Whether any of `views` from the [`LOOK_BACK_SECONDS`] up to `time` was
/// aimed at `spot`.
fn aimed_at<'a>(views: impl IntoIterator<Item = &'a View>, time: f32, spot: Vec3) -> bool {
    views.into_iter().any(|view| {
        view.time <= time && time - view.time <= LOOK_BACK_SECONDS && in_reach_along_aim(view, spot)
    })
}

/// What a demo's frames say about a lambda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Along the recorder's aim, and the recorder has no spray of their own.
    Recorders,
    /// Along the recorder's aim, but the recorder sprays for real, so it is
    /// someone beside them.
    RecorderSpraysForReal,
    /// Nowhere near the recorder's aim.
    NotAimedAt,
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Verdict::Recorders => "the recorder's",
            Verdict::RecorderSpraysForReal => "along the aim, but the recorder sprays for real",
            Verdict::NotAimedAt => "not along the recorder's aim",
        })
    }
}

/// What one POV demo says about its lambdas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Every lambda that landed along the recorder's aim.
    aimed: Vec<Vec3>,
    /// Bit `n`: entity `n` has a `TE_PLAYERDECAL` of their own in the demo.
    sprayers: u64,
    /// All lambdas in the demo.
    lambdas: usize,
}

impl Plan {
    /// What the demo says about the lambda at `spot`. `recorder` is the
    /// recorder's entity index, the player byte of their own sprays.
    pub fn verdict(&self, recorder: usize, spot: Vec3) -> Verdict {
        let aimed = self
            .aimed
            .iter()
            .any(|at| length(delta(*at, spot)) <= SAME_SPOT_UNITS);
        if !aimed {
            Verdict::NotAimedAt
        } else if recorder < 64 && self.sprayers & (1 << recorder) != 0 {
            Verdict::RecorderSpraysForReal
        } else {
            Verdict::Recorders
        }
    }

    /// One line on what it holds, for the hook log.
    pub fn summary(&self) -> String {
        format!(
            "{} lambdas, {} along the recorder's aim, {} players with sprays of their own",
            self.lambdas,
            self.aimed.len(),
            self.sprayers.count_ones()
        )
    }
}

/// A decal's position: three coords from `at` in a temp entity's bytes.
fn coords(bytes: &[u8], at: usize) -> Option<Vec3> {
    let coord = |i: usize| {
        let b = bytes.get(at + 2 * i..at + 2 * i + 2)?;
        Some(f32::from(i16::from_le_bytes([b[0], b[1]])) / COORD_SCALE)
    };
    Some([coord(0)?, coord(1)?, coord(2)?])
}

/// The plan as a demo's frames go by.
#[derive(Default)]
struct Reader {
    plan: Plan,
    /// The views of the last [`LOOK_BACK_SECONDS`].
    views: VecDeque<View>,
    last_svc: Option<f32>,
}

impl Reader {
    fn note_view(&mut self, view: View) {
        self.views.push_back(view);
        while self
            .views
            .front()
            .is_some_and(|old| view.time - old.time > LOOK_BACK_SECONDS)
        {
            self.views.pop_front();
        }
    }

    fn note_lambda(&mut self, time: f32, spot: Vec3) {
        self.plan.lambdas += 1;
        if aimed_at(&self.views, time, spot) {
            self.plan.aimed.push(spot);
        }
    }

    fn note_player_decal(&mut self, entity: u8) {
        if entity < 64 {
            self.plan.sprayers |= 1 << entity;
        }
    }

    fn temp_entity(&mut self, time: f32, entity: &TempEntity) {
        match entity {
            TempEntity::TeWorldDecal(bytes)
                if bytes.get(6).is_some_and(|i| i32::from(*i) == LAMBDA_DECAL) =>
            {
                if let Some(spot) = coords(bytes, 0) {
                    self.note_lambda(time, spot);
                }
            }
            TempEntity::TePlayerDecal(bytes) => {
                if let Some(player) = bytes.first() {
                    self.note_player_decal(*player);
                }
            }
            _ => {}
        }
    }

    /// One network frame; false once the clock has turned back.
    fn network_frame(&mut self, time: f32, message: &NetworkMessage) -> bool {
        let params = &message.info.refparams;
        if let (&[x, y, z], &[fx, fy, fz]) =
            (params.view_origin.as_slice(), params.forward.as_slice())
        {
            self.note_view(View {
                time,
                eye: [x, y, z],
                forward: [fx, fy, fz],
            });
        }
        let MessageData::Parsed(messages) = &message.messages else {
            return true;
        };
        for m in messages {
            let NetMessage::EngineMessage(em) = m else {
                continue;
            };
            match em.as_ref() {
                EngineMessage::SvcTime(t) if t.time.is_finite() => {
                    if self
                        .last_svc
                        .is_some_and(|l| t.time < l - TURNED_BACK_SECONDS)
                    {
                        return false;
                    }
                    self.last_svc = Some(t.time);
                }
                EngineMessage::SvcTempEntity(te) => self.temp_entity(time, &te.entity),
                _ => {}
            }
        }
        true
    }
}

/// Reads the POV demo in `file` into a [`Plan`], a frame at a time. Stops at
/// the first frame it can't read, or where the clock turns back, keeping what
/// it had. Asks `superseded` every network frame and gives up once it says
/// yes.
pub fn read<R: Read + Seek>(mut file: R, superseded: &dyn Fn() -> bool) -> Result<Plan, ReadError> {
    let directory = crate::demo_file::read_directory(&mut file).ok_or(ReadError::NotADemo)?;
    let mut frames = Frames::new(file);
    let aux = Aux::new2();
    let mut reader = Reader::default();
    'segments: for (offset, _) in crate::demo_file::segments(&directory) {
        if frames.seek(offset).is_err() {
            break;
        }
        loop {
            let Some(frame) = frames.next_frame(&aux) else {
                break 'segments;
            };
            match &frame.frame_data {
                FrameData::NextSection => break,
                FrameData::NetworkMessage(message) => {
                    if superseded() {
                        return Err(ReadError::Superseded);
                    }
                    if !reader.network_frame(frame.time, &message.1) {
                        break 'segments;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(reader.plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    const M00CAT: usize = 13;
    const STEALTHLOL: usize = 5;

    /// A view `degrees` off the line to `spot` from `units` away, looking
    /// along +x with the spot off to the side.
    fn view_off(time: f32, units: f32, degrees: f32) -> (View, Vec3) {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let view = View {
            time,
            eye: [0.0, 0.0, 0.0],
            forward: [1.0, 0.0, 0.0],
        };
        (view, [units * cos, units * sin, 0.0])
    }

    #[test]
    fn a_lambda_off_aim_on_arrival_counts_if_it_was_on_aim_just_before() {
        // m00cat's 20:53:93 lambda: 25.8 degrees off when it arrives, 3.7
        // in the 0.3 s before.
        let (before, spot) = view_off(1253.75, 120.0, 3.7);
        let arrival = View {
            time: 1253.93,
            forward: [
                25.8f32.to_radians().cos(),
                -(25.8f32 - 3.7).to_radians().sin(),
                0.0,
            ],
            ..before
        };
        assert!(!aimed_at([&arrival], 1253.93, spot));
        assert!(aimed_at([&before, &arrival], 1253.93, spot));
    }

    #[test]
    fn the_angle_is_measured_as_the_survey_measured_it() {
        let (view, spot) = view_off(0.0, 104.0, 6.3);
        assert!((degrees_off_aim(&view, spot) - 6.3).abs() < 0.01);
    }

    #[test]
    fn a_view_older_than_the_look_back_says_nothing() {
        let (view, spot) = view_off(10.0, 100.0, 1.0);
        assert!(aimed_at([&view], 10.29, spot));
        assert!(!aimed_at([&view], 10.31, spot));
    }

    #[test]
    fn a_view_after_the_lambda_says_nothing() {
        let (view, spot) = view_off(10.0, 100.0, 1.0);
        assert!(!aimed_at([&view], 9.99, spot));
    }

    #[test]
    fn past_the_cone_or_the_reach_it_is_not_aimed_at() {
        let (near, spot) = view_off(0.0, 100.0, 8.5);
        assert!(!aimed_at([&near], 0.0, spot));
        let (far, spot) = view_off(0.0, 140.0, 1.0);
        assert!(!aimed_at([&far], 0.0, spot));
        let (edge, spot) = view_off(0.0, 130.0, 7.5);
        assert!(aimed_at([&edge], 0.0, spot));
    }

    /// The reader over one lambda, with `view` the one the look-back holds.
    fn reader_with_lambda(view: View, spot: Vec3) -> Reader {
        let mut reader = Reader::default();
        reader.note_view(view);
        reader.note_lambda(view.time, spot);
        reader
    }

    #[test]
    fn a_recorder_with_no_spray_of_their_own_gets_the_lambda() {
        // m00cat's 27:27:35 lambda in his own demo: 104 units, 6.3 degrees.
        // The other players' sprays don't count against him.
        let (view, spot) = view_off(1647.35, 104.0, 6.3);
        let mut reader = reader_with_lambda(view, spot);
        reader.note_player_decal(5);
        reader.note_player_decal(12);
        assert_eq!(reader.plan.verdict(M00CAT, spot), Verdict::Recorders);
    }

    #[test]
    fn the_man_beside_the_sprayer_is_ruled_out_by_his_own_sprays() {
        // The same lambda in stealthlol's demo: as good a match as m00cat's
        // (93 units, 6.4 degrees), and his own sprays at 43.08, 504.81 and
        // 899.68 s say it isn't his.
        let (view, spot) = view_off(504.45, 93.0, 6.4);
        let mut reader = reader_with_lambda(view, spot);
        assert_eq!(reader.plan.verdict(STEALTHLOL, spot), Verdict::Recorders);
        reader.note_player_decal(STEALTHLOL as u8);
        assert_eq!(
            reader.plan.verdict(STEALTHLOL, spot),
            Verdict::RecorderSpraysForReal
        );
    }

    #[test]
    fn a_spray_later_in_the_demo_counts_as_much_as_one_before_the_lambda() {
        let (view, spot) = view_off(100.0, 93.0, 6.4);
        let mut reader = reader_with_lambda(view, spot);
        reader.note_view(View {
            time: 500.0,
            ..view
        });
        reader.note_player_decal(STEALTHLOL as u8);
        assert_eq!(
            reader.plan.verdict(STEALTHLOL, spot),
            Verdict::RecorderSpraysForReal
        );
    }

    #[test]
    fn a_lambda_nowhere_near_the_aim_is_nobodys() {
        let (view, spot) = view_off(0.0, 100.0, 40.0);
        let reader = reader_with_lambda(view, spot);
        assert_eq!(reader.plan.verdict(M00CAT, spot), Verdict::NotAimedAt);
    }

    #[test]
    fn a_lambda_elsewhere_is_not_the_one_the_plan_holds() {
        let (view, spot) = view_off(0.0, 100.0, 1.0);
        let reader = reader_with_lambda(view, spot);
        let elsewhere = [spot[0] + 5.0, spot[1], spot[2]];
        assert_eq!(reader.plan.verdict(M00CAT, elsewhere), Verdict::NotAimedAt);
    }

    #[test]
    fn old_views_leave_the_look_back() {
        let (view, _) = view_off(0.0, 100.0, 1.0);
        let mut reader = Reader::default();
        for i in 0..100 {
            reader.note_view(View {
                time: i as f32 * 0.1,
                ..view
            });
        }
        assert!(reader.views.len() <= 4, "{}", reader.views.len());
    }

    #[test]
    fn a_world_decal_is_read_from_its_bytes() {
        // (101.75, 510.0, -131.625), index 0x6a, the 27:27:35 lambda.
        let mut bytes = Vec::new();
        for coord in [814i16, 4080, -1053] {
            bytes.extend_from_slice(&coord.to_le_bytes());
        }
        bytes.push(0x6a);
        let spot = [101.75, 510.0, -131.625];
        assert_eq!(coords(&bytes, 0), Some(spot));
        let mut reader = Reader::default();
        let (mut view, _) = view_off(1.0, 0.0, 0.0);
        view.eye = [71.5, 610.4, -133.9];
        view.forward = [0.27, -0.96, 0.0];
        reader.note_view(view);
        reader.temp_entity(1.0, &TempEntity::TeWorldDecal(bytes.clone()));
        assert_eq!(reader.plan.lambdas, 1);
        assert_eq!(reader.plan.verdict(M00CAT, spot), Verdict::Recorders);
        // Another decal's index is no lambda.
        bytes[6] = 0x6b;
        reader.temp_entity(1.0, &TempEntity::TeWorldDecal(bytes));
        assert_eq!(reader.plan.lambdas, 1);
    }

    #[test]
    fn a_cut_off_message_is_skipped_not_a_panic() {
        let mut reader = Reader::default();
        reader.temp_entity(0.0, &TempEntity::TeWorldDecal(vec![1, 2, 3]));
        reader.temp_entity(0.0, &TempEntity::TePlayerDecal(Vec::new()));
        assert_eq!(reader.plan, Plan::default());
    }

    #[test]
    fn the_player_byte_past_the_mask_is_ignored() {
        let mut reader = Reader::default();
        reader.note_player_decal(200);
        assert_eq!(reader.plan.sprayers, 0);
    }

    #[test]
    fn the_ci_fixture_reads_whole() {
        let file = std::fs::File::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        ))
        .unwrap();
        let plan = read(file, &|| false).unwrap();
        // A real POV recording: the read gets through it, whatever it holds.
        assert!(plan.lambdas >= plan.aimed.len());
    }

    /// The demos the rule was measured on. `DOD_ANALYSIS_DEMOS` names a
    /// folder, else the PRE install's `dod` folder.
    fn survey_demo(name: &str) -> std::path::PathBuf {
        let folder = std::env::var_os("DOD_ANALYSIS_DEMOS").map_or_else(
            || {
                std::path::PathBuf::from(
                    "C:/Program Files (x86)/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/dod",
                )
            },
            std::path::PathBuf::from,
        );
        folder.join(name)
    }

    fn plan_of(name: &str) -> Plan {
        let path = survey_demo(name);
        let file = std::fs::File::open(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (set DOD_ANALYSIS_DEMOS)", path.display()));
        read(file, &|| false).unwrap()
    }

    #[test]
    #[ignore = "needs the wsod25_grp4_h1 demos (DOD_ANALYSIS_DEMOS)"]
    fn wsod25_grp4_h1_gives_m00cat_both_lambdas_and_stealthlol_none() {
        let lambda = [101.75, 510.0, -131.625];
        let m00cat = plan_of("wsod25_grp4_h1_m00cat_dod_railroad2_test_1.dem");
        println!("m00cat: {}", m00cat.summary());
        assert_eq!(m00cat.aimed.len(), 2, "{:?}", m00cat.aimed);
        assert_eq!(m00cat.verdict(M00CAT, lambda), Verdict::Recorders);
        let stealth = plan_of("wsod25_grp4_h1_stealth.dem");
        println!("stealthlol: {}", stealth.summary());
        assert_eq!(
            stealth.verdict(STEALTHLOL, lambda),
            Verdict::RecorderSpraysForReal
        );
    }
}
