// step_bar.js
// A bar for a job made of steps whose insides can't be measured (#956): the
// steps done so far fill it, and a sliding block runs over the running
// step's share only, so the bar shows where the job is even while a step
// goes a minute without saying anything (an HD build step, an unpack).
// `progress_view.js`'s `showBar` slides over the whole bar instead.
import { showBar, hideBar } from './progress_view.js';

/// Where a step bar's fill ends and its sliding block sits, in percent:
/// `done` of `total` steps finished, and the next `share` steps (one by
/// default) running. A `fraction` (0..1) of the running step that is known
/// moves the fill into it, and the block covers what's left of the step.
export function stepBarLayout(done, total, { fraction = null, share = 1 } = {}) {
  const steps = Math.max(1, Number(total) || 0);
  const finished = Math.max(0, Math.min(steps, Number(done) || 0));
  const part = Math.max(0, Math.min(1, Number(fraction) || 0));
  const fill = ((finished + part * share) / steps) * 100;
  const end = Math.min(100, ((finished + share) / steps) * 100);
  return {
    fill: Math.min(100, fill),
    // Nothing slides once every step is done, or when the fraction is known.
    slideFrom: fraction == null && finished < steps ? Math.min(100, fill) : null,
    slideWidth: fraction == null && finished < steps ? Math.max(0, end - fill) : 0,
  };
}

/// Draws `stepBarLayout`'s result on a `.progress-bar-container`.
export function showStepBar(bar, done, total, options = {}) {
  const { fill, slideFrom, slideWidth } = stepBarLayout(done, total, options);
  showFillAndSlide(bar, fill, slideFrom, slideWidth);
}

/// The fill at `fill` percent, and the sliding block over `slideWidth`
/// percent from `slideFrom`, or no block when `slideFrom` is null. For a
/// bar whose shares aren't equal steps (the capture batch's, #952).
export function showFillAndSlide(bar, fill, slideFrom = null, slideWidth = 0) {
  if (!bar) return;
  bar.hidden = false;
  bar.classList.remove('indeterminate');
  bar.classList.add('step-bar');
  const fillEl = bar.querySelector('.progress-bar-fill');
  if (fillEl) fillEl.style.width = `${fill}%`;
  let slide = bar.querySelector('.progress-bar-step');
  if (slideFrom == null) {
    slide?.remove();
    return;
  }
  if (!slide) {
    slide = document.createElement('div');
    slide.className = 'progress-bar-step';
    bar.appendChild(slide);
  }
  slide.style.left = `${slideFrom}%`;
  slide.style.width = `${slideWidth}%`;
}

/// Takes the running step's block off a bar `showStepBar` drew.
export function clearStepBar(bar) {
  if (!bar) return;
  bar.classList.remove('step-bar');
  bar.querySelector('.progress-bar-step')?.remove();
}

/// Draws a bar from what a view function says it should be: `null` hides
/// it, `{ slide: true }` slides over the whole bar (nothing to measure yet),
/// `{ pct }` fills it, and `{ done, total, fraction? }` is a step bar.
export function drawBar(bar, view) {
  if (!bar) return;
  if (!view) {
    clearStepBar(bar);
    hideBar(bar);
  } else if (view.slide) {
    clearStepBar(bar);
    showBar(bar, { indeterminate: true });
  } else if (view.pct != null) {
    clearStepBar(bar);
    showBar(bar, { pct: view.pct });
  } else {
    showStepBar(bar, view.done, view.total, { fraction: view.fraction ?? null });
  }
}
