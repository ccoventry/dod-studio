// app_menu.js — File/Help dropdown menu bar (#122).
//
// Custom in-app dropdowns, not Tauri's native Menu/MenuBuilder — Windows
// renders native menus with system chrome and there's no API to recolor
// them (tauri-apps/tauri#6645 is the closest unresolved upstream issue, for
// the analogous system-tray-menu case), so a native bar would look like
// stock Windows chrome instead of matching the app's theme.
//
// Save Session/Load Session/Check for Updates/View Logs keep the exact ids
// they had before this menu existed (#save-project-btn, #load-project-btn,
// #update-check-btn, #view-logs-btn) — they just moved into a
// .menu-dropdown-panel in index.html. Their click handlers, defined in
// main.js/updater_pane.js, needed no changes at all.

export function initAppMenu() {
  const menus = document.querySelectorAll('.menu-dropdown');

  function closeAll() {
    menus.forEach((m) => m.classList.remove('open'));
  }

  menus.forEach((menu) => {
    const btn = menu.querySelector('.menu-dropdown-btn');
    btn?.addEventListener('click', (e) => {
      e.stopPropagation();
      const isOpen = menu.classList.contains('open');
      closeAll();
      menu.classList.toggle('open', !isOpen);
    });
    // Standard menu-bar behaviour: once any menu is open via a click,
    // moving the mouse onto a sibling menu's button switches which one is
    // open, rather than requiring a second click.
    btn?.addEventListener('mouseenter', () => {
      if (menu.classList.contains('open')) return;
      const anotherIsOpen = [...menus].some((m) => m !== menu && m.classList.contains('open'));
      if (anotherIsOpen) {
        closeAll();
        menu.classList.add('open');
      }
    });
    // Any click inside the panel — including on an item — closes the menu
    // after the item's own listener (registered elsewhere) has run.
    menu.querySelector('.menu-dropdown-panel')?.addEventListener('click', () => closeAll());
  });

  document.addEventListener('click', () => closeAll());
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeAll();
  });

  const aboutModal = document.querySelector('#about-modal');
  document.querySelector('#about-menu-btn')?.addEventListener('click', () => {
    if (aboutModal) aboutModal.style.display = 'flex';
  });
  document.querySelector('#about-modal-close-btn')?.addEventListener('click', () => {
    if (aboutModal) aboutModal.style.display = 'none';
  });
}
