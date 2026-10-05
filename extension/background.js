// Znimok — the browser log of a recording (ZK-97), after Little Helpers' extension (CAPS-83/107).
//
// The extension talks to Znimok through Native Messaging: the browser starts `znimok` (the CLI)
// as the host «com.znimok.devtools», which relays to the running app. Znimok sends
// {"cmd":"start"} when a recording starts and {"cmd":"stop"} when it ends; in between the
// extension attaches the debugger (chrome.debugger) to the ACTIVE tab, follows tab switches and
// sends every event as it comes.
//
// Times are milliseconds since the epoch by the SYSTEM clock (Date.now() here, the same clock
// Znimok stamps the video with): console, errors and Log carry it themselves, the network through
// the request's wallTime, navigations the time they were seen — no pings for the sync.
//
// What is written (the owner's decision of 30.09.2026): everything the Network panel shows —
// request and response headers, the request's payload, the response's body (up to a limit),
// timings — and the console's arguments in full. Hiding secrets is a choice at export.
//
// dataLayer (ZK-195): GTM's and gtag's events, in full. A hook in the page's own world (set before
// any script of a new document, and into the page already open when the debugger attaches) wraps
// `dataLayer.push`; each pushed value comes back through a CDP binding with the time it was
// pushed. GTM replaces `push` with its own that calls the one before — the hook stays in front
// (an accessor) and a value is sent once however many wrappers it passes.
//
// «Record this window» (CAPS-107): the popup or a shortcut asks {"cmd":"rec"}; Znimok does not
// know the window's handle, so a mark is appended to the page's title for a moment, Znimok finds
// the window with it, says «found» — the mark goes — and only then starts.

const HOST = "com.znimok.devtools";
const MAX_TEXT = 1 << 20;       // a text field of an event, characters
const MAX_BODY = 4 << 20;       // a response body, characters (base64 counts as is)

let port = null;
let recording = false;
let current = null;              // the tab the debugger is attached to now
const attached = new Set();
const reqs = new Map();          // `${tabId}:${requestId}` → a request on its way
let app = { state: "idle", ms: 0, at: 0, log: 1, ctl: 1, app: false };
const pending = new Map();       // rid → { resolve, tabId, suffix, marked, preAttached, timer }
let ridSeq = 0;
let lastError = "";

const browserName = /Edg\//.test(navigator.userAgent) ? "Edge" : "Chrome";

function send(obj) {
  if (!port) return false;
  try { port.postMessage(obj); return true; } catch (e) { return false; }
}

function emit(ev) {
  if (!recording) return;
  ev.b = browserName;
  send(ev);
}

function clip(s, max = MAX_TEXT) {
  s = String(s == null ? "" : s);
  return s.length > max ? s.slice(0, max) + "…" : s;
}

// ---- the connection to Znimok ----

function connect() {
  if (port) return;
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch (e) {
    port = null;
    return;
  }
  port.onMessage.addListener((m) => {
    if (m.cmd === "start") startRecording();
    else if (m.cmd === "stop") stopRecording();
    else if (m.state) onAppState(m);
    else if (m.rec) onRecReply(m);
  });
  port.onDisconnect.addListener(() => {
    void chrome.runtime.lastError;
    port = null;
    if (recording) stopRecording();
    onAppState({ state: "idle", ms: 0, log: app.log, ctl: app.ctl, app: false });
    for (const rid of [...pending.keys()]) finishRec(rid, { ok: false, why: "no-app" });
  });
  send({ hello: 1, browser: browserName, ua: navigator.userAgent, ext: chrome.runtime.getManifest().version });
}

// An open native port keeps the service worker alive; the alarm brings the port back.
setInterval(() => { if (port) send({ ping: 1 }); else connect(); }, 20000);
chrome.alarms.create("znimok-connect", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener(() => connect());
chrome.runtime.onStartup.addListener(() => connect());
chrome.runtime.onInstalled.addListener(() => connect());
connect();

// ---- dataLayer (ZK-195) ----

const DL_BINDING = "__znimokDL";
const MAX_DL = 1 << 20;          // one pushed value as JSON, characters (a safety net only)

// The hook, as the page runs it. `snapshot`: send what the array already holds (the page was
// open before the recording) marked as such.
function dlHook(snapshot) {
  return `(() => {
  const bind = window.${DL_BINDING};
  if (typeof bind !== "function" || window.__znimokDLHooked) return;
  Object.defineProperty(window, "__znimokDLHooked", { value: true });
  const LIMIT = ${MAX_DL};
  const plain = (v) => {
    const seen = new WeakSet();
    const walk = (x, d) => {
      if (typeof x === "function") return "[function " + (x.name || "anonymous") + "]";
      if (typeof x === "bigint") return x.toString();
      if (typeof x === "number" && !isFinite(x)) return String(x);
      if (x === null || typeof x !== "object") return x === undefined ? null : x;
      if (typeof Node !== "undefined" && x instanceof Node)
        return "<" + String(x.nodeName || "").toLowerCase() + (x.id ? "#" + x.id : "") + ">";
      if (typeof Window !== "undefined" && x instanceof Window) return "[window]";
      if (x instanceof Date) return isNaN(x) ? null : x.toISOString();
      if (seen.has(x)) return "[circular]";
      if (d > 40) return "[deep]";
      seen.add(x);
      let out;
      if (Array.isArray(x) || Object.prototype.toString.call(x) === "[object Arguments]") {
        out = Array.prototype.map.call(x, (e) => walk(e, d + 1));
      } else {
        out = {};
        for (const k of Object.keys(x)) if (x[k] !== undefined) out[k] = walk(x[k], d + 1);
      }
      seen.delete(x);
      return out;
    };
    return walk(v, 0);
  };
  const sent = new WeakSet();
  const send = (x, pre) => {
    if (x !== null && (typeof x === "object" || typeof x === "function")) {
      if (sent.has(x)) return;
      sent.add(x);
    }
    try {
      let v = JSON.stringify(plain(x));
      let cut = 0;
      if (v === undefined) v = "null";
      if (v.length > LIMIT) { cut = v.length; v = v.slice(0, LIMIT); }
      bind(JSON.stringify({ t: Date.now(), v, cut, pre, top: window === window.top }));
    } catch (e) { /* a value that cannot be read */ }
  };
  const hook = (arr, existing) => {
    if (!Array.isArray(arr) || arr.__znimokDL) return;
    for (const x of arr) send(x, existing);
    let inner = arr.push;
    Object.defineProperty(arr, "__znimokDL", { value: true });
    Object.defineProperty(arr, "push", {
      configurable: true,
      enumerable: false,
      get() {
        const f = inner;
        return function (...a) { for (const x of a) send(x, false); return f.apply(this, a); };
      },
      set(f) { inner = f; },
    });
  };
  let dl = window.dataLayer;
  hook(dl, ${snapshot ? "true" : "false"});
  try {
    Object.defineProperty(window, "dataLayer", {
      configurable: true,
      enumerable: true,
      get() { return dl; },
      set(v) { dl = v; hook(v, false); },
    });
  } catch (e) { /* the page made it read-only */ }
})()`;
}

// The name a row shows: the pushed object's `event`; gtag's call as «gtag event purchase».
function dlName(v) {
  if (Array.isArray(v)) return ["gtag"].concat(v.slice(0, 2).filter((x) => typeof x === "string")).join(" ");
  if (v && typeof v === "object" && typeof v.event === "string") return v.event;
  return "";
}

// ---- recording ----

async function activeTab() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  return tab || null;
}

const debuggable = (url) => !!url && !/^(chrome|edge|about|chrome-extension|devtools|view-source|chrome-search):/i.test(url)
  && !/^https:\/\/(chrome\.google\.com\/webstore|chromewebstore\.google\.com|microsoftedge\.microsoft\.com\/addons)/i.test(url);

async function follow(tabId) {
  if (!recording || tabId == null || tabId === current) return;
  // The log follows the screen: only the active tab is written — the old one is let go.
  if (current != null) await detach(current);
  current = tabId;
  let tab = null;
  try { tab = await chrome.tabs.get(tabId); } catch (e) { /* closed */ }
  if (tab && !debuggable(tab.url)) {
    emit({ t: Date.now(), k: "tab", s: 0, url: tab.url, title: clip(tab.title), note: "not-debuggable" });
    return;
  }
  try {
    if (!attached.has(tabId)) await chrome.debugger.attach({ tabId }, "1.3");
    attached.add(tabId);
    await chrome.debugger.sendCommand({ tabId }, "Runtime.enable", {});
    await chrome.debugger.sendCommand({ tabId }, "Log.enable", {});
    await chrome.debugger.sendCommand({ tabId }, "Network.enable", { maxPostDataSize: 1 << 20 });
    await chrome.debugger.sendCommand({ tabId }, "Page.enable", {});
    // dataLayer: the binding, the hook for every new document, and the page already open.
    await chrome.debugger.sendCommand({ tabId }, "Runtime.addBinding", { name: DL_BINDING });
    await chrome.debugger.sendCommand({ tabId }, "Page.addScriptToEvaluateOnNewDocument", { source: dlHook(false) });
    try {
      await chrome.debugger.sendCommand({ tabId }, "Runtime.evaluate", { expression: dlHook(true) });
    } catch (e) { /* a page that cannot run scripts */ }
    emit({ t: Date.now(), k: "tab", s: 0, url: tab ? tab.url : "", title: tab ? clip(tab.title) : "" });
  } catch (e) {
    emit({ t: Date.now(), k: "info", s: 1, text: "attach failed: " + clip(e && e.message) });
  }
}

async function detach(tabId) {
  if (!attached.has(tabId)) return;
  attached.delete(tabId);
  for (const key of [...reqs.keys()]) if (key.startsWith(tabId + ":")) reqs.delete(key);
  try { await chrome.debugger.detach({ tabId }); } catch (e) { /* already */ }
}

async function startRecording() {
  if (recording) return;
  recording = true;
  const tab = await activeTab();
  if (tab) await follow(tab.id);
}

async function stopRecording() {
  recording = false;
  current = null;
  for (const id of [...attached]) await detach(id);
  reqs.clear();
}

chrome.tabs.onActivated.addListener(({ tabId }) => { if (recording) follow(tabId); });
chrome.windows.onFocusChanged.addListener(async (winId) => {
  if (!recording || winId === chrome.windows.WINDOW_ID_NONE) return;
  const [tab] = await chrome.tabs.query({ active: true, windowId: winId });
  if (tab) follow(tab.id);
});
chrome.tabs.onUpdated.addListener((tabId, info) => {
  if (recording && tabId === current && info.status === "complete" && !attached.has(tabId)) { current = null; follow(tabId); }
});
chrome.debugger.onDetach.addListener((src, reason) => {
  attached.delete(src.tabId);
  if (recording && src.tabId === current) {
    emit({ t: Date.now(), k: "info", s: 1, text: "debugger detached: " + reason });
    current = null;
  }
});

// ---- CDP events ----

function argText(a) {
  if (!a) return "";
  if (a.value !== undefined) return typeof a.value === "string" ? a.value : JSON.stringify(a.value);
  if (a.unserializableValue) return a.unserializableValue;
  if (a.preview && a.preview.properties) {
    const inner = a.preview.properties.map((p) => (a.subtype === "array" ? "" : p.name + ": ") + p.value).join(", ");
    return a.subtype === "array" ? "[" + inner + "]" : "{" + inner + (a.preview.overflow ? ", …" : "") + "}";
  }
  if (a.description) return a.description;
  return a.type || "";
}

function stackOf(st) {
  if (!st || !st.callFrames) return undefined;
  return st.callFrames.slice(0, 30).map((f) => ({ fn: f.functionName || "", url: f.url, line: f.lineNumber + 1, col: f.columnNumber + 1 }));
}

function frameSrc(st) {
  const f = st && st.callFrames && st.callFrames[0];
  return f ? `${f.url}:${f.lineNumber + 1}` : "";
}

const headersOf = (h) => (h ? Object.fromEntries(Object.entries(h).map(([k, v]) => [k, clip(v, 64 << 10)])) : undefined);

async function finishRequest(tabId, p, failed) {
  const key = tabId + ":" + p.requestId;
  const r = reqs.get(key);
  if (!r) return;
  reqs.delete(key);
  const bad = failed || (r.status || 0) >= 400;
  const ev = {
    t: r.t, k: "net", s: bad ? 2 : 0, id: p.requestId, method: r.method, url: r.url, status: r.status || 0,
    statusText: r.statusText || "", type: r.type, mime: r.mime || "", proto: r.proto || "",
    dur: Math.round((p.timestamp - r.ts) * 1000), size: failed ? 0 : (p.encodedDataLength || 0),
    reqHeaders: r.reqHeaders, resHeaders: r.resHeaders, timing: r.timing, initiator: r.initiator,
    remote: r.remote,
  };
  if (r.postData != null) ev.postData = clip(r.postData);
  if (r.cache) ev.cache = true;
  if (failed) { ev.err = p.errorText || "failed"; if (p.canceled) ev.canceled = true; }
  if (!failed && r.hasPostData && r.postData == null) {
    try {
      const d = await chrome.debugger.sendCommand({ tabId }, "Network.getRequestPostData", { requestId: p.requestId });
      if (d && d.postData != null) ev.postData = clip(d.postData);
    } catch (e) { /* gone */ }
  }
  // The response's body, as the Response tab shows it (binary as base64), up to the limit.
  if (!failed && r.status !== 204 && r.status !== 304 && !/^(Image|Media|Font)$/.test(r.type || "")) {
    try {
      const b = await chrome.debugger.sendCommand({ tabId }, "Network.getResponseBody", { requestId: p.requestId });
      if (b && b.body != null) {
        ev.body = b.body.length > MAX_BODY ? b.body.slice(0, MAX_BODY) : b.body;
        if (b.base64Encoded) ev.b64 = true;
        if (b.body.length > MAX_BODY) ev.bodyCut = b.body.length;
      }
    } catch (e) { /* no body (redirect, cache, gone) */ }
  }
  emit(ev);
}

chrome.debugger.onEvent.addListener((src, method, p) => {
  if (!recording || src.tabId !== current) return;
  const tabId = src.tabId;
  switch (method) {
  case "Runtime.consoleAPICalled": {
    const lvl = p.type === "warning" ? "warn" : p.type;
    const s = lvl === "error" || lvl === "assert" ? 2 : lvl === "warn" ? 1 : 0;
    emit({ t: p.timestamp, k: "console", s, lvl, text: clip((p.args || []).map(argText).join(" ")),
           args: (p.args || []).map((a) => clip(argText(a), 64 << 10)), src: frameSrc(p.stackTrace), stack: stackOf(p.stackTrace) });
    break;
  }
  case "Runtime.exceptionThrown": {
    const d = p.exceptionDetails || {};
    const text = (d.exception && d.exception.description) || d.text || "exception";
    emit({ t: p.timestamp, k: "error", s: 2, text: clip(text),
           src: d.url ? `${d.url}:${(d.lineNumber || 0) + 1}` : frameSrc(d.stackTrace), stack: stackOf(d.stackTrace) });
    break;
  }
  case "Log.entryAdded": {
    const e = p.entry || {};
    if (e.level === "verbose") break;
    const s = e.level === "error" ? 2 : e.level === "warning" ? 1 : 0;
    emit({ t: e.timestamp, k: "log", s, lvl: e.level, text: clip(e.text), src: e.source || "", url: e.url || "" });
    break;
  }
  case "Network.requestWillBeSent": {
    const key = tabId + ":" + p.requestId;
    const wall = p.wallTime * 1000;
    if (p.redirectResponse && reqs.has(key)) {   // a redirect: the step before as its own event
      const r = reqs.get(key);
      emit({ t: r.t, k: "net", s: 0, id: p.requestId, method: r.method, url: r.url, status: p.redirectResponse.status,
             type: r.type, dur: Math.round((p.timestamp - r.ts) * 1000), size: 0, redirect: p.request.url,
             reqHeaders: r.reqHeaders, resHeaders: headersOf(p.redirectResponse.headers) });
    }
    reqs.set(key, {
      t: wall, ts: p.timestamp, url: p.request.url, method: p.request.method, type: p.type || "",
      reqHeaders: headersOf(p.request.headers), postData: p.request.postData, hasPostData: !!p.request.hasPostData,
      initiator: p.initiator ? { type: p.initiator.type, url: p.initiator.url, line: p.initiator.lineNumber } : undefined,
    });
    break;
  }
  case "Network.requestWillBeSentExtraInfo": {
    // The headers as sent, cookies included.
    const r = reqs.get(tabId + ":" + p.requestId);
    if (r && p.headers) r.reqHeaders = headersOf(p.headers);
    break;
  }
  case "Network.responseReceived": {
    const r = reqs.get(tabId + ":" + p.requestId);
    if (r) {
      const q = p.response;
      r.status = q.status; r.statusText = q.statusText; r.mime = q.mimeType; r.proto = q.protocol;
      r.resHeaders = headersOf(q.headers); r.timing = q.timing; r.remote = q.remoteIPAddress;
      r.cache = !!(q.fromDiskCache || q.fromServiceWorker || q.fromPrefetchCache);
    }
    break;
  }
  case "Network.responseReceivedExtraInfo": {
    const r = reqs.get(tabId + ":" + p.requestId);
    if (r && p.headers) r.resHeaders = headersOf(p.headers);
    break;
  }
  case "Network.loadingFinished":
    finishRequest(tabId, p, false);
    break;
  case "Network.loadingFailed":
    finishRequest(tabId, p, true);
    break;
  case "Network.webSocketFrameSent":
  case "Network.webSocketFrameReceived": {
    const f = p.response || {};
    emit({ t: Date.now(), k: "ws", s: 0, id: p.requestId, dir: method.endsWith("Sent") ? "out" : "in",
           op: f.opcode, data: clip(f.payloadData) });
    break;
  }
  case "Runtime.bindingCalled": {
    if (p.name !== DL_BINDING) break;
    let m;
    try { m = JSON.parse(p.payload); } catch (e) { break; }
    let data;
    try { data = JSON.parse(m.v); } catch (e) { data = m.v; }
    const ev = { t: m.t || Date.now(), k: "dl", s: 0, ev: dlName(data), data };
    if (m.cut) ev.cut = m.cut;
    if (m.pre) ev.pre = true;
    if (m.top === false) ev.frame = true;
    emit(ev);
    break;
  }
  case "Page.frameNavigated": {
    if (p.frame && !p.frame.parentId) emit({ t: Date.now(), k: "nav", s: 0, url: p.frame.url });
    break;
  }
  }
});

// ---- «Record this window» (CAPS-107) ----

// Texts from _locales (ZK-182).
const L = {
  tipRec: chrome.i18n.getMessage("tipRec"),
  tipPaused: chrome.i18n.getMessage("tipPaused"),
  menuStart: chrome.i18n.getMessage("menuStart"),
  menuPause: chrome.i18n.getMessage("menuPause"),
  menuResume: chrome.i18n.getMessage("menuResume"),
  menuStop: chrome.i18n.getMessage("menuStop"),
};

function elapsed() {
  return app.state === "rec" ? app.ms + (Date.now() - app.at) : app.ms;
}

function fmt(ms) {
  const s = Math.floor(ms / 1000), m = Math.floor(s / 60);
  if (m >= 100) return Math.floor(m / 60) + "h";
  return m + ":" + String(s % 60).padStart(2, "0");
}

let badgeTimer = null;

function paintAction() {
  const on = app.state === "rec" || app.state === "paused";
  // While recording there is no popup: a click on the icon comes to onClicked and stops it.
  chrome.action.setPopup({ popup: on ? "" : "popup.html" });
  if (on) {
    const t = fmt(elapsed());
    chrome.action.setBadgeBackgroundColor({ color: app.state === "paused" ? "#FFB300" : "#E53935" });
    if (chrome.action.setBadgeTextColor) chrome.action.setBadgeTextColor({ color: app.state === "paused" ? "#000000" : "#FFFFFF" });
    chrome.action.setBadgeText({ text: t });
    chrome.action.setTitle({ title: (app.state === "paused" ? L.tipPaused : L.tipRec).replace("{t}", t) });
  } else {
    chrome.action.setBadgeText({ text: lastError ? "!" : "" });
    if (lastError) chrome.action.setBadgeBackgroundColor({ color: "#B3261E" });
    chrome.action.setTitle({ title: "Znimok" });
  }
  // The right-click menu of the icon (ZK-230, as Little Helpers' extension had it): «Record this
  // window» while idle, pause / stop while recording.
  chrome.contextMenus.update("zn-start", { visible: !on, enabled: app.app !== false }, () => void chrome.runtime.lastError);
  chrome.contextMenus.update("zn-pause", { visible: on, title: app.state === "paused" ? L.menuResume : L.menuPause }, () => void chrome.runtime.lastError);
  chrome.contextMenus.update("zn-stop", { visible: on }, () => void chrome.runtime.lastError);
}

function onAppState(m) {
  const was = app.state;
  app = { state: m.state, ms: Number(m.ms) || 0, at: Date.now(), log: m.log == null ? app.log : m.log,
          ctl: m.ctl == null ? app.ctl : m.ctl, app: m.app !== false };
  if (app.state !== "idle" && was === "idle") lastError = "";
  // «Znimok not found» is over once Znimok is there (ZK-296).
  if (app.app && lastError === "no-app") lastError = "";
  paintAction();
  if (app.state === "rec" && !badgeTimer) badgeTimer = setInterval(paintAction, 1000);
  if (app.state !== "rec" && badgeTimer) { clearInterval(badgeTimer); badgeTimer = null; }
}

// The mark is appended and removed the same way: the page may have changed its title meanwhile.
async function setMarker(tabId, suffix, on) {
  const [r] = await chrome.scripting.executeScript({
    target: { tabId },
    func: (sfx, add) => {
      if (add) { document.title = document.title + sfx; return { iw: innerWidth, ih: innerHeight, dpr: devicePixelRatio }; }
      const t = document.title, i = t.lastIndexOf(sfx);
      if (i >= 0) document.title = t.slice(0, i) + t.slice(i + sfx.length);
      return null;
    },
    args: [suffix, on],
  });
  return r ? r.result : null;
}

function finishRec(rid, res) {
  const p = pending.get(rid);
  if (!p) return;
  pending.delete(rid);
  clearTimeout(p.timer);
  if (p.marked) setMarker(p.tabId, p.suffix, false).catch(() => {});
  if (p.preAttached && (!res.ok || !res.log) && !(recording && current === p.tabId)) detach(p.tabId);
  lastError = res.ok ? "" : (res.why || "failed");
  paintAction();
  p.resolve(res);
}

function onRecReply(m) {
  const p = pending.get(m.rid);
  if (!p) return;                                   // another browser's request
  if (m.rec === "found") {
    if (p.marked) { p.marked = false; setMarker(p.tabId, p.suffix, false).catch(() => {}); }
    return;
  }
  finishRec(m.rid, m.rec === "ok" ? { ok: true, log: m.log !== 0 } : { ok: false, why: m.why || "failed" });
}

async function startRec(tabId, windowId, pageOnly) {
  if (!port || !app.app) return { ok: false, why: "no-app" };
  if (!app.ctl) return { ok: false, why: "disabled" };
  if (app.state !== "idle") return { ok: false, why: "busy" };
  let tab, win;
  try { tab = await chrome.tabs.get(tabId); win = await chrome.windows.get(windowId); } catch (e) { return { ok: false, why: "not-found" }; }
  const rid = (Date.now() % 1000000) * 100 + (++ridSeq % 100);
  const marker = "ZN-" + [...crypto.getRandomValues(new Uint8Array(4))].map((b) => b.toString(16).padStart(2, "0")).join("");
  const p = { tabId, suffix: " ⏺ " + marker, marked: false, preAttached: false, timer: 0, resolve: null };
  const done = new Promise((res) => { p.resolve = res; });
  pending.set(rid, p);
  // With the log on, the debugger goes first: its «is debugging» bar shows before the first frame.
  if (app.log && debuggable(tab.url) && !attached.has(tabId)) {
    try { await chrome.debugger.attach({ tabId }, "1.3"); attached.add(tabId); p.preAttached = true; } catch (e) { /* the log tries again */ }
    await new Promise((r) => setTimeout(r, 150));
  }
  let metrics = null;
  try { metrics = await setMarker(tabId, p.suffix, true); p.marked = true; } catch (e) { /* chrome:// — no mark */ }
  const msg = { cmd: "rec", rid, page: pageOnly ? 1 : 0, title: tab.title || "", wl: win.left, wt: win.top, ww: win.width, wh: win.height };
  if (p.marked) msg.marker = marker;
  if (metrics) { msg.pw = Math.round(metrics.iw * metrics.dpr); msg.ph = Math.round(metrics.ih * metrics.dpr); }
  p.timer = setTimeout(() => finishRec(rid, { ok: false, why: "timeout" }), 8000);
  send(msg);
  return done;
}

async function startFromWindow(windowId) {
  const [tab] = await chrome.tabs.query({ active: true, windowId });
  if (!tab) return { ok: false, why: "not-found" };
  const { pageOnly = true } = await chrome.storage.local.get("pageOnly");
  return startRec(tab.id, windowId, pageOnly);
}

chrome.action.onClicked.addListener(() => send({ cmd: "stop" }));   // only while recording

function setupMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "zn-start", title: L.menuStart, contexts: ["action"] });
    chrome.contextMenus.create({ id: "zn-pause", title: L.menuPause, contexts: ["action"], visible: false });
    chrome.contextMenus.create({ id: "zn-stop", title: L.menuStop, contexts: ["action"], visible: false });
    paintAction();
  });
}
chrome.runtime.onInstalled.addListener(setupMenus);
chrome.runtime.onStartup.addListener(setupMenus);
chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "zn-stop") send({ cmd: "stop" });
  else if (info.menuItemId === "zn-pause") send({ cmd: app.state === "paused" ? "resume" : "pause" });
  else if (info.menuItemId === "zn-start") {
    if (app.state !== "idle") return;
    const w = tab ? tab.windowId : (await chrome.windows.getLastFocused()).id;
    await startFromWindow(w);
  }
});

chrome.commands.onCommand.addListener(async (command, tab) => {
  if (command !== "toggle-recording") return;
  if (app.state !== "idle") { send({ cmd: "stop" }); return; }
  const w = tab ? tab.windowId : (await chrome.windows.getLastFocused()).id;
  await startFromWindow(w);
});

// ---- the popup's questions ----
chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg && msg.q === "state") {
    reply({ connected: !!port && app.app, recording, browser: browserName, app: app.state, ctl: app.ctl, log: app.log, lastError });
    return true;
  }
  // The popup showed the last failure: once is enough, the «!» on the icon goes (ZK-296).
  if (msg && msg.q === "seen") { lastError = ""; paintAction(); reply({ ok: true }); return true; }
  if (msg && msg.q === "reconnect") { if (port) { try { port.disconnect(); } catch (e) {} port = null; } connect(); reply({ ok: true }); return true; }
  if (msg && msg.q === "rec") {
    chrome.storage.local.set({ pageOnly: !!msg.pageOnly });
    startRec(msg.tabId, msg.windowId, !!msg.pageOnly).then(reply, () => reply({ ok: false, why: "failed" }));
    return true;
  }
  return false;
});
