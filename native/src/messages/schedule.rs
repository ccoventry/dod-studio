// messages/schedule.rs
// Activity-log note of the schedule builder (`patch/builder/schedule.rs`,
// #237). The function returns the line only; the caller writes it to the log.

use std::fmt::Display;

/// A Scheduled Command that lands while playback is fast-forwarding. The
/// note has eight moving parts, each a different clause of one sentence.
#[allow(clippy::too_many_arguments)]
pub fn runs_during_fast_forward(
    command: impl Display,
    relation: impl Display,
    offset: f32,
    direction: impl Display,
    target_tick: i32,
    where_: impl Display,
    host_framerate: impl Display,
    fix: impl Display,
) -> String {
    format!(
        "ℹ️ **Scheduled command runs during fast-forward** — `{command}` is set {relation} {offset:.1}s \
         {direction} the highlight, which lands at tick {target_tick}, {where_}. Setting a cvar there is \
         usually fine — it simply takes effect early. It is worth checking only \
         for commands that depend on playback running at real speed: anything \
         touching sound, recording, or rendering, since the engine is at \
         `host_framerate {host_framerate}`, drawing nothing, with its audio buffers unflushed. \
         To have it run at normal speed instead, {fix}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fast_forward_note_keeps_its_wording() {
        assert_eq!(
            runs_during_fast_forward(
                "say hi",
                "before",
                2.0,
                "after",
                120,
                "after the post-roll ends",
                1000,
                "raise it"
            ),
            "ℹ️ **Scheduled command runs during fast-forward** — `say hi` is set before 2.0s after the highlight, which lands at tick 120, after the post-roll ends. Setting a cvar there is usually fine — it simply takes effect early. It is worth checking only for commands that depend on playback running at real speed: anything touching sound, recording, or rendering, since the engine is at `host_framerate 1000`, drawing nothing, with its audio buffers unflushed. To have it run at normal speed instead, raise it."
        );
    }
}
