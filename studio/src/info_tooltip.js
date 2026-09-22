// info_tooltip.js
//
// Custom hover/focus tooltip for `.info-icon` elements. Replaces the native
// `title` attribute for this pattern: native tooltips require the cursor to
// sit essentially motionless on a target for ~1s and silently reset that
// timer on any tiny movement, which on a 13x13 icon reads as "the tooltip
// just doesn't show" (see #164). apply_strings.js stops writing `title` for
// `.info-icon` elements and stashes the resolved string in
// `data-tooltip-text` instead; this module renders it in a single shared,
// viewport-clamped bubble on hover or keyboard focus.

let tooltipEl = null;
let activeTarget = null;

function ensureTooltipEl() {
  if (tooltipEl) return tooltipEl;
  tooltipEl = document.createElement('div');
  tooltipEl.className = 'info-tooltip';
  document.body.appendChild(tooltipEl);
  return tooltipEl;
}

function positionTooltip(target) {
  const tip = ensureTooltipEl();
  const rect = target.getBoundingClientRect();
  const tipRect = tip.getBoundingClientRect();
  let left = rect.left + rect.width / 2 - tipRect.width / 2;
  left = Math.max(6, Math.min(left, window.innerWidth - tipRect.width - 6));
  let top = rect.top - tipRect.height - 8;
  if (top < 6) top = rect.bottom + 8; // flip below when clipped at the top
  tip.style.left = `${left}px`;
  tip.style.top = `${top}px`;
}

function showTooltip(target) {
  const text = target.dataset.tooltipText;
  if (!text) return;
  const tip = ensureTooltipEl();
  tip.textContent = text;
  tip.classList.add('visible');
  activeTarget = target;
  positionTooltip(target);
}

function hideTooltip() {
  if (!tooltipEl) return;
  tooltipEl.classList.remove('visible');
  activeTarget = null;
}

export function initInfoTooltips(root = document) {
  root.querySelectorAll('.info-icon').forEach((el) => {
    if (el.dataset.tooltipBound) return;
    el.dataset.tooltipBound = '1';
    if (!el.hasAttribute('tabindex')) el.tabIndex = 0;
    el.addEventListener('mouseenter', () => showTooltip(el));
    el.addEventListener('mouseleave', hideTooltip);
    el.addEventListener('focus', () => showTooltip(el));
    el.addEventListener('blur', hideTooltip);
  });
}

window.addEventListener('scroll', () => {
  if (activeTarget) positionTooltip(activeTarget);
}, true);
