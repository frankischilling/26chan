// Recovered timezone behavior; the form remains usable without scripts.
(() => {
  "use strict";
  function run() {
    if (!document.getElementById("application-form")) return;
    const sel = document.getElementById("js-tz");
    if (!sel || sel.hasAttribute("data-edit")) return;
    const hd = -(0 | (new Date().getTimezoneOffset() / 60));
    for (let i = 0; i < sel.options.length; i++) {
      if (hd == sel.options[i].value) { sel.selectedIndex = i; return; }
    }
  }
  document.addEventListener("DOMContentLoaded", run, false);
})();
