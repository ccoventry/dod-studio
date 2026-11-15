# Editing what happens in a demo (#1469)

How far a recorded GoldSrc demo can be rewritten so the game plays back
something that never happened: a player who walks another way, or a player
who is there for seconds the demo never had him. The R&D tool is
`analysis/examples/entity_edit_probe.rs` (its header lists the modes).

Every number below was measured on the demos named; what was seen in the
game is marked as such, and what is inferred says so.

## Short answer

LIVE_SUMMARY

## How a player is stored, and what an edit has to keep true

A player another client sees is an entity in the `svc_deltapacketentities`
stream (`docs/goldsrc_demo_salvage.md` has the structure). The facts an edit
rests on:

- **`origin` and `angles` are absolute.** A packet lists only the fields that
  changed since the snapshot its `delta_sequence` names; a field it leaves
  out carries over from that snapshot. So an edit that changes his origin
  must list it in every packet that holds him, or packets that do not list it
  carry a stale (edited or unedited) value forward. `bend` lists all three
  origin fields in every packet of the window that holds him, inserting an
  entry for him where the packet had none.
- **A player new to a snapshot is decoded against his spawn baseline**, not
  against what the client last had. So the first real packet after a gap
  lists every field that differs from the baseline, and leaves out the ones
  that equal it. If made-up frames before it set a field the real packet
  leaves out, that made-up value survives into the real data. `fill` makes
  every made-up packet and the first real one list his whole state.
- **Indices are re-encoded per packet.** An inserted entry changes the
  6-bit index difference of the entry after it; `set_entity_fields`
  re-encodes the packet's differences from 0, absolute only for a gap over
  63, as the engine writes them.
- **`entity_count` is the resulting snapshot's size** and a count below the
  carried set is a crash (`goldsrc_demo_salvage.md`, "Still open"). Adding a
  player to 117 packets raised 117 counts; `restate_counts` rewrites them
  from the carried set (the repair's `entity_count` step). A bend never
  changes a snapshot's membership, so it changed none.
- **Events find their player by place in the snapshot, not by number.** An
  `svc_event` names its entity with `packet_index`, an index into the
  frame's snapshot, and the client takes the event's origin and angles from
  that entity when the event carries none (Tron's 28 `mg42.sc` events in
  302.0-304.0 s all do it that way: packet index 4, no origin of their own).
  So:
  - a bent player's shots fire from where he is drawn (inferred from the
    encoding, not watched: the bent player in step 1 does not shoot);
  - adding a player to a snapshot moves every higher-numbered entity one
    place up, and an event pointing at one of them would fire from the
    wrong entity. `shift_event_packet_indices` moves those events up one in
    every frame he was added to.
- **Sounds carry their own origin.** The other player's footsteps,
  landings and similar sounds arrive as `svc_sound` with an explicit
  position (`heard`: entity 4 in step 1's window has 91
  `player/jumplanding.wav` and 11 `pl_step*`/`pl_wood*`, every one with x, y
  and z). Bent, they still play where the demo had him.
- **The writer round-trips.** `solifotw-pb2.dem` writes back byte for byte
  (96,936,350 bytes). `tr2.dem` (the 2004 build) writes back the same length
  with 6 bytes different, all in the directory at the end of the file
  (offset 20,216,515 on): a directory field the writer recomputes. The
  frames are unchanged.

## Step 1: move a player in a POV demo

STEP1

## Step 2: fill a gap before a player first appears

STEP2

## Step 3: change the recorder's own view (plan)

STEP3

## Step 4: bigger edits (plan)

STEP4

## Edit the file or fake it in the hook

HOOKVSFILE

## Files

Kept out of the repo, in `D:\dodstudio-tests\unattended-2026-11-16\demoedit\`:
FILES
