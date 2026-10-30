# The `ds_*` console surface

One place to scan the whole `goldsrc-hooks` cvar/command surface without
grepping `commands.rs`. Closes issue #313.

Every name here shares the `ds_` prefix ([`names.rs`](../goldsrc-hooks/src/names.rs)'s
`console_name!` macro). `ds_debug_status` reports the live value of
everything below in one place (cvars unconditionally; the two fixes with
preconditions only while enabled), and each item's own doc file has the full
engine-level detail this table deliberately doesn't repeat.

Diagnostics (status, logging, the HD miss list) all sit under
`ds_debug_`, so typing that prefix in the console lists every one of
them; everything else is a setting you'd use for a capture. No name is the
start of another, because the console's autocomplete would otherwise swap
the shorter one for the longer when you press space.

**Scope:** the surface actually on `dev` today: forty-nine cvars and
sixteen commands. Entries still in open PRs are not listed -- update this file as
part of merging each one, the same way every one of them already updates
`README.md`'s own control-surface list.

## Cvars

A cvar shows up in the console's own type-ahead with its current value,
answers a bare-name query, and can be set from `+name value` on the launch
line or a line in any `.cfg` the user execs -- unlike a command, none of that
needs remembering a subcommand shape. None of them are `FCVAR_ARCHIVE`: the
game never writes one into the user's `config.cfg`, matching the pipeline's
standing "user `.cfg` files are never written" rule (`CLAUDE.md`).

A setting defaults to `1` when it is a clear win for everyone (a resizable
window, Match POV, the sniper scope in demos), and to `0` when it is a big
change or not everyone would call it an improvement (hiding part of the HUD,
the death cam).

| cvar | default | what it does | doc |
| --- | --- | --- | --- |
| `ds_spec_match_pov` | `1` (on) unless `GOLDSRC_HOOKS_SPEC_MATCH_POV=0` at launch | one switch for making a spectated first-person view look like the player's own recording. It turns on five things together: the first-person weapon's animations (firing, reloading, drawing, the bipod families, and grenades including primed ones); the spectator crosshair drawn from `sprites/customXHair.spr` with the tile `cl_xhair_style` gives the POV view (needs `cl_xhair_style` 1 or higher, see issue #308; loses to `ds_hud_hide_crosshair`), and hidden when the player's own would be while playing: sprinting, in the air after a jump, going prone or getting up, crawling, on a ladder, reloading, just after a weapon switch, cycling a bolt rifle, holding a knife, spade or sniper rifle, or a machine gun that is not deployed; the camera dropping to the ground when the player goes prone, where the game leaves it at crouch height; the camera going down into a crouch over the 0.4 s their own view takes and ending when theirs does, where the game drops it four times faster and later, and back up at their own view's speed (#1144; to start with their view it reads the demo file ahead of playback on a background thread, #1154, and `ds_debug_status` says what it read); and the gun lowering off screen while they sprint, jump, go prone, crawl or climb a ladder, as their own does, and coming back up as theirs does when they let go of sprint running off an edge (#704, #1154) (`goldsrc-hooks/src/spectator_gun.rs`). All five stand down while a POV demo plays, which is the recording they match (#613); live play and HLTV demos are unaffected. The gunshots a demo lost are not part of it any more: they are `ds_sound_restore_gunshots`, in HLTV and POV demos alike (#1036). So a config that sets `ds_spec_match_pov 0` to silence the restored gunshots now needs `ds_sound_restore_gunshots 0` as well, and the "log every restored gunshot" that `ds_spec_match_pov 2` used to turn on is `ds_sound_restore_gunshots 2`. It replaces `dodstudio_hltv_show_viewmodel_animations`, `dodstudio_hltv_play_missing_gunshots` and `dodstudio_match_pov_crosshair`; `dodstudio_hltv_gunshots_fix` and `dodstudio_hltv_gunshot_attenuation` are gone, since making gunfire carry further than POV hears it is not matching POV | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md), [`goldsrc_hltv_missing_gunshots.md`](goldsrc_hltv_missing_gunshots.md), [`goldsrc_crosshair.md`](goldsrc_crosshair.md) §2, [`goldsrc_spectator_camera.md`](goldsrc_spectator_camera.md) |
| `ds_debug_log_weapon_model` | `0` | logs the third-person weapon model the spectated player holds, each time it changes | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) |
| `ds_debug_log_gun_lowered` | `0` | R&D (#704): logs each time the first-person gun goes down or comes back up, with the demo player's clock: in a POV demo from `client.dll`'s own flag, in HLTV in-eye from `ds_spec_match_pov`'s copy with its reason (sprinting, prone transition, crawling, in the air, on a ladder). In a POV demo it reads `client.dll`'s own lowering sum (`g_ihidexhair`), not the gun that is drawn, so it misses POV's lowering on a jump or crouch-jump: check the drawn frames before calling a POV-vs-HLTV difference real | `goldsrc-hooks/src/gun_log.rs` |
| `ds_debug_log_spectator_target` | `0` | logs who the spectator HUD thinks is being followed next to the entity the engine renders a viewmodel for, whenever either changes (issue #206) | `goldsrc-hooks/src/spectator_target.rs` |
| `ds_hud_hide_scoreboard` | `0` | stops a POV demo's recorded TAB presses from putting the scoreboard over the shot | [`goldsrc_scoreboard.md`](goldsrc_scoreboard.md) |
| `ds_sound_mute_voice_commands` | `0` | silences "fire in the hole!" and the rest, without touching the game's own `.wav` files; subtitles and speaker icons still show | [`goldsrc_hud_suppression.md`](goldsrc_hud_suppression.md) |
| `ds_sound_mute_victory_music` | `0` | silences the victory music a map plays to everyone when a team wins the round, without touching the game's `.wav` files or the demo; every other sound plays, the capture announcements too (`ds_sound_mute_flag_capture`). It acts on the next one the server sends, so music already playing finishes. It reads each map's own victory music from the map as it loads (what its round-end wiring plays, `no_winner.wav` for a round with no winner included), so a custom map with its own file names is muted too; a list of 37 file names from the 250 maps in both movie installs also applies, for a map it can't read. dod_vicenza's `uswin` / `germanwin` are also placed as ambient sounds there, and are its victory music too (#284) | `goldsrc-hooks/src/map_sounds.rs` |
| `ds_sound_mute_flag_capture` | `0` | silences the point-captured announcements a map plays, and also the objective-secured ones some maps play the same way (dod_escape's `usobjectivesecure`), without touching the game's `.wav` files or the demo; every other sound plays, the victory music too (`ds_sound_mute_victory_music`). It acts on the next one the server sends. It reads each map's own capture sounds from the map as it loads (its control points' sound settings), so a custom map with its own file names is muted too; a list of 61 file names from the 250 maps in both movie installs also applies, for a map it can't read (#284) | `goldsrc-hooks/src/map_sounds.rs` |
| `ds_sound_restore_gunshots` | `1` | plays the gunshots a demo fired but has no sound for, with their sound, muzzle flash, tracer and impact, from the weapon's own fire code. It picks how by what is playing (#1036). **HLTV demos:** every player's rounds with no fire event, found from the shooter's body restarting its fire animation (about 60% of rounds in some recordings). **POV demos, other players:** the same, for every player in the recorder's view; about 1 in 7 of their rounds has no fire event (14% over 361 demos from 27 recorders). **POV demos, the recorder:** his own shots the server fired but his game never predicted, found from the clip count the server sends him (`CurWeapon`), about 1 shot in 360; it plays a little over half of them and leaves the rest silent rather than risk doubling a shot (#1029). A round whose own sound arrives plays only that, and a real empty-gun press is left as it is. Live play is unaffected. `2` also logs every restored round. Not part of `ds_spec_match_pov` (it was until #1036): a config that turned that off to silence restored gunshots needs this at `0` too | [`goldsrc_hltv_missing_gunshots.md`](goldsrc_hltv_missing_gunshots.md), `goldsrc-hooks/src/pov_recorder_shots.rs` |
| `ds_hud_hide_crosshair` | `0` | hides the crosshair and keeps it hidden, which the stock `crosshair` cvar can't do because `CHud::Redraw` forces the value back every frame | [`goldsrc_crosshair.md`](goldsrc_crosshair.md) |
| `ds_spec_hide_bars` | `0` | hides the spectator panel while spectating, in a demo or live: the two dark bands across the top and bottom of the screen, the score, timer and player name on them, and the menu row DUCK brings up. On screen, with no capture running | [`goldsrc_spectator_bars.md`](goldsrc_spectator_bars.md) |
| `ds_spec_hide_text` | `0` | hides the two lines the game prints when the view becomes a spectator camera: the camera mode's name, centred just above the bottom bar ("Locked Chase Cam", "Free Chase Cam", "Free Look", "First Person", `#Spec_Mode1`..`4`), and the green "Press DUCK for Spectator Menu" (`#Spec_Duck`). Both show in a POV demo when the player dies, and in HLTV when the camera mode changes. The mode name comes twice, from the game itself and as a `TextMsg` the server sent; both go. Every other centre-print and HUD message still shows. A line already on screen when it's turned on stays until it times out (#677) | `goldsrc-hooks/src/spectator_text.rs` |
| `ds_spec_lock` | `0` | HLTV demos: the camera stays on the player being watched when they die. Without it the game moves to the next player four seconds later. The viewer's own keys still change player | [`goldsrc_spectator_camera.md`](goldsrc_spectator_camera.md) |
| `ds_spec_deathcam` | `0` | HLTV demos: when the player being watched in first person dies, the view becomes the death cam their own recording shows, the camera 64 units behind their body pulling back and turning to the killer, with no gun, for 2.4 s; then it is the game's own view from their eyes again until they respawn. Without it the game rolls the dead player's eye view 80° and the dropped weapon fills the clip. A switch of its own, not part of `ds_spec_match_pov`: a movie wants it, a shoutcast following the fight does not. Stands down in a POV demo (`goldsrc-hooks/src/spectator_deathcam.rs`) | [`goldsrc_spectator_camera.md`](goldsrc_spectator_camera.md) |
| `ds_hud_overview_players` | `0` | which players get an icon on the overview map: `0` the game's own (the team of the player being watched), `1` everyone, `2` only the other team, `3` only the player being watched (the recorder in a POV demo), `4` nobody. Applies where the game draws the overview map, which today means HLTV demos only: a POV demo shows no overview at all, alive or dead (#897) | `goldsrc-hooks/src/overview_players.rs` |
| `ds_player_overrides` | `0` | sprays and crosshairs in demos follow Studio's Players table (Configuration > Players, #428); 0 leaves both to the game. Studio sets it from the tab's "Use in the game" box when it starts the game, a review or a batch, and sends it to a running game when the box changes. A flip reaches the crosshair on the next frame, and sprays loaded after it: a spray already on a wall keeps its texture, so reload the demo to compare | `goldsrc-hooks/src/player_overrides.rs`, `player_crosshair.rs`, `spray.rs` |
| `ds_view_use_cl_bob` | `0` | a POV demo bobs the view and the gun by the `cl_bob` (and `cl_bobcycle`, `cl_bobup`) set now, as live play and an HLTV demo watched in first person do (#359, #889). At `0` a POV demo shows the bob it recorded, the recording player's own settings (DoD's default `cl_bob` is `0.005`; some recorders had `0`), because the engine replaces the view `client.dll` works out live with the recorded one (HLTV demos already follow the cvars). At `1` the recorded bob is swapped for the live one the way DoD bobs: the view sideways, the gun forward and tilted (pitch, yaw and roll), never up; the rest of the recorded view stays. The bob's cycle runs on the demo's clock (the demo player's world clock under `viewdemo`, the recorded client time under `playdemo`), not on the client's own timer since launch, so every capture of the same moment sways the same way and two passes of one clip line up. While the recording player is dead or spectating someone, the recorded view is left alone | `goldsrc-hooks/src/pov_live_bob.rs` |
| `ds_debug_log_view_bob` | `0` | logs `ds_view_use_cl_bob`'s numbers every frame: in a POV demo the demo clock, the live bob on it and the bob the client worked out itself, the recorded bob, why a frame was left alone, and the gun's place relative to the camera; in an HLTV demo watched in first person the same for the live view, to compare | `goldsrc-hooks/src/pov_live_bob.rs` |
| `ds_debug_log_server_sounds` | `0` | logs every sound the server sends (`svc_sound`, `svc_spawnstaticsound`): its entity, channel, file name and flags, and whether it played or which of `ds_sound_mute_victory_music` / `ds_sound_mute_flag_capture` muted it, and whether the name came from the map (`(map)`) or the fixed list (`(list)`). Each map's own count is logged as it loads, and `ds_debug_status` shows the current one | `goldsrc-hooks/src/map_sounds.rs` |
| `ds_debug_record_frame_times` | `0` | times every frame the game draws while `1` (from `HUD_Frame`, two atomic stores a frame); back at `0` it prints the frame count, average fps, 1% and 0.1% lows (from the 99th and 99.9th percentile frame times) and the longest frame, and writes every frame's time in microseconds to `%APPDATA%\dod-studio\frame_times.txt` on a thread of its own. Studio's OBS capture test turns it on for each test clip (#646) | `goldsrc-hooks/src/frame_times.rs` |
| `ds_capture_run_in_background` | `0` | the game runs at full speed while another window is in front. The engine otherwise waits up to 20 ms for input every frame while it isn't the active app (`CEngine::Frame` calls `SleepUntilInput` when `IsActiveApp` is false), about 45 frames a second, which slows a capture batch's fast-forward between clips 5-8x; this skips that wait. A menu left in the background keeps drawing at full speed while it's on | `goldsrc-hooks/src/run_in_background.rs` |
| `ds_capture_stay_behind` | `0` | the game window stays behind the other windows while it records: HLAE no longer brings it to the front, topmost, for every recording. A minimised game records black frames (its window owns no pixels), so while this is on a minimise parks the game off-screen instead: restored without being activated and moved left of the whole desktop, same size, taskbar button kept (#723). Activating it (its taskbar button, alt-tab) or turning this off moves it back where it was. Studio turns it on for every batch unless Keep the Game Behind Your Windows While Capturing is off | `goldsrc-hooks/src/hlae_window.rs` |
| `ds_hud_overview_team_marker` | `0` | which camera icon marks the player being watched on the overview map: `0` the game's own, `1` their team's in HLTV too, `2` none. `1` uses `allies_camera.spr` / `axis_camera.spr` / `brit_camera.spr` instead of `spec_camera.spr`, by the watched player's team (#907), and keeps `spec_camera.spr` while the camera follows nobody (roaming, free overview); DoD Studio's bordered ones go in `dod_addon/sprites`. `2` works in POV and HLTV, so `spec_camera.spr` needs no blanking, and the player icons stay (#677). In demo playback the game draws the overview map only in HLTV demos today (#897) | `goldsrc-hooks/src/overview_marker.rs` |
| `ds_hud_overview_hide_speaker` | `0` | leaves the speaker icon (`mapsprites/speakerIcon.spr`) off the players' icons on the overview map, so the sprite needs no blanking; the player icons stay, unlike `ds_hud_overview_players 4`. What makes the game draw it (a player talking or using a voice command) is a guess (#677) | `goldsrc-hooks/src/overview_speaker.rs` |
| `ds_view_hide_headshot_red_screen` | `0` | POV demos: drops the red screen flash (a server `ScreenFade` of (128,0,0) at alpha 200, flags 0) that comes with every hit on the player's head hitbox, fatal or not, grenades included (#606). The black fades (the clear at respawn, the end of the map) still play | `goldsrc-hooks/src/screen_effects.rs` |
| `ds_view_hide_screen_shake` | `0` | POV demos: drops every server `ScreenShake`, which in DoD demos is the blast shake from a grenade going off nearby (amplitude 16 for 1 s; no WSOD25 map has an `env_shake`) (#606), and takes the shake the recorder's client already put into the recorded view back out of it (#910) | same |
| `ds_view_show_own_blood` | `0` | POV demos: shows the recording player's own blood when they're hit, which the game skips this close to the camera. DoD draws no blood puff closer than 32 units to the view (`EV_BloodPuff`), and the recorder's own wounds land a few units below the eye, so the demo has them (25-58 per demo in 6 demos from 6 recorders) and the game never draws them; a puff 27 units away drew at 60 fps and not at 300, by chance (#1018). The value picks the cut: `0` the game's own 32, `1` 24, `2` 16 (most sprays, without the blocky close-ups), `3` 8, `4` 0 (everything; under about 16 units a puff covers a third of the screen with blocky pixels and a pale haze). Switches live; a value past 4 gives 4, below 0 gives 0, with a console line. Only the blood-puff cut changes (#1035) | `goldsrc-hooks/src/own_blood.rs` |
| `ds_hud_show_scope` | `1` | shows the sniper scope while a demo plays. The game turns it off for playback (`CHudScope::DrawTriApiScope` skips its draw when `IsPlayingback()`). A POV demo shows the recorder's own scope whenever they zoomed. Watching in-eye, the game always picks the Springfield's scope; with this on it is the watched player's rifle: the scoped K98 and scoped FG42 get `scope_k43`, the Springfield `scope_spring`, the scoped Enfield `scope_enfield` (that one only when the game's scoped-weapon flag is set, which an HLTV viewer may not have). Still only while the view is zoomed (#307). The round scope keeps its size and everything outside its frame is black, at any FOV: DoD sizes the frame for the view it expects, so `gl_widescreen_yfov 1` (25th Anniversary) or a wider `mirv_fov` used to show the map around it; demos only, live play keeps the game's own scope (#799) | `goldsrc-hooks/src/demo_scope.rs`, `goldsrc-hooks/src/demo_scope/fill.rs`, [`goldsrc_hud_elements.md`](goldsrc_hud_elements.md) |
| `ds_player_hide_hand_signals` | `0` | replaces any `hs_*` body sequence (the nod, the point, the wave -- players miming their own voice commands) with that player's last ordinary one, for everyone in view | [`goldsrc_hltv_animation_fix.md`](goldsrc_hltv_animation_fix.md) §12 |
| `ds_player_fix_model_flicker` | `1` | player models hold still from the frame an HLTV demo's clock steps back until it passes the last frame they were drawn on (one frame at normal speed, a few at 1/4 speed). The step back otherwise turns a strafing player's model round for that frame, several times a second, so his upper body flickers (#927). It happens under `viewdemo` at a frame rate above the demo's packet rate. A switch of its own, not part of `ds_spec_match_pov`: it is a bug in how the game draws a model, which HLTV only shows more often, not a difference in what an HLTV demo records. Stands down in a POV demo, where the clock (almost) never steps back | `goldsrc-hooks/src/strafe_twitch.rs`, [`goldsrc_dod_quirks.md`](goldsrc_dod_quirks.md) |
| `ds_hud_hide_voice_menus` | `0` | hides the numbered menus a POV demo's player picked voice commands and hand signals from: the three voice-command menus (`voice_menu1`-`3`, which the server sends as `ShowMenu` with the nine `#Menu_*Voice*` keys) and the command menu `+commandmenu` opens, where the hand signals are. The menus still take the recorded number presses, so weapon switches replay as recorded; other `ShowMenu` text menus (server plugin menus and votes) still show, unless `ds_hud_hide_plugin_text` is on. While on, `+commandmenu` doesn't open in live play either. Turning it on or off with a voice menu up hides or shows it at once | `goldsrc-hooks/src/voice_menus.rs` |
| `ds_hud_hide_hints` | `1` | hides DoD's auto-help hint box, the tip with a soldier's head in the top-left corner that a POV demo shows wherever its player recorded with `_ah 1`. The viewer's own `_ah` can't stop it: `client.dll` never reads `_ah`, and the hint is a flagged `HudText` message in the demo. Stubs `DoDViewport::ShowHint`, the one thing that opens the box, so ordinary `HudText` (map text, DoD's prompts) still shows. On by default: it is the recorder's tutorial and tells a viewer nothing. Turning it on also closes a hint already on screen. Also hides the machine gun's "Deploy your machine gun to Reload!" prompt, the other piece of the game teaching the recording player (#342): the one call that queues it goes to a filter that drops it, and server centre text, which goes another way, still shows (that is `ds_hud_hide_center_text`'s). Applies in live play too (#286) | `goldsrc-hooks/src/hint_panel.rs`, `goldsrc-hooks/src/mg_reload_hint.rs` |
| `ds_hud_hide_teammate_info` | `0` | POV demos: hides the line under the crosshair that names the teammate the recording player aims at and gives their health (`Name (100)`). Nothing else ever shows there: the game draws it only for a living POV player aiming at a teammate, never for an enemy, and never in an HLTV demo or while spectating. `hud_centerid` only moves the line. The team icon over teammates' heads is a different thing (`cl_identiconmode 0` hides it). Stubs the status bar element's `Draw` (#342) | `goldsrc-hooks/src/teammate_info.rs`, `goldsrc-hooks/src/hudelement.rs` |
| `ds_hud_hide_map_text` | `0` | hides the text a map puts on screen itself -- the `dod_anzio` mortar warning, the round result -- by matching each `HudText` message against the `message` strings the loaded map's own entities declare. DoD's own prompts on the same channel (`#Clan_allies_ready` and friends) still show. Reads the map's BSP once per level | `goldsrc-hooks/src/map_text.rs` |
| `ds_hud_hide_clan_text` | `0` | hides all of DoD's clan-match text: the warm-up and ready rules, "Warmup Mode" on the left, the countdown to the match and "MATCH IS LIVE!". Drops the `#clan_*` `TextMsg` keys, swallows `ClanTimer` (which draws both the countdown and "MATCH IS LIVE!") and clears `GameRules`' warm-up flag. Turning it off mid-countdown brings the countdown back at once; turning it on mid-countdown removes "Warmup Mode" but not yet a countdown already showing (#618). Leaves everything else alone, round-start text included | `goldsrc-hooks/src/clan_text.rs` |
| `ds_hud_hide_center_text` | `0` | hides the text the server prints in the middle of the screen, whichever way it sends it: every `TextMsg` to `HUD_PRINTCENTER` (destination 4), such as match plugins' `print_center` countdowns and notices, and the engine's own `svc_centerprint`, which `hw.dll` draws itself (one call in its handler becomes `nop`s, per engine build). Chat, joins and leaves (destination 3) still show. Turning it on also ends a `TextMsg` centre line showing at that moment; an engine one stays for its last `scr_centertime`. Takes the server's copy of the spectator camera mode's name too, but not the game's own (`ds_spec_hide_text` hides both); doesn't reach the clan countdown (`ds_hud_hide_clan_text`) (#677) | `goldsrc-hooks/src/center_text.rs`, `goldsrc-hooks/src/engine_center_print.rs` |
| `ds_hud_hide_plugin_text` | `0` | hides what server plugins put on screen: their coloured HUD messages (`set_hudmessage`/`show_hudmessage`: KTP's "Match Pending" and ready lists, practice-mode banners, stats lines) and their menus (AMX Mod X menus and votes: every `ShowMenu` text menu but DoD's voice-command menus, which are `ds_hud_hide_voice_menus`'). The HUD messages arrive as `TE_TEXTMESSAGE`, which the engine hands to every `HudText` handler, so no message hook can drop them: one call in `hw.dll`'s `TE_TEXTMESSAGE` handler goes through a filter instead, per engine build. A line the loaded map declares on its own `game_text` still shows, and so does everything on the `HudText` message itself (map text, DoD's prompts). A line already on screen fades out on its own (#677). A hidden menu still takes the recorded number presses, so they don't switch weapons. Turning it on or off with a menu up hides or shows it at once. It was `ds_hud_hide_server_text` before #342 | `goldsrc-hooks/src/plugin_text.rs`, `goldsrc-hooks/src/voice_menus.rs` |
| `ds_demo_ex_interp_max` | `100` (the engine's own ceiling) | raises the engine's clamp on `ex_interp` above its stock 100 ms ceiling, for smoother entity motion between snapshots; refuses `<=50` or `>1000`. Mechanism live-proven on pre-Anniversary, no specific value settled on yet. On the 25th Anniversary build (where HLTV demos already get 200 ms) any value but the default sets both paths | [`goldsrc_ex_interp.md`](goldsrc_ex_interp.md) |
| `ds_hd_enabled` | `1` if there's a `dodstudio_hd` folder (in `dod_addon`, or `dod` for an install built before #415), else `0`; `GOLDSRC_HOOKS_TEXTURE_HIRES=1`/`0` at launch overrides | HD textures on/off: map textures, model skins, sprites (world and HUD, #728), decals, detail textures and skies from `dodstudio_hd`, and player sprays from PNGs in `logos` (`ds_player_spray`). `ds_debug_status` counts each kind's loads and replacements (HUD sprites apart from world sprites). A change applies to what loads next -- walls, detail and skies from the next map, models, sprites and decals already loaded after a restart. Turning it on in a session that started off installs the hook then | `goldsrc-hooks/src/texture_hires.rs`, `goldsrc-hooks/tools/hd/README.md` |
| `ds_hd_style` | `ultrasharp` | which `dodstudio_hd/<type>/<style>` folder to use; a name with no folder means originals (plus `overrides`). Same timing as `ds_hd_enabled` | same |
| `ds_map_allow_shaders` | `0` | 25th Anniversary only: lets the engine draw map surfaces through its own GLSL shaders (`platform/gl_shaders/fs_world.frag`) during demo playback. The engine gates them on `sv_allow_shaders`, which a demo can never turn on: the console refuses it in multiplayer and every demo load resets it to 0. This writes 1 into it while a demo plays. Needs `gl_use_shaders 1` too. `gl_reloadshaders` recompiles the files live. Does nothing on the pre-Anniversary engine | `goldsrc-hooks/src/world_shaders.rs` |
| `ds_map_weather` | `0` | rain or snow on any map, or none (needs `cl_particlefx 2`): `0` the map's own, `1` rain, `2` snow, `3` none. For moody intro and outro shots, clean shots on rainy maps (anjou, donner), and the same weather across clips from different matches. DoD's client keeps one weather type, read from the map's `info_doddetect` at each load (28 of the movie installs' maps set one: 20 rain, 8 snow); this points the weather updater's read of it at a value of its own, so the map's own value is never written and needs no re-applying after a map load, and `0` puts the read back. 1 and 2 are the game's own numbers for rain and snow. The game draws weather only with `cl_particlefx 2` (a console line says so when it is lower) and only under open sky, as on a rainy map; it spawns by game time (rain every 0.3 s, snow every 0.7 s), so 300 fps gets the same density as 60. A forced rain falls straight: the map's wind (`CDoDParticle::m_vGlobalWind`, `client+0x196c60`) is not changed. Switches live; a value past 3 gives 3, below 0 gives 0, with a console line (#1129) | `goldsrc-hooks/src/map_weather.rs` |
| `ds_spec_hide_hltv_messages` | `0` | drops the text an HLTV proxy puts on screen during playback ("You're watching HLTV. Visit www.valvesoftware.com", and a proxy operator's own `msg` lines) as it arrives, so an HLTV demo needs no patched copy. Only director text messages: the pipeline's highlight labels and everything that drives the spectator camera still go through. A message already on screen when it's turned on fades out on its own | `goldsrc-hooks/src/hltv_messages.rs` |
| `ds_debug_log_texture_loads` | `0` | logs every HD-eligible texture load: replaced (from which file) or why not | same |
| `ds_debug_keep_detail_cache` | `0` | puts the engine's detail texture cache back, to compare. By default the hook bypasses that cache (#463, #703): `DT_SetRenderState` skips binding a detail texture it thinks is still bound, and nothing ever resets what it remembers, so a run of surfaces can draw with no detail, or another surface's, depending on where the camera stands. Seen on the 25th Anniversary build: the wall left of the double doors leaving harrington's Allied spawn | `goldsrc-hooks/src/detail_cache.rs` |
| `ds_debug_cloak_instead` | `0` | with `ds_capture_run_in_background` on, a minimised game is DWM-cloaked in place (`DWMWA_CLOAK`, from inside the game's process) instead of parked off-screen, and uncloaked when activated, to compare the two (#723). The hook log says whether DWM accepted the cloak; when it refuses, the game is parked as usual | `goldsrc-hooks/src/hlae_window.rs` |
| `ds_demo_seek_skip_between` | `0` | `1` makes `ds_demo_seek_to`/`_by` land without running the director events and console commands they jump over; `0` runs them, as the editor's Goto does | [`goldsrc_viewdemo.md`](goldsrc_viewdemo.md) |
| `ds_demo_fast_load` | `1` | `viewdemo` reads the demo into its world for up to 8 ms of each frame instead of the engine's 33 network packets a frame, so a demo loads faster while it plays on. A number above 1 is the budget in milliseconds (at most 100); `0` is the engine's own 33 packets. A frame always reads at least 33, so loading is never slower than stock. On by default: faster loading is a clear win for everyone (#798). The hook log has one `fast_load:` line per load with its time, packets and frame times; `playdemo` is unaffected | `goldsrc-hooks/src/fast_load.rs`, [`goldsrc_viewdemo.md`](goldsrc_viewdemo.md) §5 |
| `ds_ui_demo_list_folders` | `1` | the Load Demo window, and the DoD Studio window's Demos tab, also list folders and open them, so demos in subfolders or outside `dod/` can be picked; a row is the demo's path from `dod/`. The Load Demo window lists `../` to go up; the Demos tab has an Up button instead. in the Load Demo window `dod/` also lists each drive (`D:/`), whose folders come straight from the disk and whose demo rows are full paths (`viewdemo` takes them with `ds_demo_other_drives`); the Demos tab has Back, Forward and Up, a Go to dropdown (game folder, Studio's analysed demos: every demo in the analyzer cache as one list, This PC: every drive as one list, which Up opens from a drive and greys out on, drives, Studio's demo folders, folders visited lately), remembers its folder between sessions, and shows an icon per row (up, folder, drive, POV demo, HLTV demo) in a first column (#779) | `goldsrc-hooks/src/demo_list_folders.rs`, #408 |
| `ds_ui_demo_list_hide_empty` | `0` | while folders are listed and counted (`ds_ui_demo_list_show_counts 1`), a folder with no demo anywhere inside it (any depth) is left out | `goldsrc-hooks/src/demo_list_folders.rs` |
| `ds_ui_demo_list_count_subfolders` | `0` | a folder row's demo count on the Demos tab includes its subfolders (the Settings tab's "Include subfolders"); `0` counts only the demos directly in it, and a folder with none there but some deeper reads "0 here, 12 inside" (Hide empty folders keeps it, since empty means none at any depth). Counted off the game thread, with a progress bar on the Demos tab; the hook log says how long each count took (`folder_counts:`) | same |
| `ds_ui_demo_list_show_counts` | `0` (counting every folder slows the window on a big library) | folders are counted in the background, and folder rows on the Demos tab end in "(N demos)" (the game's own Load Demo window shows no counts either way). `0` counts nothing at all, so a folder opens with no progress bar and no second fill; `ds_ui_demo_list_hide_empty` and `_count_subfolders` need the counts, so they do nothing then and their Settings boxes hide | same, #787 |
| `ds_demo_other_drives` | `1` | `viewdemo` and `playdemo` open a demo on another drive by its full path, as in `viewdemo "D:/folder/demo"`. The first time a drive is named its root (`D:\`) is added as a search path, last and under the hook's own path ID, and the rest of the path is opened under it; the game's own files are never looked for there. `0` leaves such a name to the game, which does not find it (the Anniversary file system asserts on the `:` and closes the game) | `goldsrc-hooks/src/demo_other_drives.rs`, #776 |
| `ds_demo_show_load_progress` | `1` | while `viewdemo` is still reading a demo, the stock VCR bar shows a percent and a progress bar where its time label sits (the buffered span over the demo's own playback length, the Playback tab's number), hidden again once the console says "Demo file completely loaded.". Only on the bar layout DoD Studio's launch writes to `dod_addon\resource\DemoPlayerDialog.res` (`native/src/demo_bar_res.rs`, from the user's own `dod\resource` one or the stock `valve` one, never written back), so a game started without `-addons` or without Studio shows nothing; does nothing while `ds_ui_viewdemo_in_panel` is on, since the Playback tab shows it then | `goldsrc-hooks/src/demo_bar_progress.rs`, #465 |
| `ds_ui_viewdemo_in_panel` | `1` | `viewdemo` opens the DoD Studio window (`ds_ui_panel`) on its Playback tab, which takes the VCR bar's time slider and time label; the stock bar stays off screen while this is on, with the window open or closed (ESC → DoD Studio opens the window again); `0` brings the stock bar back | `goldsrc-hooks/src/studio_panel.rs`, #408 |
| `ds_ui_console_in_panel` | `1` | the console key (`toggleconsole`) opens the DoD Studio window on its Console tab, which holds the real console's history, input line and Submit button; the key again goes back to the game and leaves the window open for the next ESC | `goldsrc-hooks/src/studio_panel.rs`, #408 |
| `ds_ui_resizable_windows` | `1` | every GameUI window (VCR bar, events list, Load Demo, Options...) can be resized by its edges, like the console; its controls stretch as far as their `.res` `autoResize`/`pinCorner` allow. `0` puts back the ones it changed | `goldsrc-hooks/src/window_layout.rs`, #408 |
| `ds_ui_remember_layout` | `1` | each GameUI window comes back where it was left, and at its size when resizable, after the game restarts; kept in `%APPDATA%\dod-studio\goldsrc_hooks_windows.txt` | same |
| `ds_ui_review_keys` | `1` | Review highlights answers on the game screen: while a highlight plays and at its end, where the review pauses, a box lists Y Yes, M Maybe, N No, R Replay, B Back, Space Next (on without answering), S Stop: one key for each of the Review tab's buttons, in their order. An answer while the highlight plays ends it and moves on (#1141). For 1 s after each highlight starts playing the keys are greyed and do nothing, so a double press can't answer the next one unseen. ESC opens the DoD Studio window's Review tab for the kill range and note. The keys work only during a review highlight and while keys go to the game: never while typing in the console, chat or a window's box, and at any other time each key does what it always does. They take no bind, so `config.cfg` is untouched. `0` opens the Review tab at each highlight's end instead (#660) | `goldsrc-hooks/src/review_keys.rs` |
| `ds_ui_review_info` | `1` | Review highlights: while a highlight loads and plays, a box on the game screen says which it is, whose and how many kills, with the answer keys. While it plays the box lists its kills (time, victim, weapon), the next one to come in amber behind a `>` (#1140). The answer keys' box shows at a highlight's end either way (#660); with this off the keys still answer while a highlight plays, unseen | `goldsrc-hooks/src/review_keys.rs` |

The HD rows work the same on the pre-Anniversary and the 25th Anniversary
`hw.dll` (#370).

## Commands

Always a command rather than a cvar when it has subcommands or a variable
argument count, which a cvar's single value can't hold.

**Named parameters.** A command (or subcommand) that takes more than three
values, or any optional ones, names its parameters: each value follows its
name (`killer 3`, `at 7:42`, `id <player> in <demo> to <name>`), and the
named parts can come in any order. A row of positional values would need
`-` placeholders for the ones left out, and nobody remembers which slot is
which (#1021). Short commands with a fixed shape stay positional
(`scale 2`, `block <id>...`, `fake <killer> <victim> <weapon>`). A value
with spaces goes in quotes; GoldSrc's console can't put `"` or `;` inside
one. An unknown name is refused, and the message names it, so a typo never
silently does something else.

### `ds_debug_status`

No arguments. Always reports the four suppression cvars, the two
`debug_log_*` cvars and `ds_hud_deathmsg`'s own status; the rest
(including the animation fix and gunshots fix) are only included while
enabled, since a flag being on says nothing about whether
their preconditions are currently being met and they'd otherwise be noise
when off. Force-calls the per-frame `poll()` first, so chaining a cvar set
and a status query on one `;`-joined console line reports the post-change
state, not the state from before that line ran.

### `ds_hud_deathmsg`

Raises DoD's four-line cap on the kill feed, moves it, hides frags, injects
one by hand, changes the names' font, or scales the weapon icons. It also
changes or hides one recorded kill (`replace`, #1021), and empties the feed
now, at a new round or when the watched player dies (`clear`). The font
(#562), the scale (#728), the layout (#1022) and the gap take effect on the next frame.
HLAE's own
`mirv_deathmsg` supports only
`cstrike` and `tfc`, so none of it works for DoD. See
[`goldsrc_death_notices.md`](goldsrc_death_notices.md).

| subcommand | does |
| --- | --- |
| `ds_hud_deathmsg` | status + usage |
| `ds_hud_deathmsg max <4..127>` | lines of kill feed shown at once (default 4) |
| `ds_hud_deathmsg offset <-4096..4096>` | y the feed starts at (default 20); negative pulls it above the top of the screen |
| `ds_hud_deathmsg offset default` | hand y back to the default layout: while spectating, just below the spectator bar (at the top while it's hidden, under the minimap while `_cl_minimap 2` shows it) |
| `ds_hud_deathmsg block <id>...` | hide frags involving these players (replaces the set) |
| `ds_hud_deathmsg block !<id>...` | hide everything *except* these players |
| `ds_hud_deathmsg block clear` | stop hiding anything |
| `ds_hud_deathmsg players` | list each player's slot, name and SteamID as `block` sees them (0 = the engine gave none) |
| `ds_hud_deathmsg fake <killer> <victim> <weapon>` | inject one by hand; weapon is a name (`d_garand`, `garand`) or `1..43` |
| `ds_hud_deathmsg replace [killer <id>] [victim <id>] [at <time>] [in <demo>[,<demo>...]] with [killer <id>] [victim <id>] [weapon <weapon>]` | change one kill-feed line as it arrives (see below) |
| `ds_hud_deathmsg replace ... with hide` | hide just that line |
| `ds_hud_deathmsg replace list` / `remove <n>` / `clear` | list the lines (numbered), drop one, or drop them all |
| `ds_hud_deathmsg font <face> <size> [bold] [shadow]` | draw the names in that Windows font, `size` 4..200 px, from the next frame on (no reload); the face can be unquoted (`font Times New Roman 24`); the line step grows with the font (`scale` sizes the icon); each name keeps its team colour. Past Latin-1 the face draws the characters it has (Bahnschrift: accented Latin, Cyrillic, Greek, `™`, `€`) and Tahoma the rest, as the game would; a missing one with a plain form is written that way instead (`⅞` as `7/8`) and stays in the face. The `outline` flag is accepted but this engine's font renderer has none: use the `outline` subcommand |
| `ds_hud_deathmsg font default` | the game's own font again (`EngineFont` in `TrackerScheme.res`) |
| `ds_hud_deathmsg fallback <face> [<face> ...]` | up to 4 Windows faces to try, in order, for a character the `font` lacks, before Tahoma (e.g. `fallback "Segoe UI" Tahoma` beside `font Bahnschrift SemiBold 32 shadow`); each at the font's size, weight and shadow, and the names still measure right. One argument per face, so quote a face with a space in it; a face Windows doesn't have is left out with a note. A character with a plain form still keeps it (`⅞` as `7/8` in the font itself). Kept across `font` changes and `font default`; applies to a `font` only, not the game's own |
| `ds_hud_deathmsg fallback default` | no backup faces: Tahoma only, as the game does (the default) |
| `ds_hud_deathmsg scale <1..8>` | draw the weapon icons that many times their size (`1.5`, `2`, `3`), from the next frame on; the lines are spaced and right-aligned to match (with `scale`, `font`, `outline` or `background` in force they keep 1/160 of the screen's width clear of the right edge, or a quarter of the text's height when that is more, so a big last letter or a drop shadow is not cut), the names are centred on the line beside a taller icon, and an icon is centred on its line when the `font` is taller than it. It is the same sprite magnified (bilinear, alpha-tested), so HD HUD sprites (#728 (a)) are what make it sharp |
| `ds_hud_deathmsg scale default` | the game's own size again (`scale 1` is the same) |
| `ds_hud_deathmsg layout even` | every weapon icon in a box 26 rows tall (times `scale`), from the next frame on: every long gun's barrel on one line and its full magazine and bipod shown (a gun that doesn't fit is drawn smaller, its barrel kept on the line), pistols and launchers at their own size on that line, knives and grenades centred, so every line steps the same and the names are centred on it; each box is only as wide as its icon's art, so the names sit as close to a stick grenade as to a rifle (`gap` sets that space). It draws from DoD Studio's own atlas (`sprites\dodstudio_killfeed.spr` in `dod_addon`, built by `killfeed_icons.py --even`) and only while HD is on (`ds_hd_enabled 1`) and that atlas went up with its HD files; otherwise it says why in the status and the game's layout stays. Works at `scale 1` with no `font` too |
| `ds_hud_deathmsg layout default` | the game's own boxes and spacing, with whatever icons the sheets hold (the default) |
| `ds_hud_deathmsg gap <0..64>` | with `layout even`: that many screen px (not scaled by `scale`) between each name and the icon's art, on both sides, from the next frame on. Counted from the name's `outline` when one is drawn, and on the killer's side from the `font`'s `shadow` (1 px), so the visible space stays the same with them on; the names' own side bearings (a px or two per glyph) stay as the font draws them. The atlas's boxes are trimmed to each icon's art (atlas v2, `killfeed_icons.py --even`), so it is the same for a stick grenade as for a rifle; an atlas built before keeps each icon's empty columns |
| `ds_hud_deathmsg gap default` | the game's spacing: 5 px between the killer's name and the icon, none between the icon and the victim's name (the default); with the trimmed atlas that is how far every icon now sits, as a rifle always did |
| `ds_hud_deathmsg outline <0..4> [colour] [alpha]` | outline every name that many px (default black, alpha 255), drawn as the name repeated around itself before the coloured pass; works with the game's font (always opaque there: the engine's text call takes no alpha) and with `font`. `0` is off |
| `ds_hud_deathmsg outline default` | no outline (the default) |
| `ds_hud_deathmsg background <alpha> [colour] [padding]` | one translucent box behind each line, from the killer's name to the victim's, `alpha` 0..255, colour default black, `padding` 0..64 px sideways (default 4); the box is the line's height, 2 px short so two lines' boxes don't overlap |
| `ds_hud_deathmsg background team <alpha> [padding]` | a box behind each name instead, in a dark shade (a quarter) of that name's team colour; the icon gets none |
| `ds_hud_deathmsg background allies <colour>` / `background axis <colour>` | that team's box colour for `background team` (`allies` covers the British, who are team 1 on a British map); `background allies default` goes back to the shade |
| `ds_hud_deathmsg background default` | no boxes (the default), and `allies`/`axis` back to the shade |
| `ds_hud_deathmsg clear` | empty the kill feed now (each line goes on the next frame, the way an expired one does); works in Scheduled Commands at a tick |
| `ds_hud_deathmsg clear_on restart <0\|1> [delay]` | at every new round, `delay` seconds later (default 0, up to 600), clear the lines that were up when it restarted; lines that arrive during the delay stay (see below) |
| `ds_hud_deathmsg clear_on death <0\|1> [delay]` | when the player the view belongs to dies (a POV demo's recorder, or the player an HLTV demo watches in eye), `delay` seconds later, clear the lines up at the death, that death's own line included; later lines stay |
| `ds_hud_deathmsg clear_on default` | neither (the default) |

`clear_on restart` follows DoD's `RoundState` reset, the message that starts
every round. In 176 WSOD25 demos (POV and HLTV) it came 3 s after every round
win (5 s on some servers), when a clan match went live (both teams ready, then
the 10 s count), at the start of a recording, and every 20 s while a server
said there were not enough players. A half that ends on the map's time limit
sends none: the map changes, and the next demo starts with an empty feed. A
single flag capture sends none either, and a full cap's own lines stay up
through the win: they go at the restart.

A `colour` for `outline` and `background` is three numbers `r g b`, each 0..255 (`10 40 110`), or one hex colour: `#0a286e`, the short `#0af` (each digit doubled, so `#00aaff`), or six digits without the `#` (`0a286e`). Three digits need their `#`, since `255` on its own is a number. The `#` works from the console and from a cfg line alike, e.g. `ds_hud_deathmsg background allies #0a286e`.

A `block` id is any of:

- **A slot number**, as before. It only holds for one demo, because the same
  player gets a different slot in every demo.
- **A SteamID**: the 17-digit SteamID64 (`76561197975574370`),
  `STEAM_0:0:7654321`, or SteamID3 `[U:1:15308642]`. Paste it as is: the
  console splits it at each `:`, and the hook joins it back. It is matched
  against each player's userinfo `*sid` at every death notice, so one command
  works across a whole batch of demos and survives reconnects.
  `ds_hud_deathmsg players` lists every player's slot and SteamID.
- **`self`**: the recording player in a POV demo. An HLTV demo has no
  recording player, so there `self` matches nobody, and the console says so
  once.

Mix them freely. `ds_hud_deathmsg block !self !76561197975574370` shows
only your own frags in both kinds of demo: in a POV demo both entries point
at you, and in an HLTV demo `self` drops out and the SteamID finds you.

#### `replace`: change or hide one kill (#1021)

`replace` rewrites a recorded kill's line in the kill feed: who got it, who
died, or the weapon. The motivating case is a kill the recorder stole from a
teammate, shown as the teammate's:

    ds_hud_deathmsg replace killer self victim STEAM_0:0:5550001 at 7:42 in dice-wsod25_r03 with killer STEAM_0:1:5550002

- **Before `with`: which kill.** `killer` and `victim` take an id as `block`
  does (a slot, a SteamID in any form, or `self`); at least one of them is
  needed, so a slip can't change every kill. `at` is the demo bar's clock
  (the one `ds_demo_seek_to` takes), as `m:ss`, `h:mm:ss` or seconds, and
  matches within 2 s. `in` takes a demo file name, a comma list, or `*`
  patterns (`wsod25_r03_*`), compared with the loaded demo's file name
  without its folder or `.dem`; without `in` the line applies to every demo.
  A part left out matches any.
- **After `with`: what changes.** `killer <id>`, `victim <id>`, `weapon
  <weapon>` (the names `fake` takes, or `1..43`); a part left out stays as
  recorded. Or `hide` alone, which hides just that line. The names' colours
  follow the new killer's and victim's teams.
- **Named, any order**, per the rule above.
- **Refused, with the reason:** no `killer` or `victim` before `with`;
  nothing after `with`; no `with`; a word given twice; an unknown word,
  named in the message.
- **Warned, not refused:** `self` or a slot without `in`, since each is a
  different player in each demo ("self is a different player in each POV
  demo; add `in <demo>` or use a SteamID").
- **The kill feed only**, and the console's kill line with it. The
  scoreboard's frags, the POV effects (red screen, kill sound) and the demo
  file are untouched. A new killer or victim has to be in the demo; when
  they aren't, the line shows as recorded and the console says so once.
- The first line that matches a kill decides. `block` then filters the
  feed as `replace` left it. `fake` lines are not changed. The same line
  sent twice (a batch's Initial Commands run again for every demo) is kept
  once.
- A kill played again after a seek matches again. During a stepped forward
  seek the world clock runs up to one 5 s step ahead of the kills being
  read, so the match then allows that whole step. Under `playdemo`, which
  has no demo bar, `at` uses the client's clock instead (not measured).

### Hiding HUD parts with the game's own cvars

`ds_hud_hide_element <name> <0|1>` hid one HUD element by name until #342
retired it. Where the game already has a cvar that does the job, use that;
DoD Studio adds a `ds_` setting only for what no stock cvar reaches.

| what | how to hide it |
| --- | --- |
| chat | `hud_saytext_internal 0`: the chat element draws nothing while it is 0, exactly what `ds_hud_hide_element saytext 1` did. The `hud_saytext` command toggles it. Not saved to `config.cfg` |
| objective icons and capture bars, the objective timer, the reinforcement clock | `cl_hud_objectives 0`, `cl_hud_objtimer 0`, `cl_hud_reinforcements 0` (one each; the reinforcement clock never shows while spectating). "Warmup Mode" is `ds_hud_hide_clan_text`'s |
| the kill feed | `hud_deathnotice_time 0`: each new kill expires as it arrives; a line already up stays until its own time runs out. `ds_hud_deathmsg block` hides chosen players' kills. `hud_draw 0` hides the whole HUD (not the scoreboard, the spectator bars or menus) |
| HUD icons: MG deploy, blood, bandage, the marker arrow | `ds_map_hide_asset add icon_mg_deploy` (`icon_blood`, `icon_bandage`, `hud_mapmarker`); the TNT icon is `sprites/tnt.spr`, and a map's capture-area icon is the sprite its `dod_capture_area` names (`ds_map_hide_asset list` shows the sheets drawn so far) |
| the teammate name and health under the crosshair | `ds_hud_hide_teammate_info 1` |
| HUD messages | `ds_hud_hide_map_text`, `ds_hud_hide_plugin_text`, `ds_hud_hide_clan_text`, `ds_spec_hide_text`, `ds_spec_hide_hltv_messages` |
| text menus | `ds_hud_hide_voice_menus` (voice and hand-signal menus), `ds_hud_hide_plugin_text` (every other menu: plugin menus and votes) |
| centre text | `ds_hud_hide_center_text`; the camera mode name is `ds_spec_hide_text`'s, the MG "Deploy... to Reload!" prompt `ds_hud_hide_hints`' |
| tram controls | nothing: no map in use has trams |

The `cl_hud_*` cvars and `hud_deathnotice_time` are saved: the game writes
them to `config.cfg` when it quits (unless the file is read-only), so a value
set for a capture carries into live play. Set them back afterwards, or keep
them in Studio's Initial Commands, which run before every batch.
`hud_saytext_internal` and the `ds_` settings are never saved. See
[`goldsrc_hud_elements.md`](goldsrc_hud_elements.md).

### `ds_map_clear_decals`

No arguments. Empties the engine's 4096-slot decal pool on command,
unlinking each decal from its surface first the way the engine's own remove
functions do. Nothing to do with `r_decals`. Works on the pre-Anniversary
and the 25th Anniversary `hw.dll`.
See [`goldsrc_decals.md`](goldsrc_decals.md).

### `ds_demo_seek_to` / `ds_demo_seek_by`

`ds_demo_seek_to <time>` jumps `viewdemo` playback to a world time (the
clock the editor's events list shows); `ds_demo_seek_by <time>` jumps
from where playback is, back when negative. The time is seconds (`610`,
`610.5`), `m:ss` (`10:10`) or `h:mm:ss` (`1:02:03`), quoted or not, with a
minus for going back (`ds_demo_seek_by -0:30`); anything else is refused with
a line saying which forms it takes (#928). Neither pauses, and both refuse
under `playdemo`. Pre-Anniversary and 25th Anniversary `DemoPlayer.dll`.

While the demo is still loading, a seek typed on the same line as the
`viewdemo` (`viewdemo mydemo; ds_demo_seek_to 10:05`) waits:
`will seek to 10:05 once the demo has loaded`, then
`the demo has loaded; seeking to 10:05` when it runs. Only the last one
counts, and anything done to the demo first cancels it, with
`cancelled the seek to 10:05 (you <did what>)`: another seek, pausing, a
speed change, moving the clock (a demo-bar click or drag), or loading or
closing a demo. A seek typed later while the demo is still loading is
refused, as before.

A forward jump of more than 5 seconds moves 5 seconds per frame until it lands
(#596), so the names, teams and scores of everything jumped over reach the
game; a 20-minute jump takes a few hundred frames. The console commands it
replays (a POV demo's key presses) are dropped and the keys let go on landing,
since the engine would run them one a frame long after it (#702). Anything
after the time is
a command to run once the jump has landed:
`ds_demo_seek_to 1335.3 ds_spec_target 13`. The in-game Highlights
tab's Go uses that. See [`goldsrc_viewdemo.md`](goldsrc_viewdemo.md).

### `ds_demo_reload`

No arguments. Plays the last demo started with `playdemo` or `viewdemo`
again, from the start, by running the same command with the same name. Says
so when no demo has been played this session. See `src/demo_reload.rs`.

### `ds_capture_schedule`

R&D for #434 (the hook runs a batch's commands instead of frames patched into
the demo). `load "<file>"` reads a schedule: `dodstudio-schedule 1` on the
first line, then `<seconds>`, a tab and a command per line, the seconds on the
demo player's clock (the one `ds_demo_seek_to` takes). While a `viewdemo`
demo plays, each command runs once through the engine's `pfnClientCmd`, on
the first frame at or past its time; a seek back re-arms the commands after
it, and a jump forward runs the ones it passed, late. The hook log has each
command's due time and how late it ran, and, while a schedule is loaded, every
`[dod-studio]` marker `echo`ed from any route on the same clock. `clear` drops
the schedule; bare says how far it got. Commands run this way skip the demo
command filter (#679). Source: `goldsrc-hooks/src/schedule.rs`.

### `ds_capture_batch`

Studio's **Seek Between Clips** (#434): runs a capture batch on the original
demos instead of patched copies. `start "<file>"` reads the batch file Studio
wrote (`native::patch::runtime_batch`): `dodstudio-batch 1`, then per demo a
`demo <path>` line, its `init` commands, and `at <seconds> <command>`,
`seek <seconds> <to>` and `next <seconds>` lines on the demo player's clock,
and `end` commands after the last demo, all tab-separated. Each demo is loaded
with `viewdemo`; once it has loaded, the init commands run, then each timed
line runs once on the first frame at or past its time, and where the patched
demo fast-forwarded the batch seeks (the stepped seek, #601). Nothing runs
while a seek is on its way. A demo that doesn't load in three minutes, stops,
or ends early is skipped with a `BATCH_PROBLEM` line on the events pipe.
`hold` makes the batch stop at the next clip boundary (a seek to the next
clip, or the move to the next demo, both after the clip's recording stopped)
with the demo paused and a `BATCH_HELD` line on the events pipe; `go` lets it
go on (`BATCH_GOING`). Studio's Render While Capturing sends them while
renders free space for the next clip (#861). A held batch repeats
`BATCH_HELD` every 5 s; when Studio has been gone from the events pipe for
10 s, the batch ends there, keeping its clips, and the game quits as after a
batch Studio stopped watching.
In OBS mode Studio puts an `at` line running `ds_capture_batch wait_for_recording`
at each clip's `AUDIO_SYNC`: the batch pauses the demo there with a
`BATCH_WAITING_FOR_RECORDING` line on the events pipe, and plays on when
Studio sends `recording` once OBS has started (#1109). With no `recording`
after 20 s it plays on anyway; with Studio gone from the events pipe for
10 s it ends the batch as the space hold does.
`stop` ends the batch; bare says where it is. With
`ds_capture_stay_behind` on when it starts, the game steps aside behind
the other windows (minimised if it had the foreground, which then parks it
off-screen, #723); `away` does that on its own, and Studio sends it the
moment the game takes commands, before the batch starts, unless Keep the
Game Behind Your Windows is off (then it sends `ds_capture_stay_behind 0`). Source: `goldsrc-hooks/src/batch.rs`.

### `ds_demo_review`

DoD Studio's **Review highlights** (#623): plays each queued highlight at
normal speed, from 4 s before its first kill to 2 s after its last, and
pauses for an answer: from the answer keys on the game screen
(`ds_ui_review_keys`, #660; they answer while it plays too, #1141), or, with
those off, on the window's Review tab, which it opens. When the queue's header asks for it
(`gap=` and `speed=`, #665), a stretch between two kills further apart than
`gap` seconds plays at `speed`. `start "<queue file>"` begins (Studio
sends it over the game's pipe); `yes`, `maybe` (#661: look again later) and
`no` answer with the Review tab's kill range and note and move on; `replay`, `next` (skip without answering),
`back` and `stop` move around. `range <from> <to>` and `note <text>` set the
answer from the console, for key binds. Bare prints where the review is.
While a review runs, the viewer's spectator buttons (`+attack`, `+attack2`,
`+jump`, `+duck`, `+use`) do nothing in an HLTV demo, so a stray click can't
move the camera off the highlight's player (#826), and the console refuses
the commands that move the demo or the camera (seeks, `dem_*`, `viewdemo`
and other loads, `stop`, `disconnect`, `spec_mode`, `ds_spec_target`,
`spec_autodirector`, `mirv_input`, ...) with one line saying so; a typed
`host_framerate` or `spec_autodirector_internal` is put back
(#855). The lists are in `src/review_lock.rs`; `ds_demo_review` itself
always works.
Each answer goes to Studio as a `[dod-studio] REVIEW` line on the events
pipe. See `src/review.rs`.

### `ds_hud_overview_layout`

`ds_hud_overview_layout <full|mini> <x> <y> <w> <h>` places and sizes DoD's
overview map; `default` releases one back to the engine. With no arguments,
lists both maps and what the engine currently has. See `src/overview_map.rs`'s
module doc, including why it closes while a spectated player is scoped into a
sniper (the engine's own FOV gate, not this command).

### `ds_spec_target`

`ds_spec_target <player>` puts the camera on a player while watching an
HLTV demo. The number is the one `ds_hud_deathmsg players` lists (1-32).
With no number it says who the camera is on. It refuses a number with no
player, and a first-person demo. See
[`goldsrc_spectator_camera.md`](goldsrc_spectator_camera.md).

### `ds_debug_position`

No arguments. Prints where the view is, as `spec_pos` does while the map has no
overview (once it has one, `spec_pos` prints the overview camera instead): the
eye position, pitch and yaw; the player in view (the one watched in first
person, else your own) with the origin and where the feet are (36 units below
the origin standing, 18 crouched or prone); and the map. For checking an
overview's reach in the game (#581). The line also goes to the hook log.
`goldsrc-hooks/src/position.rs`.

### `ds_debug_msglog`

`ds_debug_msglog <name>... | all | clear` dumps chosen DoD user messages and
their payloads to the log file, forwarded to the game untouched. See
`src/msglog.rs`'s module doc.

### `ds_hud_objectives`

Places the territory-flag icon row and the objective timer beside it, or
draws them bigger for high resolutions. The game draws both ~117px lower at
1080p while spectating than in a POV demo. See
[`goldsrc_objective_icons.md`](goldsrc_objective_icons.md).

| subcommand | does |
| --- | --- |
| `ds_hud_objectives offset <y>` | y the objective icons are drawn at |
| `ds_hud_objectives xoffset <x>` | x the icon row starts at |
| `ds_hud_objectives timer <y>` | y the objective timer is drawn at (no x -- the game hardcodes it) |
| `ds_hud_objectives scale <1..8>` | draw the icons (with their capture bars and player counts) and the timer that many times bigger, from the next frame (#891); each grows from its top-left corner, so `offset`, `xoffset` and `timer` still place that corner, and the game's own icon x scales with the timer so the row stays clear of it. `scale 1` or `scale default` is the game's own size. Not on the full overview map (`_cl_minimap 1`), where the icons sit at map positions |
| `ds_hud_objectives count_scale <1..8>` | draw the `1/2`-style player count under a flag that many times its size, whatever `scale` is (#943): it keeps the game's place, its top-left corner 4 px in from its icon's left and 4 px under it (both times `scale`), and grows from there. `count_scale default` (the default) draws it at `scale`, as big as the icons |
| `ds_hud_objectives <any> default` | hand that one back to the default layout: while spectating, just below the spectator bar, or at the top as in a POV demo while it's hidden |

### `ds_debug_hd_misses`

Lists every texture that kept its original this session, map by map, grouped
by why: the HD file is for a different version of the texture, there's no HD
file (naming the file that would match), the file couldn't be used, or it
was left alone on purpose (tool textures, blank sprites, per-player skins). A
texture several maps use is listed under each. `ds_debug_hd_misses <map>`
shows one map; `ds_debug_hd_misses clear` forgets the list.

### `ds_player_spray`

Player sprays and crosshairs per player in demos, and switching your own
spray (#428). Who gets what is Studio's **Configuration > Players**: a table
of players (by SteamID, or by name when there is none), each with a spray (a
PNG from the game's own `<game>\logos\` folder, any size, RGBA, or **From
the demo**) and a crosshair (a 64x64 tile of a sprite in `<game>\sprites\`, picked from a 4x4
preview), and what everyone not listed gets:

- **Sprays, POV demos:** their own spray (the default: the one the demo
  saved), one fallback PNG, or no spray;
- **Sprays, HLTV demos:** one fallback PNG (the default) or no spray. "Their
  own" is not offered: an HLTV demo records that a player sprayed, never the
  image.
- **Crosshair:** the game's own (the default: the viewer's `cl_xhair_style`
  tile of `customXHair.spr`, as without the table) or one sprite and tile.

Studio mirrors the table to `%APPDATA%\dod-studio\goldsrc_hooks_players.txt`
whenever it changes (it never writes into the game install for this); the
hook rereads it when its time changes, checked once a second, and applies it
only while a demo plays and `ds_player_overrides` is 1. Players are
looked up through the engine's player info: SteamID first, then the name,
ignoring case.

**Sprays.** One rule for both demo types, and who the camera follows plays
no part: when a player sprays, the wall shows the PNG the table gives that
player, or the fallback.

- **Every spray from a PNG** -- a listed player's, and every fallback PNG --
  is a spray the hook builds from the PNG and shoots in place of the
  engine's, whether or not the demo saved one for that player: the same in
  every demo, at the Players tab's **Spray size** (the longer side, 64 by
  default, 96 or 128; the picture's shape kept). It needs no HD textures;
  with them on, its upload is swapped for the PNG itself at the largest
  power of two `gl_max_size` allows, drawn at the built size. A size change,
  like any other spray change, reaches sprays built after the table is
  read again.
- **"No spray"** is a fully transparent spray, shot the same way. It is
  drawn rather than skipped because skipping would need another patch
  inside `CL_ParseTEnt` on both builds; this path is the one already checked
  on both.
- **"From the demo"** (a listed player) and **"Their own spray"** (the POV
  fallback) draw the spray the demo saved, untouched, at its real size.
  With none saved (the player joined before the recording, or any HLTV
  demo), "From the demo" is the fallback for that demo type, and "Their own
  spray" is no spray: never the engine's bullet-hole stand-in.

How: `CL_ParseTEnt`'s `customdata` test asks the hook for every spray, and
`Draw_DecalTexture` answers each frame with the hook's texture for that
player, from one table for players with no saved spray and one for players
whose saved spray the table replaces (`goldsrc-hooks/src/spray_hltv.rs`;
`tools/verify_hltv_spray_offsets.py`). These detours install once the table
asks for anything and the cvar is 1. The log says `spray: TE_PLAYERDECAL for
<player> (slot <n>, ...) -> <spray>` for each one drawn.

A flip of `ds_player_overrides` reaches sprays loaded after it; a
spray already on a wall keeps its texture, so reload the demo to compare.

**Crosshairs.** The crosshair on screen is the recorder's in a POV demo and
the watched player's in an HLTV demo's in-eye view (free and chase cameras
draw none). It changes on the next frame. No code is patched: the hook
already sits on the engine's `SPR_Set`/`SPR_Draw*` in client.dll's own
`gEngfuncs` copy (`hud_sprites.rs`, #808), and when a draw is the
crosshair's -- any rect of `customXHair.spr`, or exactly the stock 24x24
spectator rect of `crosshairs.spr` while in-eye -- it sets the table's
sprite (loaded once per level with the engine's `SPR_Load`, after checking
the file is a Half-Life sprite), draws its tile centred where the game
centred its own, and sets the game's sprite back. When the crosshair shows
stays the game's: `ShouldDrawCrossHair`, `ds_hud_hide_crosshair` and
`ds_spec_match_pov`'s hides are untouched. In a POV demo it needs the
viewer's `cl_xhair_style` at 1-16: style 0 is DoD's classic crosshair, which
draws no tile to swap. Details: `docs/goldsrc_crosshair.md` §3.

| subcommand | does |
| --- | --- |
| `ds_player_spray` | lists the PNGs in `logos\` and your spray's size |
| `ds_player_spray <name>` | makes `logos\<name>.png` your spray for live play: writes `dod\tempdecal.wad` (the game's spray file -- the one file in the game's folder the hook writes, because that is what the command is for; sides in steps of 16 and the file at most 20480 bytes, the most a server takes, so a square picture is 112x112; 8-bit, 255 colours, transparent where the PNG is) and keeps `tempdecal.wad.dodstudio-backup` of the original the first time. The engine reads the file when it connects, so it shows from the next connect; a demo already playing draws the sprays it recorded |
| `ds_player_spray players` | each player in the demo with their name, SteamID, whether the Players table lists them and the spray they get, then whether the detours are on |

`cl_logofile` and the game's `.cfg` files are left alone. Refused in a
capture batch's Initial and Scheduled Commands (`docs/command_tiers.md`): a
batch draws recorded sprays and would only change the file on disk.
Source: `goldsrc-hooks/src/spray.rs`, `goldsrc-hooks/src/spray_hltv.rs`
(offsets checked by `goldsrc-hooks/tools/verify_hltv_spray_offsets.py`).

### `ds_player_change_name`

Renames a player everywhere their name shows during playback: the kill
feed, the scoreboard, the aim-at-teammate name, the voice icon, chat, the
capture lines and the join and team lines (#1021). The demo files stay as
they are.

```
ds_player_change_name add id <slot | SteamID | self> [in <demo>[,<demo>...]] to <new name>
ds_player_change_name list
ds_player_change_name remove <n>
ds_player_change_name clear
```

- **`id`**: a SteamID in any form `ds_hud_deathmsg block` takes
  (`STEAM_0:1:5550002`, `[U:1:11100005]`, `7656119...`; `ds_hud_deathmsg
  players` lists them), a slot (1-32), or `self`, the recording player of a
  POV demo. Bots have no SteamID, so they go by slot.
- **`in`**: one demo, a comma list, or `*` patterns, matched against the
  loaded demo's file name without `.dem` (`in wsod25_r03_*`). Without `in`,
  every demo.
- **`to`**: the new name, up to 63 characters; quoted when it has spaces or
  any of `: , ( ) { } '`. No `%`. Unquoted, the console splits a name at
  `:` and `,` and loses the spaces beside them (`dicE[: :]Alex` arrives as
  `dicE[::]Alex`), so the pieces are glued back with a warning that shows
  the name as it now reads.
- `self` or a slot without `in` gets a warning, not a refusal: "self is a
  different player in each POV demo; add `in <demo>` or use a SteamID".
- Refused, with the reason: `add` without `id` and `to`, any unknown word, a
  value given twice, a bad id, a name too long.
- When two renames match one player, one with `in` wins over one without,
  then the one added last. `add` with the same `id` and `in` again changes
  that rename's name instead of adding another.
- `list` numbers the renames and says who each one renames in the demo
  playing now; `remove <n>` takes that number.

How: a detour of `client.dll`'s copy of `pfnGetPlayerInfo` (slot 21), which
hands out the new name; the names the hook itself looks up go through the
engine's own table, so SteamID matching always sees the real player. The
server wrote the sender's name into each chat line, so `SayText` is rewritten
too (the game then colours the new name, since it finds the detour's name in
the line), and a `TextMsg` argument equal to a renamed player's real name
(`*<name> joined Axis`) is swapped. A name over 31 characters lifts the kill
feed's copy cap (`strncpy(..., 32)` into its 64-byte fields) to 63; checked by
`goldsrc-hooks/tools/verify_killfeed_name_cap_offsets.py`. Source:
`goldsrc-hooks/src/player_names.rs`, `goldsrc-hooks/src/deathmsg/name_cap.rs`.

### `ds_map_hide_asset`

`ds_map_hide_asset` stops the game drawing an asset -- a sprite, model
or brush entity, named by its file path -- here, specific world entities by exact model
path: map sprites, props (`.mdl`), brush entities (`*12`). It keeps a list,
shaped like HLAE's `mirv_matte_entities` but by model path rather than entity
number (a path stays the same across demos):

- `ds_map_hide_asset list` (or no arguments): what is hidden.
- `ds_map_hide_asset add <model-path>...`: hide these too.
- `ds_map_hide_asset del <model-path>...`: stop hiding these.
- `ds_map_hide_asset clear`: stop hiding anything.

It is an allow-list, not a blanket toggle: `all` is refused. It was
`ds_hide_sprite`, then `ds_hide_entity` (#333); neither was in
a release, and both names are gone. "Asset" because #614 extends it to the
same paths drawn as temporary effects and particles.

It checks the same list in each place DoD draws a model or sprite (#614):
world entities and the engine's temporary effects (shell casings, blood
sprites, popped helmets) in `HUD_AddEntity`, and DoD's own particles (impact
debris, the grenade dirt cloud `sprites/effects/adrian/dirt_puff.spr`, smoke
trails, splashes, snow) where they are made. The status says where each path
was seen: `seen as an entity`, `an effect` or `a particle`. Since #808 the HUD's own 2D sprites are reachable too, through client.dll's
`SPR_Load`/`SPR_Set`/`SPR_Draw*` engine-function pointers: name the sheet
(`ds_map_hide_asset add sprites/hud_icons.spr` hides everything drawn from
it) or one icon by its name in `sprites/hud.txt` (`ds_map_hide_asset add
icon_bandage`; the file names every icon with its sheet and rectangle, and the
whole rectangle is the key, since `number_0` and `icon_mg_deploy` share a
corner). A name that is not in `hud.txt` is refused. `list` ends with the
sheets the HUD has drawn so far and the `hud.txt` names each holds, so there
is no need to open the file. The crosshair and whole HUD elements still have
their own switches (`ds_hud_hide_crosshair`, `ds_hud_hide_teammate_info`). The
status, bare or in `ds_debug_status`, says whether each path has
matched anything this session, so a typo no longer fails silently. See
`src/hide_asset.rs`'s module doc.

### `ds_ui_panel`

Opens DoD Studio's own window in the game, or closes it if it's open. It sits
with GameUI's windows, so press ESC for the menu to see it. Real tabs, like the
Options and Find Servers windows (Playback, Demos, Highlights, Console,
Settings, Commands, Studio), and never
narrower than its tabs. The Playback buttons do what the VCR bar's do, so a
demo has to be playing under `viewdemo`; the tab also shows the bar's own time
slider and time label, and while the demo player is still reading the demo,
how far it has got (#465). The Console tab holds the real console's history, input
line and Submit button (Enter submits). Both are the original controls, lent
by their windows while ours is open and handed back when it closes.
The Demos tab lists the demos, with each one's map and date, and loads one
with no demo playing. Click a column heading to sort by it; the boxes above
filter by name or map (Search), map, HLTV or POV, and age in days.
The Highlights tab lists every life with a kill in the demo playing
(player, kills, weapons, time). A POV demo lists only the recording player's,
named in a greyed-out Player box; an HLTV demo lists everyone's, the Player box
narrows them, and Go also puts the camera on that player (`ds_spec_target`). Go or a
double-click jumps to 5 s before the first kill. It reads Studio's analyzer cache
(`%APPDATA%\dod-studio\analyzer_cache`), so a demo Studio's Demo Analyzer or
Master Queue already read shows at once; anything else is analysed in the
game, with a progress bar, and saved there for Studio too. That analysis
starts as soon as a demo plays, with the window open or not; while it or the
demo player's own load still runs, a strip along the top of every tab says how
far each has got (`Loading: Demo 45%   Highlights 60%`, #797). A demo too big for
the game's memory (analysis takes about 12 times the demo's size; the
pre-Anniversary game has under 1 GB free) is refused with a note to open it in
the Demo Analyzer instead. On the
Settings tab, each check box named `cvar_<name>` is bound to that cvar: it
shows the value and sets it when clicked (add more in build mode).
`ds_ui_panel [1|0|reset|<tab>]`: bare opens or closes it, `1` opens, `0`
closes, `reset` writes the default layouts back and rebuilds the window, and a
tab's name (`ds_ui_panel highlights`) opens it on that tab. With
`ds_ui_viewdemo_in_panel 1`, a bare `viewdemo` opens the Playback tab.

The hook also writes DoD Studio's main menu to `dod_addon\resource\GameMenu.res`
(DoD Studio, Resume/Disconnect in a demo, Options, Quit), shown when the game
is launched with `-addons` (#412); `dod\resource`'s menu is never written, and a
`dod_addon` menu without the "DoD Studio" mark is left alone.

The layouts are in `dod_addon\dodstudio_ui\` (or `dod\dodstudio_ui\` when the game has no `-addons`, or while only that one exists, as before #415): `DodStudio.res` for the window and
one per tab (`Playback.res`, `Demos.res`, `Console.res`, `Studio.res`, `Review.res`), written
the first time and never overwritten. The empty `...Slot` controls in
`Playback.res` and `Console.res` mark where the lent controls go. Edit a tab in-game with Ctrl+Shift+Alt+B on it, then
Save. A button's `Command` can be a VCR command (`play`, `pause`, `faster`,
`slower`, `stepf`, `stepb`, `start`, `end`, `stop`, `load`, `events`, `save`)
or `engine <console command>`. See `src/studio_panel.rs` (#408).

## Stock settings the hook extends

- **`hud_fastswitch 1` also keeps the weapon list hidden in demo playback (#1032).** A POV demo replays the player's number keys, and the game's own `SelectSlot` opens the weapon list even with fast switch when the key's slot holds two weapons. In DoD that happens to the grenade key after a primed grenade is caught with USE: the caught one becomes a weapon of its own beside the class's grenade. While a demo plays with `hud_fastswitch 1`, the hook skips drawing the list (one code byte in `CHudAmmo::DrawWeaponList`); the weapon still switches as recorded. With `hud_fastswitch 0` playback shows the list as the player saw it, and live play is unchanged either way. `ds_debug_status` gets a `weapon list:` line once it has hidden it. `goldsrc-hooks/src/weapon_list.rs`.

## Renamed from `dodstudio_` (#555)

Every name above used to start `dodstudio_`. #555 renamed all 60: the prefix is
now `ds_` (shorter to type, and nothing in the engine, DoD's client or HLAE has
a name starting with it), and the word after it is the area the name belongs
to, as HLAE's `mirv_movie_*` are: `ds_spec_` (watching another player),
`ds_hud_` (drawn on screen), `ds_view_` (the recording player's own
first-person view), `ds_player_` (the players, as everyone sees them), `ds_map_`
(the level), `ds_sound_`, `ds_demo_` (playback), `ds_capture_` (capture
batches), `ds_ui_` (DoD Studio's window and the game's windows), `ds_hd_` and
`ds_debug_`. Subcommands and their options did not change
(`ds_hud_deathmsg font ...`, `ds_capture_batch start`). The table below is the
whole list; the signed-off one, with the names open PRs add, is on #555.

- **The old names stop working.** The game has no aliases for them, so a
  `dodstudio_` line typed in the console, or left in a config, does nothing.
- **Studio's own saved lists move by themselves:** Initial Commands, Scheduled
  Commands and every command profile are rewritten to the new names the
  first time the new version loads its settings.
- **The in-game window's saved settings move too** (`dodstudio_ui\settings.cfg`,
  DoD Studio's own file): the hook reads a line with an old name as its new
  name and writes the file back with the new names.
- **Your own `.cfg` files are never changed.** The Commands tab's warnings list
  every line in `config.cfg`, `userconfig.cfg`, `autoexec.cfg` and the configs
  they `exec` (such as `movie.cfg`) that still uses an old name, with the name
  that replaced it. Typing an old name in Initial or Scheduled Commands
  suggests the new one.

The old -> new list the app uses is `goldsrc-hooks/src/old_names.rs` (Studio's
copy: `studio/src/console_renames.js`). Folder and file names that start with
`dodstudio_` (`dodstudio_hd`, `dodstudio_ui`, `dodstudio_helper.cfg`,
`dodstudio_goldsrc_hooks.dll`, the hook log) did not change.

| old name | new name |
| --- | --- |
| `dodstudio_batch` | `ds_capture_batch` |
| `dodstudio_run_in_background` | `ds_capture_run_in_background` |
| `dodstudio_schedule` | `ds_capture_schedule` |
| `dodstudio_stay_behind_while_capturing` | `ds_capture_stay_behind` |
| `dodstudio_debug_cloak_instead` | `ds_debug_cloak_instead` |
| `dodstudio_debug_hd_misses` | `ds_debug_hd_misses` |
| `dodstudio_debug_keep_detail_cache` | `ds_debug_keep_detail_cache` |
| `dodstudio_debug_log_spectator_target` | `ds_debug_log_spectator_target` |
| `dodstudio_debug_log_texture_loads` | `ds_debug_log_texture_loads` |
| `dodstudio_debug_log_weapon_model` | `ds_debug_log_weapon_model` |
| `dodstudio_debug_msglog` | `ds_debug_msglog` |
| `dodstudio_debug_position` | `ds_debug_position` |
| `dodstudio_debug_status` | `ds_debug_status` |
| `dodstudio_ex_interp_max` | `ds_demo_ex_interp_max` |
| `dodstudio_demo_fast_load` | `ds_demo_fast_load` |
| `dodstudio_demo_other_drives` | `ds_demo_other_drives` |
| `dodstudio_reload_demo` | `ds_demo_reload` |
| `dodstudio_review` | `ds_demo_review` |
| `dodstudio_seek_by` | `ds_demo_seek_by` |
| `dodstudio_seek_skip_between` | `ds_demo_seek_skip_between` |
| `dodstudio_seek_to` | `ds_demo_seek_to` |
| `dodstudio_hd_enabled` | `ds_hd_enabled` |
| `dodstudio_hd_style` | `ds_hd_style` |
| `dodstudio_deathmsg` | `ds_hud_deathmsg` |
| `dodstudio_hide_clan_text` | `ds_hud_hide_clan_text` |
| `dodstudio_hide_crosshair` | `ds_hud_hide_crosshair` |
| `dodstudio_hide_hudelement` | `ds_hud_hide_element`, retired in #342 |
| `dodstudio_hide_map_text` | `ds_hud_hide_map_text` |
| `dodstudio_hide_scoreboard` | `ds_hud_hide_scoreboard` |
| `dodstudio_hide_voice_menus` | `ds_hud_hide_voice_menus` |
| `dodstudio_objectives` | `ds_hud_objectives` |
| `dodstudio_overviewmap` | `ds_hud_overview_layout` |
| `dodstudio_hud_map_players` | `ds_hud_overview_players` |
| `dodstudio_hud_map_team_marker` | `ds_hud_overview_team_marker` |
| `dodstudio_show_demo_scope` | `ds_hud_show_scope` |
| `dodstudio_allow_shaders` | `ds_map_allow_shaders` |
| `dodstudio_clear_decals` | `ds_map_clear_decals` |
| `dodstudio_hide_asset` | `ds_map_hide_asset` |
| `dodstudio_hide_hand_signals` | `ds_player_hide_hand_signals` |
| `dodstudio_player_overrides` | `ds_player_overrides` |
| `dodstudio_spray` | `ds_player_spray` |
| `dodstudio_mute_voice_commands` | `ds_sound_mute_voice_commands` |
| `dodstudio_spec_deathcam` | `ds_spec_deathcam` |
| `dodstudio_hide_spectator_bars` | `ds_spec_hide_bars` |
| `dodstudio_hide_hltv_messages` | `ds_spec_hide_hltv_messages` |
| `dodstudio_spec_lock` | `ds_spec_lock` |
| `dodstudio_spec_match_pov` | `ds_spec_match_pov` |
| `dodstudio_spec_target` | `ds_spec_target` |
| `dodstudio_console_in_panel` | `ds_ui_console_in_panel` |
| `dodstudio_demo_list_count_subfolders` | `ds_ui_demo_list_count_subfolders` |
| `dodstudio_demo_list_folders` | `ds_ui_demo_list_folders` |
| `dodstudio_demo_list_hide_empty` | `ds_ui_demo_list_hide_empty` |
| `dodstudio_demo_list_show_counts` | `ds_ui_demo_list_show_counts` |
| `dodstudio_panel` | `ds_ui_panel` |
| `dodstudio_remember_window_layout` | `ds_ui_remember_layout` |
| `dodstudio_resizable_windows` | `ds_ui_resizable_windows` |
| `dodstudio_viewdemo_in_panel` | `ds_ui_viewdemo_in_panel` |
| `dodstudio_hide_headshot_red_screen` | `ds_view_hide_headshot_red_screen` |
| `dodstudio_hide_screen_shake` | `ds_view_hide_screen_shake` |
| `dodstudio_pov_live_bob` | `ds_view_use_cl_bob` |

## Keeping this in sync

Manual, like the rest of this crate's docs -- there is no generator from
`commands.rs`. Each PR that adds or changes a `ds_*` entry should
update this file the same way it already updates `README.md`'s own
control-surface list; the two lists should never drift apart. Issue #313
left open whether this eventually becomes a generated table or a GitHub wiki
page instead -- not resolved here.
