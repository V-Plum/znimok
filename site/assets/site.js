// Znimok download page (ZK-83): the latest release's installers, and the visitor's system first.
// Without a release (or without the network) the buttons lead to the releases page and a note
// says the first release is not out yet.
(function () {
  "use strict";
  var REPO = "V-Plum/znimok";
  var page = document.documentElement;
  var t = {
    uk: { none: "Перший реліз ще не вийшов — стежте за сторінкою релізів на GitHub.", version: "Версія " },
    en: { none: "The first release is not out yet — watch the releases page on GitHub.", version: "Version " }
  }[page.lang === "uk" ? "uk" : "en"];

  // The visitor's system goes first and gets the accent.
  var mac = /Mac/.test(navigator.platform || navigator.userAgent);
  var win = document.getElementById("dl-windows");
  var macBtn = document.getElementById("dl-macos");
  if (mac && win && macBtn) {
    macBtn.parentNode.insertBefore(macBtn, win);
    macBtn.classList.add("primary");
    win.classList.remove("primary");
  }

  var note = document.getElementById("dl-note");
  fetch("https://api.github.com/repos/" + REPO + "/releases/latest", {
    headers: { Accept: "application/vnd.github+json" }
  })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(function (rel) {
      if (!rel || !rel.assets) {
        if (note) note.textContent = t.none;
        return;
      }
      rel.assets.forEach(function (a) {
        if (/windows-x64\.msi$/.test(a.name) && win) win.href = a.browser_download_url;
        if (/macos-arm64\.dmg$/.test(a.name) && macBtn) macBtn.href = a.browser_download_url;
      });
      if (note) note.textContent = t.version + rel.tag_name.replace(/^v/, "");
    })
    .catch(function () {
      if (note) note.textContent = t.none;
    });
})();
