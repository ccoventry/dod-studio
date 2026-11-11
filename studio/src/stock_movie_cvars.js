// stock_movie_cvars.js
// The game's own cvars people use for movies, with a one-line description
// shown beside them in Initial and Scheduled Commands (#1087). Where a stock
// cvar does the job, DoD Studio adds no setting of its own (#342), so the
// list has to be findable. Effects and saving come from
// docs/goldsrc_hud_elements.md, and for DoD's fog and atmosphere cvars from
// docs/goldsrc_client_dll_survey.md section 7 (#1131); a cvar whose effect is
// not documented there is left out rather than guessed.

/** name -> what it does. `saved` is true when the game writes the value to
 *  config.cfg on quit, so it carries into live play; false when it never
 *  does; absent when not measured, and then the line says nothing about it. */
export const STOCK_MOVIE_CVARS = {
  hud_saytext_internal: { text: 'Chat: 0 hides it.', saved: false },
  cl_hud_objectives: { text: 'Objective icons and capture bars: 0 hides them.', saved: true },
  cl_hud_objtimer: { text: 'Objective timer: 0 hides it.', saved: true },
  cl_hud_reinforcements: { text: 'Reinforcement clock: 0 hides it (it never shows while spectating).', saved: true },
  hud_deathnotice_time: { text: 'Kill feed: 0 hides new kills.', saved: true },
  hud_draw: { text: 'Whole classic HUD: 0 hides it (not the scoreboard, spectator bars or menus).' },
  // Fog and atmosphere (#1131). The fog's colour and density cvars are left
  // out: cl_fog 1's fog is always grey and linear, and cl_fog 2 drew nothing.
  cl_fog: { text: 'Distance fog: 1 a grey fog from cl_fog_start to cl_fog_end, 0 none.', saved: true },
  cl_fog_start: { text: 'Distance where the cl_fog 1 fog begins, in units (default 10).', saved: true },
  cl_fog_end: { text: 'Distance where the cl_fog 1 fog is solid grey, in units (default 3500; 900 is thick).', saved: true },
  cl_weatherdis: { text: 'How far from the player particle effects are still made, in units (default 1700, at least 100).', saved: true },
  cl_bulletejects: { text: 'Shell casings from fired guns: 0 hides them.', saved: true },
  cl_dynamiclights: { text: 'Dynamic lights (muzzle flashes, explosions): 0 turns them off.', saved: true },
  cl_corpsestay: { text: 'Seconds a dead body lies before it sinks into the ground (default 10).', saved: true },
  cl_particlefx: { text: 'Particle effects: 0 turns them off. Rain and snow only show at 2.', saved: true },
  room_off: { text: 'Room reverb on sounds: 1 turns it off.' },
};

/** The one-line hint for `name`, or '' when it is not a listed movie cvar. */
export function stockMovieHint(name) {
  const e = STOCK_MOVIE_CVARS[name];
  if (!e) return '';
  const saved = e.saved === true ? ' Saved to config.cfg.' : e.saved === false ? ' Not saved.' : '';
  return `${e.text}${saved}`;
}
