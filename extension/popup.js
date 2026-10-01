// The popup: whether Znimok is there and recording; «Record this window» (CAPS-107 — while a
// recording runs there is no popup: a click on the icon stops it).
// Texts from _locales (ZK-182): the browser picks the language (English by default, Ukrainian).
const M = (k) => chrome.i18n.getMessage(k);
const T = {
  title: M("popupTitle"), app: M("popupApp"), rec: M("popupRec"),
  on: M("popupOn"), off: M("popupOff"), recOn: M("popupRecOn"), recOff: M("popupRecOff"),
  go: M("popupGo"), busy: M("popupBusy"), page: M("popupPage"),
  hintOff: M("hintOff"), hintOn: M("hintOn"), hintLog: M("hintLog"),
  why: {
    "no-app": M("whyNoApp"), disabled: M("whyDisabled"), busy: M("whyBusy"),
    "not-found": M("whyNotFound"), ambiguous: M("whyAmbiguous"), minimized: M("whyMinimized"),
    unsupported: M("whyUnsupported"), timeout: M("whyTimeout"), failed: M("whyFailed"),
  },
};
document.querySelectorAll("[data-i]").forEach((e) => { e.textContent = T[e.dataset.i]; });
const go = document.getElementById("go"), page = document.getElementById("page"), err = document.getElementById("err");
chrome.storage.local.get("pageOnly").then(({ pageOnly = true }) => { page.checked = pageOnly; });
page.addEventListener("change", () => chrome.storage.local.set({ pageOnly: page.checked }));

function show(st) {
  const c = document.getElementById("conn"), r = document.getElementById("rec");
  c.textContent = st.connected ? T.on : T.off; c.className = st.connected ? "ok" : "no";
  r.textContent = st.recording ? T.recOn : T.recOff; r.className = st.recording ? "ok" : "";
  document.getElementById("hint").textContent = !st.connected ? T.hintOff : (st.log ? T.hintOn + " " + T.hintLog : T.hintOn);
  go.disabled = !st.connected || st.app !== "idle";
  if (st.connected && !st.ctl) err.textContent = T.why.disabled;
  else if (st.lastError && T.why[st.lastError]) err.textContent = T.why[st.lastError];
}
chrome.runtime.sendMessage({ q: "state" }, (st) => {
  if (st && st.connected) { show(st); return; }
  chrome.runtime.sendMessage({ q: "reconnect" }, () => {
    setTimeout(() => chrome.runtime.sendMessage({ q: "state" }, show), 400);
  });
});

go.addEventListener("click", async () => {
  go.disabled = true;
  go.textContent = T.busy;
  err.textContent = "";
  const w = await chrome.windows.getCurrent();
  const [tab] = await chrome.tabs.query({ active: true, windowId: w.id });
  const res = await chrome.runtime.sendMessage({ q: "rec", tabId: tab && tab.id, windowId: w.id, pageOnly: page.checked });
  if (res && res.ok) { window.close(); return; }
  err.textContent = T.why[(res && res.why) || "failed"] || T.why.failed;
  go.textContent = T.go;
  go.disabled = false;
});
