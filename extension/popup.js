// The popup: whether Znimok is there and recording; «Record this window» (CAPS-107 — while a
// recording runs there is no popup: a click on the icon stops it).
const uk = /^uk/i.test(chrome.i18n.getUILanguage());
const T = uk ? {
  title: "Znimok", app: "Znimok", rec: "Запис",
  on: "підключено", off: "не знайдено", recOn: "іде — пишу лог", recOff: "ні",
  go: "● Записати це вікно", busy: "Шукаю вікно…",
  page: "Лише сторінка — без вкладок і адресного рядка",
  hintOff: "Запустіть Znimok — розширення знайде застосунок саме.",
  hintOn: "Поки йде запис, клік по значку розширення зупиняє його; пауза — у меню правої кнопки.",
  hintLog: "Поки пишеться лог, браузер показує смугу «розширення налагоджує браузер» — у записі лише сторінки її не видно. Лог пише все, що показує панель Network (заголовки, тіла запитів і відповідей); приховати чутливе можна при експорті.",
  why: {
    "no-app": "Znimok не знайдено — запустіть застосунок.",
    disabled: "Запуск запису з розширення вимкнено в Znimok (Налаштування → Запис).",
    busy: "Запис уже йде або вибирається ділянка.",
    "not-found": "Не вдалося знайти це вікно браузера.",
    ambiguous: "Кілька вікон із такою самою вкладкою — перейдіть на іншу вкладку або почніть запис клавішею Alt+Shift+5.",
    minimized: "Вікно згорнуте.",
    unsupported: "Запис з розширення на цій системі — з наступним оновленням.",
    timeout: "Znimok не відповів.", failed: "Не вдалося почати запис.",
  },
} : {
  title: "Znimok", app: "Znimok", rec: "Recording",
  on: "connected", off: "not found", recOn: "on — writing the log", recOff: "no",
  go: "● Record this window", busy: "Looking for the window…",
  page: "Page only — no tabs or address bar",
  hintOff: "Start Znimok — the extension finds the app by itself.",
  hintOn: "While recording, clicking the extension icon stops it; pause is in the right-click menu.",
  hintLog: "While the log is written, the browser shows the \"extension is debugging this browser\" bar — a page-only recording doesn't show it. The log writes what the Network panel shows (headers, request and response bodies); sensitive parts can be hidden on export.",
  why: {
    "no-app": "Znimok not found — start the app.",
    disabled: "Starting recording from the extension is turned off in Znimok (Settings → Recording).",
    busy: "A recording is already running or a region is being picked.",
    "not-found": "Could not find this browser window.",
    ambiguous: "Several windows show the same tab — switch to another tab or start with Alt+Shift+5.",
    minimized: "The window is minimized.",
    unsupported: "Recording from the extension on this system comes with the next update.",
    timeout: "Znimok didn't answer.", failed: "Could not start recording.",
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
