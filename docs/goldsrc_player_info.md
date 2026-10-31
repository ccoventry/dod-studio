# client.dll's per-player record: `g_PlayerExtraInfo`, and what demos fill in it

`client.dll` keeps one record per player for the scoreboard, the kill feed's colours, the status bar and the spectator UI. The hook reads it for the overview's camera icon, the butt-stroke icon and the name plates (`ds_hud_show_player_names`, `ds_hud_show_player_health`, #1130). This page says where it is, who writes each field, and what POV and HLTV demos give it.

## 1. Where it is

`extra_player_info_t g_PlayerExtraInfo[65]` at `client+0xe9118`, 48 bytes an entry, indexed by entity index (1..=32 are the players). The names, member order and size come from `client.so`'s DWARF (`linux_so.py client struct extra_player_info_t`; the object is 3,120 bytes, 65 entries); the offsets are `client.dll`'s own, from its code. `client.dll` is byte-identical in the PRE and POST installs, so one table serves both.

| Offset | Type | Field | Written by (`client.dll`) |
|---|---|---|---|
| `+0x00` | `short` | `frags` | message handlers |
| `+0x02` | `short` | `objscore` | message handlers |
| `+0x04` | `short` | `deaths` | message handlers |
| `+0x06` | `short` | `playerclass` | message handlers; `HUD_ProcessPlayerState` when spectating |
| `+0x08` | `short` | `teamnumber` | message handlers; `HUD_ProcessPlayerState` when spectating |
| `+0x0c` | `int` | `status` | message handlers |
| `+0x10` | `char[16]` | `teamname` | message handlers |
| `+0x20` | `float` | `showhealth` | nothing: never read or written |
| `+0x24` | `int` | `health` | `HUD_ProcessPlayerState`, `CHud::MsgFunc_HLTV` (section 2) |
| `+0x28` | `int` | `teamId` | message handlers |
| `+0x2c` | `bool` | `dead` | message handlers |

Checked by scanning every instruction in `client.dll`'s `.text` whose memory operand points into the table: 82 of them, every one on a field boundary above. "Message handlers" are `DoDViewport`'s user-message code (`ScoreInfo`, `PClass`, `PStatus`, the team messages), where `name_client_dll.py`'s map pairs the writing functions with wrong names (`MsgFunc_MOTD`, `DisplayVGUIMenu`), so which handler writes which field is not pinned here; the offsets don't depend on it. `goldsrc-hooks/tools/verify_player_info_offsets.py` checks the `health`, `teamnumber` and `dead` operands in both installs.

## 2. Who fills `health`

Two writers, and nothing else in `client.dll` touches the field:

- **`HUD_ProcessPlayerState`** (`client+0x31130`) copies `entity_state_t::health` (`+0xac`) into the table for every player in the frame's packet, unconditionally, in every kind of demo. DoD's `delta.lst` sends it for players (`DEFINE_DELTA( health, DT_BYTE, 8, 1.0 )` in `entity_state_player_t`). The same function copies `team` and `playerclass` too, but only when `IsSpectateOnly()`.
- **`CHud::MsgFunc_HLTV`** (`client+0x222c9`) reads the `HLTV` user message, `BYTE player, BYTE health | 0x80` (player 0: every entry 0..63). The server writes it in `CBasePlayer::TakeDamage`, `UpdateClientData` and `CDoDTeamPlay::RestartRound` (`user_message_layouts.py writers HLTV`), to spectators only.

So `health` is current for every player the recorder's packet carries, in POV and HLTV demos alike; in an HLTV demo the `HLTV` message also updates players outside the packet. A player out of view keeps the last value heard.

## 3. What the demos carry

`analysis/examples/player_health_probe.rs` counts, per demo, the player-entity deltas that carry `health` and the `HLTV` messages that carry a health value:

```
cargo run --release -p analysis --example player_health_probe -- <demo>...
```

Surveyed on 27 demos: 20 POV demos from 19 recorders (17 `solifotw-*` Frags of the Week recorders, plus dyelife and stealth from `ktps8w1` and k4 from `ktps9w1`) and 7 HLTV demos from three events (`ktps9qf` x2, `monday-wsod25_r07` x2, `wsod25_grp1/3/5`).

| | POV (20) | HLTV (7) |
|---|---|---|
| Player deltas carrying `health` | 1,790..12,802 per demo | 914..1,369 per demo |
| Players with health | every player (12) in every demo | every player (12-13) |
| Values 1..=100 (alive) | 80-84% of them | 64-68% |
| `HLTV` messages | none | 1,323..1,840 per demo, about half with a health value, for every player |

The rest are outside 1..=100 (not broken down further; a dead player's is 0). HLTV demos carry fewer entity deltas, and the `HLTV` message fills in between. In both kinds every player's health reaches the client, so a health bar works in both.

## 4. The name plates

`goldsrc-hooks/src/nameplates.rs` draws from this table after `client.dll`'s own `HUD_Redraw`, in demo playback only (in a live game the plates would show enemies through walls):

- **Who:** the players `client.dll`'s `HUD_AddEntity` let through this frame. That is every player the engine is about to render: not the ones outside the recorder's view, nor the one an HLTV camera watches in-eye (the client returns 0 for him), nor a POV demo's recorder. A player the table holds as `dead`, or whose `teamnumber` is not Allies (1) or Axis (2), gets no plate.
- **Where:** 12 units over `curstate.maxs[2]` (which follows crouch and prone), at `cl_entity_t::origin`, through TriAPI's `WorldToScreen`, the projection the engine just rendered with.
- **Through walls:** "drawn this frame" means in the recorder's potentially visible set, not in his line of sight. A trace per player would hide the ones behind walls; that is for the user to choose.

<!-- LIVE RESULTS -->
