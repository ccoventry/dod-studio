// stock_movie_cvars.js
// The game's own cvars people use for movies, with a one-line description
// shown beside them in Initial and Scheduled Commands (#1087). Where a stock
// cvar does the job, DoD Studio adds no setting of its own (#342), so the
// list has to be findable. Effects and saving come from
// docs/goldsrc_hud_elements.md; a cvar whose effect is not documented there
// is left out rather than guessed.

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
};

/** The one-line hint for `name`, or '' when it is not a listed movie cvar. */
export function stockMovieHint(name) {
  const e = STOCK_MOVIE_CVARS[name];
  if (!e) return '';
  const saved = e.saved === true ? ' Saved to config.cfg.' : e.saved === false ? ' Not saved.' : '';
  return `${e.text}${saved}`;
}
