"use strict";

const invoke = window.__TAURI__.core.invoke;

const state = {
  view: "apps",
  query: "",
  showSystem: false,
  live: true,
  sort: { key: "cpu", asc: false },
  snap: null,
  byPid: new Map(),
  selected: null,       // pid with focus (shown in the detail pane)
  selectedStart: null,  // its start_time, to detect pid reuse
  pick: MultiSelect.empty(),   // processes picked with ⌘/⇧-click (keys: pid)
  pickStart: new Map(),        // pid -> start_time when picked
  expanded: new Set(),  // app pids / tree pids that are open
  treeCollapsed: new Set(),
  icons: new Map(),     // path -> data url | null | Promise
  rows: [],             // pids in display order, for keyboard nav
  busy: false,
  history: new Map(),   // key -> { cpu: [], mem: [] }
  totalCpu: [],
  startup: null,        // list of startup items
  startupAt: 0,
  selectedStartup: null, // plist path of the item with focus
  startupPick: MultiSelect.empty(), // startup items picked (keys: plist path,
                                    // since one label can be in two folders)
  helperReady: null,     // password-free switching set up?
};
const HIST = 90; // samples kept (3 minutes at one every 2 s)

const $ = (s) => document.querySelector(s);
const el = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};

const plural = (n, one, many = one + "s") => `${n} ${n === 1 ? one : many}`;

// ---------- selection ----------
// Every list supports picking several rows: ⌘-click adds or removes one,
// ⇧-click (or ⇧-arrow) picks a range, ⌘A picks all, Esc goes back to one.
function setPick(sel) {
  state.pick = sel;
  for (const k of [...state.pickStart.keys()]) if (!sel.keys.has(k)) state.pickStart.delete(k);
  for (const k of sel.keys) {
    const p = state.byPid.get(k);
    if (!state.pickStart.has(k) && p) state.pickStart.set(k, p.start_time);
  }
  state.selected = sel.focus;
  state.selectedStart = sel.focus != null ? (state.pickStart.get(sel.focus) ?? null) : null;
}
function setStartupPick(sel) {
  state.startupPick = sel;
  state.selectedStartup = sel.focus;
}
/** Processes picked that are still the same running process. */
function pickedProcs() {
  return [...state.pick.keys]
    .map((pid) => state.byPid.get(pid))
    .filter((p) => p && p.start_time === state.pickStart.get(p.pid));
}
function pickedStartup() {
  return (state.startup || []).filter((i) => state.startupPick.keys.has(i.plist));
}
function pickCount() {
  return state.view === "startup" ? state.startupPick.keys.size : state.pick.keys.size;
}
/** Apply a click or arrow key to the current view's selection, then show it. */
function applyPick(next) {
  if (state.view === "startup") setStartupPick(next(state.startupPick));
  else setPick(next(state.pick));
  render();
  const focus = state.view === "startup" ? state.selectedStartup : state.selected;
  if (focus == null) return;
  const sel = state.view === "startup" ? `.row[data-plist="${CSS.escape(String(focus))}"]` : `.row[data-pid="${focus}"]`;
  document.querySelector(sel)?.scrollIntoView({ block: "nearest" });
}

// ---------- formatting ----------
function fmtMem(b) {
  if (b >= 1 << 30) return (b / (1 << 30)).toFixed(1) + " GB";
  if (b >= 1 << 20) return Math.round(b / (1 << 20)) + " MB";
  if (b >= 1 << 10) return Math.round(b / (1 << 10)) + " KB";
  return b + " B";
}
function fmtCpu(c) { return c >= 10 ? c.toFixed(0) + "%" : c.toFixed(1) + "%"; }
function fmtDuration(s) {
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  if (d) return `${d}d ${h}h`;
  if (h) return `${h}h ${m}m`;
  if (m) return `${m}m`;
  return `${s}s`;
}
const KIND_LABEL = {
  app: "App",
  menubar: "Menu bar app",
  helper: "App helper",
  background: "Background program",
  system: "macOS system",
};

// ---------- data ----------
async function refresh() {
  if (state.busy) return;
  state.busy = true;
  try {
    const snap = await invoke("snapshot");
    state.snap = snap;
    state.byPid = new Map(snap.procs.map((p) => [p.pid, p]));
    // With several picked, drop the ones that have ended. (A single pick
    // stays, so the detail pane can say "That process has ended".)
    if (state.pick.keys.size > 1) {
      setPick(MultiSelect.prune(state.pick, (pid) => state.byPid.get(pid)?.start_time === state.pickStart.get(pid)));
    }
    recordHistory(snap);
    if (state.view === "startup" && Date.now() - state.startupAt > 10000) await loadStartup();
    render();
  } catch (e) {
    toast(String(e), true);
  } finally {
    state.busy = false;
  }
}

// ---------- history (for the graphs) ----------
const histKey = (p) => (p.kind === "app" || p.kind === "menubar") ? `g:${p.pid}:${p.start_time}` : `p:${p.pid}:${p.start_time}`;
function recordHistory(snap) {
  const seen = new Set();
  const push = (key, cpu, mem) => {
    let h = state.history.get(key);
    if (!h) { h = { cpu: [], mem: [] }; state.history.set(key, h); }
    h.cpu.push(cpu); h.mem.push(mem);
    if (h.cpu.length > HIST) { h.cpu.shift(); h.mem.shift(); }
    seen.add(key);
  };
  const groups = new Map();
  for (const p of snap.procs) {
    if (p.kind !== "app" && p.kind !== "menubar") push(histKey(p), p.cpu, p.memory);
    if (p.app_pid != null) {
      const g = groups.get(p.app_pid) || { cpu: 0, mem: 0 };
      g.cpu += p.cpu; g.mem += p.memory;
      groups.set(p.app_pid, g);
    }
  }
  for (const [pid, g] of groups) {
    const p = state.byPid.get(pid);
    if (p) push(histKey(p), g.cpu, g.mem);
  }
  for (const k of state.history.keys()) if (!seen.has(k)) state.history.delete(k);
  state.totalCpu.push(snap.procs.reduce((t, p) => t + p.cpu, 0) / (snap.cpu_count || 1));
  if (state.totalCpu.length > HIST) state.totalCpu.shift();
}

const SVGNS = "http://www.w3.org/2000/svg";
function sparkline(values, max, cls) {
  const svg = document.createElementNS(SVGNS, "svg");
  svg.setAttribute("viewBox", "0 0 100 40");
  svg.setAttribute("preserveAspectRatio", "none");
  if (cls) svg.setAttribute("class", cls);
  if (values.length < 2) return svg;
  const n = HIST - 1;
  const off = n - (values.length - 1);
  const pts = values.map((v, i) => [((off + i) / n) * 100, 40 - Math.min(1, v / max) * 38 - 1]);
  const line = pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(2)},${y.toFixed(2)}`).join("");
  const area = document.createElementNS(SVGNS, "path");
  area.setAttribute("d", `${line}L${pts[pts.length - 1][0].toFixed(2)},40L${pts[0][0].toFixed(2)},40Z`);
  area.setAttribute("class", "area");
  const path = document.createElementNS(SVGNS, "path");
  path.setAttribute("d", line);
  path.setAttribute("class", "line");
  path.setAttribute("vector-effect", "non-scaling-stroke");
  svg.append(area, path);
  return svg;
}

function graphs(p) {
  const h = state.history.get(histKey(p));
  const wrap = el("div", "graphs");
  const span = h ? Math.round((h.cpu.length * 2) / 60 * 10) / 10 : 0;
  const incl = p.kind === "app" || p.kind === "menubar" ? " · app + helpers" : "";
  const mk = (cls, label, vals, max, fmt) => {
    const g = el("div", "graph " + cls);
    const lab = el("div", "g-label");
    lab.append(el("span", null, label + incl));
    lab.append(el("b", null, vals.length ? fmt(vals[vals.length - 1]) : "—"));
    g.append(lab, sparkline(vals, max));
    return g;
  };
  const cpu = h ? h.cpu : [];
  const mem = h ? h.mem : [];
  wrap.append(
    mk("cpu", "CPU", cpu, Math.max(10, ...cpu) * 1.1, fmtCpu),
    mk("mem", "Memory", mem, Math.max(1, ...mem) * 1.15, fmtMem),
  );
  wrap.title = cpu.length < 2 ? "Graphs fill in while Live is on." : `Last ${span < 1 ? Math.round(span * 60) + " seconds" : span + " minutes"}`;
  return wrap;
}

// ---------- startup items ----------
async function loadStartup() {
  try {
    const [items, ready] = await Promise.all([invoke("startup_items"), invoke("startup_helper_ready")]);
    state.startup = items;
    if (state.startupPick.keys.size > 1) {
      const plists = new Set(items.map((i) => i.plist));
      setStartupPick(MultiSelect.prune(state.startupPick, (k) => plists.has(k)));
    }
    state.helperReady = ready;
    state.startupAt = Date.now();
  } catch (e) {
    toast(String(e), true);
  }
}
const SCOPE_TITLE = {
  user: "Starts when you log in (just for you)",
  "all-users": "Starts when anyone logs in",
  "system-service": "System services (start with the Mac, run as administrator)",
};
const SCOPE_SHORT = { user: "Launch agent (you)", "all-users": "Launch agent (all users)", "system-service": "System service" };

function renderStartup(frag, q) {
  if (!state.startup) { frag.append(el("div", "empty", "Loading…")); return; }
  frag.append(el("div", "intro",
    "Programs that apps and installers set up to start automatically. Apple's own are not shown. " +
    "Turning one off stops it starting next time; you can turn it back on here."));
  const hasSystem = state.startup.some((i) => i.scope === "system-service");
  if (hasSystem) {
    const bar = el("div", "intro helper-bar");
    if (state.helperReady) {
      bar.append(el("span", null, "✓ System services switch on and off without asking for your password. "));
      const rm = el("a", "link", "Turn this off");
      rm.onclick = removeHelper;
      bar.append(rm);
    } else {
      bar.append(el("span", null, "System services ask for your password every time. "));
      const b = el("button", "btn", "Stop asking for my password…");
      b.onclick = installHelper;
      bar.append(b);
    }
    frag.append(bar);
  }
  const items = state.startup.filter((i) => !q ||
    i.label.toLowerCase().includes(q) ||
    (i.vendor || "").toLowerCase().includes(q) ||
    (i.program || "").toLowerCase().includes(q));
  for (const scope of ["user", "all-users", "system-service"]) {
    const list = items.filter((i) => i.scope === scope);
    if (!list.length) continue;
    frag.append(el("div", "section-head", `${SCOPE_TITLE[scope]} (${list.length})`));
    for (const i of list) {
      const r = el("div", "row startup");
      r.dataset.plist = i.plist;
      if (state.startupPick.keys.has(i.plist)) r.classList.add("selected");
      const name = el("div", "name");
      const bundle = i.program && i.program.includes(".app/") ? i.program.slice(0, i.program.indexOf(".app/") + 4) : null;
      name.append(iconNode(bundle));
      const label = el("span", "label", i.label);
      label.title = i.label;
      name.append(label);
      if (i.vendor) name.append(el("span", "badge", i.vendor));
      const when = el("span", "user muted", i.when);
      when.title = i.when;
      const chip = i.running_pid != null
        ? el("span", "chip run", i.enabled ? "On · running" : "Off · running")
        : el("span", "chip " + (i.enabled ? "on" : "off"), i.enabled ? "On" : "Off");
      r.append(name, when, chip);
      state.rows.push(i.plist);
      frag.append(r);
    }
  }
  if (!items.length) frag.append(el("div", "empty", "Nothing matches your search."));
}

function renderStartupDetail(pane) {
  if (state.startupPick.keys.size > 1) { renderStartupMultiDetail(pane); return; }
  const i = state.startup && state.startup.find((x) => x.plist === state.selectedStartup);
  if (!i) {
    pane.replaceChildren(el("div", "empty", "Select a startup item to see what it runs and to turn it on or off. ⌘-click or ⇧-click to pick several."));
    return;
  }
  const out = document.createDocumentFragment();
  const head = el("div", "d-head");
  const bundle = i.program && i.program.includes(".app/") ? i.program.slice(0, i.program.indexOf(".app/") + 4) : null;
  head.append(iconNode(bundle));
  const t = el("div");
  t.append(el("div", "d-title", i.label));
  t.append(el("div", "d-sub", SCOPE_SHORT[i.scope] + (i.enabled ? " · on" : " · off")));
  head.append(t);
  out.append(head);

  let desc = i.vendor ? `Installed by ${i.vendor}. ` : "";
  desc += i.when + ".";
  if (i.scope === "system-service") desc += " It runs as the administrator (root), even when nobody is logged in.";
  if (!i.program_exists && i.program) desc += " Its program file is missing, so it probably belongs to software that was removed. It's safe to turn off.";
  out.append(el("div", "d-desc", desc));

  out.append(el("div", "d-section", "Right now"));
  const now = el("div");
  if (i.running_pid != null) {
    now.append("Running as ");
    now.append(pidLink(i.running_pid, state.byPid.get(i.running_pid)?.name || i.label));
    now.append(".");
  } else {
    now.textContent = "Not running at the moment (or macOS hides it).";
  }
  out.append(now);

  out.append(el("div", "d-section", "Details"));
  const dl = el("dl", "d-grid");
  const add = (k, v, mono) => {
    dl.append(el("dt", null, k));
    const dd = el("dd", mono ? "mono" : null);
    if (v instanceof Node) dd.append(v); else dd.textContent = v;
    dl.append(dd);
  };
  const finder = (path) => {
    const a = el("a", "link", "Show in Finder");
    a.onclick = () => invoke("reveal", { path }).catch((e) => toast(String(e), true));
    return a;
  };
  if (i.program) {
    const s = el("span");
    s.append(i.program + " ");
    if (i.program_exists) s.append(finder(bundle || i.program));
    add("Program", s, true);
  }
  if (i.arguments.length > 1) add("Arguments", i.arguments.slice(1).join(" "), true);
  const pl = el("span");
  pl.append(i.plist + " ");
  pl.append(finder(i.plist));
  add("Settings file", pl, true);
  out.append(dl);

  const search = el("a", "link", "Search the web for this ↗");
  search.onclick = () => invoke("search_web", { name: i.label }).catch((e) => toast(String(e), true));
  const sp = el("div", "note");
  sp.append(search);
  out.append(sp);

  out.append(el("div", "d-section", i.enabled ? "Turn it off" : "Turn it on"));
  const actions = el("div", "actions");
  const btn = el("button", i.enabled ? "btn danger" : "btn", i.enabled ? "Turn off" : "Turn on");
  btn.onclick = () => toggleStartup(i);
  actions.append(btn);
  out.append(actions);
  out.append(el("div", "note", i.scope === "system-service"
    ? (state.helperReady ? "No password needed (set up at the top of this tab). " : "macOS will ask for your password. ") + "Be careful with security, VPN and company-managed software (antivirus, FortiClient, Duo and similar): turning those off can break things or go against your workplace's rules."
    : "Turning it off stops it now and stops it starting at login. The app it belongs to may stop working fully until you turn it back on."));
  pane.replaceChildren(out);
}

async function installHelper() {
  const ok = await confirmDialog(
    "Stop asking for your password?",
    "macOS will ask for your password one last time. After that, Process Viewer can turn system services on and off straight away. " +
    "It installs a small startup helper that can only switch third-party system services in /Library/LaunchDaemons on or off (never Apple's), " +
    "and a rule letting your account run just the startup helper. Any program running as you could then use it too, including to turn off antivirus, VPN or company security services, so leave this off on a work Mac. You can undo this here at any time.",
    "Set it up",
  );
  if (!ok) return;
  try {
    await invoke("install_startup_helper");
    toast("Done. System services now switch without a password.");
  } catch (e) {
    toast(String(e), true);
  }
  await loadStartup();
  render();
}

async function removeHelper() {
  const ok = await confirmDialog(
    "Ask for the password again?",
    "This removes the startup helper and the rule. macOS will ask for your password to do it, and again each time you switch a system service.",
    "Remove",
  );
  if (!ok) return;
  try {
    await invoke("remove_startup_helper");
    toast("Removed. System services will ask for your password again.");
  } catch (e) {
    toast(String(e), true);
  }
  await loadStartup();
  render();
}

async function toggleStartup(i) {
  return toggleMany([i], !i.enabled);
}

/** Turn startup items on or off: one confirmation, one password prompt. */
async function toggleMany(items, enable) {
  if (!items.length) return;
  const one = items.length === 1 ? items[0] : null;
  // System services need root. Until the startup helper is set up, offer it
  // right here so this is the last time macOS asks for the password.
  const offerHelper = items.some((i) => i.scope === "system-service") && !state.helperReady;
  const what = one ? `“${one.label}”` : plural(items.length, "startup item");
  const body = one
    ? (enable
      ? "It will start now and every time it's meant to."
      : "It will stop now and won't start automatically any more. You can turn it back on here.")
    : `${namesText(items.map((i) => i.label))}. ` + (enable
      ? "They will start now and every time they're meant to."
      : "They will stop now and won't start automatically any more. You can turn them back on here.");
  const choice = await confirmDialog(
    enable ? `Turn on ${what}?` : `Turn off ${what}?`,
    body,
    enable ? "Turn on" : "Turn off",
    offerHelper
      ? "Don't ask for my password again. macOS asks once more now to install the startup helper, which can only switch third-party system services (never Apple's). Other programs running as you could use it too, including to turn off security software, so leave this off on a work Mac. You can undo this at the top of this tab."
      : null,
  );
  if (!choice) return;
  if (choice === "ok+option") {
    try {
      await invoke("install_startup_helper");
      state.helperReady = true;
    } catch (e) {
      toast(String(e), true);
      await loadStartup();
      render();
      return;
    }
  }
  try {
    if (one) {
      toast(await invoke("set_startup", { label: one.label, enable }));
    } else {
      const labels = [...new Set(items.map((i) => i.label))];
      const out = await invoke("set_startup_many", { labels, enable });
      reportMany(out, (n) => `Turned ${enable ? "on" : "off"} ${plural(n, "startup item")}.`);
    }
  } catch (e) {
    toast(String(e), true);
  }
  await loadStartup();
  await refresh();
}

function renderStartupMultiDetail(pane) {
  const items = pickedStartup();
  const on = items.filter((i) => i.enabled);
  const off = items.filter((i) => !i.enabled);
  const out = document.createDocumentFragment();
  const t = el("div", "d-head");
  const h = el("div");
  h.append(el("div", "d-title", `${plural(items.length, "startup item")} selected`));
  h.append(el("div", "d-sub", `${on.length} on · ${off.length} off`));
  t.append(h);
  out.append(t, namesList(items.map((i) => i.label)));
  out.append(el("div", "d-section", "Turn them on or off"));
  const actions = el("div", "actions");
  if (on.length) {
    const b = el("button", "btn danger", `Turn off ${on.length}`);
    b.onclick = () => toggleMany(on, false);
    actions.append(b);
  }
  if (off.length) {
    const b = el("button", "btn", `Turn on ${off.length}`);
    b.onclick = () => toggleMany(off, true);
    actions.append(b);
  }
  out.append(actions);
  if (items.some((i) => i.scope === "system-service")) {
    out.append(el("div", "note", (state.helperReady
      ? "No password needed (set up at the top of this tab). "
      : "macOS will ask for your password once for all the system services. ") +
      "Be careful with security, VPN and company-managed software: turning those off can break things or go against your workplace's rules."));
  }
  out.append(el("div", "note", "⌘-click to add or remove an item, ⇧-click for a range, Esc to go back to one."));
  pane.replaceChildren(out);
}

/**
 * Resolves to false (cancelled), "ok", or "ok+option" when the optional
 * checkbox was shown and ticked.
 * @param {string} title @param {string} body @param {string} okLabel
 * @param {string | null} [option] checkbox text; ticked by default
 */
function confirmDialog(title, body, okLabel, option = null) {
  $("#confirm-title").textContent = title;
  $("#confirm-body").textContent = body;
  $("#confirm-ok").textContent = okLabel;
  const opt = $("#confirm-option");
  const box = $("#confirm-option-box");
  opt.hidden = !option;
  $("#confirm-option-text").textContent = option || "";
  box.checked = Boolean(option);
  const dlg = $("#confirm");
  dlg.returnValue = "";
  dlg.showModal();
  return new Promise((res) => dlg.addEventListener("close", () => {
    if (dlg.returnValue !== "ok") return res(false);
    res(option && box.checked ? "ok+option" : "ok");
  }, { once: true }));
}

function switchView(view) {
  document.querySelectorAll(".tab").forEach((x) => x.classList.toggle("active", x.dataset.view === view));
  state.view = view;
  if (view === "startup" && Date.now() - state.startupAt > 3000) loadStartup().then(render);
  render();
}

function iconFor(path, imgEl) {
  if (!path) return;
  const hit = state.icons.get(path);
  if (typeof hit === "string") { imgEl.src = hit; imgEl.classList.remove("ph"); imgEl.textContent = ""; return; }
  if (hit === null) return;
  if (!hit) {
    state.icons.set(path, invoke("icon", { path }).then((url) => {
      state.icons.set(path, url || null);
      return url;
    }).catch(() => { state.icons.set(path, null); return null; }));
  }
  state.icons.get(path).then?.((url) => {
    if (url && imgEl.isConnected) {
      const img = el("img", imgEl.className.replace(" ph", ""));
      img.src = url;
      img.alt = "";
      imgEl.replaceWith(img);
    }
  });
}

function iconNode(path, big) {
  const n = el("span", "icon ph", "⚙︎");
  n.setAttribute("aria-hidden", "true");
  if (path) {
    const hit = state.icons.get(path);
    if (typeof hit === "string") {
      const img = el("img", "icon");
      img.src = hit; img.alt = "";
      return img;
    }
    iconFor(path, n);
  }
  return n;
}

function appBundlePath(p) {
  if (!p) return null;
  const app = state.snap.apps.find((a) => a.pid === (p.app_pid ?? p.pid));
  if (app && app.bundle_path) return app.bundle_path;
  if (p.exe) {
    const i = p.exe.indexOf(".app/");
    if (i > 0) return p.exe.slice(0, i + 4);
  }
  return null;
}

function matches(p, q) {
  if (!q) return true;
  return (
    p.name.toLowerCase().includes(q) ||
    String(p.pid) === q ||
    (p.app_name && p.app_name.toLowerCase().includes(q)) ||
    (p.exe && p.exe.toLowerCase().includes(q)) ||
    (p.user && p.user.toLowerCase().includes(q))
  );
}

function sorter() {
  const { key, asc } = state.sort;
  const dir = asc ? 1 : -1;
  return (a, b) => {
    let x = a[key], y = b[key];
    if (typeof x === "string" || typeof y === "string") {
      x = (x || "").toLowerCase(); y = (y || "").toLowerCase();
      return x < y ? -dir : x > y ? dir : 0;
    }
    return ((x ?? 0) - (y ?? 0)) * dir;
  };
}

// ---------- rendering ----------
function render() {
  if (!state.snap) return;
  const list = $("#list");
  const scroll = list.scrollTop;
  const frag = document.createDocumentFragment();
  state.rows = [];
  const q = state.query.trim().toLowerCase();

  document.querySelectorAll(".sortable").forEach((s) => {
    s.classList.toggle("sorted", s.dataset.sort === state.sort.key);
    s.classList.toggle("asc", s.dataset.sort === state.sort.key && state.sort.asc);
  });

  $("#list-head").style.display = state.view === "startup" ? "none" : "";
  if (state.view === "startup") renderStartup(frag, q);
  else if (state.view === "apps") renderApps(frag, q);
  else if (state.view === "tree") renderTree(frag, q);
  else renderAll(frag, q);

  list.replaceChildren(frag);
  list.scrollTop = scroll;
  renderStatus();
  renderDetail();
}

function row(p, opts = {}) {
  const r = el("div", "row" + (opts.cls ? " " + opts.cls : ""));
  r.dataset.pid = p.pid;
  if (state.pick.keys.has(p.pid)) r.classList.add("selected");
  const name = el("div", "name");
  if (opts.indent) name.style.paddingLeft = opts.indent + "px";
  if (opts.disclosure !== undefined) {
    const d = el("span", "disclosure" + (opts.disclosure === null ? " none" : opts.disclosure ? " open" : ""), "▶");
    d.dataset.toggle = opts.toggleKey ?? p.pid;
    name.append(d);
  }
  name.append(iconNode(opts.icon !== undefined ? opts.icon : appBundlePath(p)));
  const label = el("span", "label", opts.label ?? p.name);
  label.title = opts.label ?? p.name;
  name.append(label);
  if (opts.badge) name.append(el("span", "badge", opts.badge));
  const cpu = opts.cpu ?? p.cpu;
  const mem = opts.memory ?? p.memory;
  const cpuEl = el("span", "num" + (cpu >= 50 ? " hot" : ""), fmtCpu(cpu));
  r.append(name, el("span", "num muted", String(p.pid)), cpuEl, el("span", "num", fmtMem(mem)), el("span", "user muted", p.user ?? "—"));
  state.rows.push(p.pid);
  return r;
}

function renderApps(frag, q) {
  const procs = state.snap.procs;
  const children = new Map(); // app pid -> helper procs
  for (const p of procs) {
    if (p.kind === "helper" && p.app_pid != null) {
      if (!children.has(p.app_pid)) children.set(p.app_pid, []);
      children.get(p.app_pid).push(p);
    }
  }
  const sort = sorter();
  const groupOf = (kind) => {
    const apps = procs.filter((p) => p.kind === kind).map((p) => {
      const kids = (children.get(p.pid) || []).slice().sort(sort);
      const cpu = p.cpu + kids.reduce((s, k) => s + k.cpu, 0);
      const memory = p.memory + kids.reduce((s, k) => s + k.memory, 0);
      return { p, kids, cpu, memory };
    }).filter(({ p, kids }) => matches(p, q) || kids.some((k) => matches(k, q)));
    const { key, asc } = state.sort;
    apps.sort((a, b) => {
      if (key === "cpu" || key === "memory") return (a[key] - b[key]) * (asc ? 1 : -1);
      return sort(a.p, b.p);
    });
    return apps;
  };

  const section = (title, items) => {
    if (!items.length) return;
    frag.append(el("div", "section-head", `${title} (${items.length})`));
    for (const { p, kids, cpu, memory } of items) {
      const open = state.expanded.has(p.pid) || (q && kids.some((k) => matches(k, q)) && !matches(p, q));
      frag.append(row(p, {
        cls: "group",
        disclosure: kids.length ? open : null,
        cpu, memory,
        label: p.app_name || p.name,
        badge: kids.length ? `${kids.length + 1} processes` : null,
      }));
      if (open) {
        for (const k of kids) {
          if (q && !matches(k, q) && !matches(p, q)) continue;
          frag.append(row(k, { cls: "child", disclosure: null }));
        }
      }
    }
  };

  section("Apps", groupOf("app"));
  section("Menu bar apps", groupOf("menubar"));

  const others = procs
    .filter((p) => p.kind === "background" || (state.showSystem && p.kind === "system"))
    .filter((p) => matches(p, q))
    .sort(sort);
  const services = others.filter((p) => p.kind === "background");
  const system = others.filter((p) => p.kind === "system");
  if (services.length) {
    frag.append(el("div", "section-head", `Other background programs (${services.length})`));
    for (const p of services) frag.append(row(p, { disclosure: null }));
  }
  if (system.length) {
    frag.append(el("div", "section-head", `macOS system (${system.length})`));
    for (const p of system) frag.append(row(p, { disclosure: null }));
  }
  if (!frag.childNodes.length) frag.append(el("div", "empty", "Nothing matches your search."));
}

function renderAll(frag, q) {
  const items = state.snap.procs
    .filter((p) => state.showSystem || p.kind !== "system")
    .filter((p) => matches(p, q))
    .sort(sorter());
  for (const p of items) frag.append(row(p, { badge: p.kind === "app" ? "App" : null }));
  if (!items.length) frag.append(el("div", "empty", "Nothing matches your search."));
}

function renderTree(frag, q) {
  const procs = state.snap.procs;
  const kids = new Map();
  for (const p of procs) {
    const parent = p.ppid != null && state.byPid.has(p.ppid) && p.ppid !== p.pid ? p.ppid : 0;
    if (!kids.has(parent)) kids.set(parent, []);
    kids.get(parent).push(p);
  }
  const sort = sorter();
  const visible = (p) => state.showSystem || p.kind !== "system" || p.pid <= 1;
  // With a search, show matches plus their ancestors.
  let keep = null;
  if (q) {
    keep = new Set();
    for (const p of procs) {
      if (!matches(p, q)) continue;
      let cur = p;
      while (cur && !keep.has(cur.pid)) {
        keep.add(cur.pid);
        cur = cur.ppid != null ? state.byPid.get(cur.ppid) : null;
      }
    }
  }
  const walk = (pid, depth, seen) => {
    const list = (kids.get(pid) || []).slice().sort(sort);
    for (const p of list) {
      if (seen.has(p.pid)) continue;
      seen.add(p.pid);
      if (keep && !keep.has(p.pid)) continue;
      // Hidden system processes still show their non-system descendants.
      if (!visible(p) && !keep) { walk(p.pid, depth, seen); continue; }
      const has = (kids.get(p.pid) || []).length > 0;
      const open = keep ? true : !state.treeCollapsed.has(p.pid);
      frag.append(row(p, { indent: depth * 16, disclosure: has ? open : null, badge: p.kind === "app" ? "App" : null }));
      if (has && open) walk(p.pid, depth + 1, seen);
    }
  };
  walk(0, 0, new Set());
  if (!frag.childNodes.length) frag.append(el("div", "empty", "Nothing matches your search."));
}

function renderStatus() {
  const s = state.snap;
  const apps = s.procs.filter((p) => p.kind === "app").length;
  const cpu = s.procs.reduce((t, p) => t + p.cpu, 0) / (s.cpu_count || 1);
  const mem = s.procs.reduce((t, p) => t + p.memory, 0);
  const st = $("#status");
  st.textContent =
    `${s.procs.length} processes · ${apps} apps open · Memory used by processes ${fmtMem(mem)} of ${fmtMem(s.total_memory)} · CPU ${cpu.toFixed(0)}%`;
  if (pickCount() > 1) st.append(` · ${pickCount()} selected`);
  st.append(sparkline(state.totalCpu, 100, "status-cpu"));
}

function renderDetail() {
  const pane = $("#detail");
  if (state.view === "startup") { delete pane.dataset.pid; renderStartupDetail(pane); return; }
  if (state.pick.keys.size > 1) { delete pane.dataset.pid; renderMultiDetail(pane); return; }
  const p = state.selected != null ? state.byPid.get(state.selected) : null;
  if (!p || (state.selectedStart != null && p.start_time !== state.selectedStart)) {
    if (state.selected != null && !p) {
      pane.replaceChildren(el("div", "empty", "That process has ended."));
    } else if (state.selected == null) {
      pane.replaceChildren(el("div", "empty", "Select a process to see what it is, what started it, and to stop it. ⌘-click or ⇧-click to pick several."));
    } else {
      pane.replaceChildren(el("div", "empty", "That process has ended."));
    }
    return;
  }
  // Keep the pane stable while the user is selecting text in it.
  const sel = window.getSelection();
  if (sel && !sel.isCollapsed && pane.contains(sel.anchorNode) && pane.dataset.pid == p.pid) {
    const live = pane.querySelector("[data-live]");
    if (live) live.textContent = `${fmtCpu(p.cpu)} CPU · ${fmtMem(p.memory)}`;
    return;
  }
  pane.dataset.pid = p.pid;

  const out = document.createDocumentFragment();
  const head = el("div", "d-head");
  head.append(iconNode(appBundlePath(p)));
  head.lastChild.classList.add("big");
  const t = el("div");
  t.append(el("div", "d-title", p.kind === "app" || p.kind === "menubar" ? (p.app_name || p.name) : p.name));
  t.append(el("div", "d-sub", KIND_LABEL[p.kind] + (p.protected ? " · protected" : "")));
  head.append(t);
  out.append(head);

  out.append(el("div", "d-desc", p.description));
  out.append(graphs(p));

  // Who is running it
  out.append(el("div", "d-section", "Program running it"));
  const who = el("div");
  if (p.kind === "app" || p.kind === "menubar") {
    who.textContent = `This is the app itself.`;
  } else if (p.app_pid != null) {
    who.append("Belongs to the app ");
    who.append(pidLink(p.app_pid, p.app_name));
    who.append(".");
  } else if (p.responsible) {
    who.append("macOS says ");
    who.append(pidLink(p.responsible.pid, p.responsible.name));
    who.append(" is responsible for it.");
  } else if (p.launched_by.length) {
    const first = p.launched_by[0];
    if (first.pid === 1) who.textContent = "Started directly by macOS (launchd), not by an app you opened.";
    else { who.append("Started by "); who.append(pidLink(first.pid, first.name)); who.append("."); }
  } else {
    who.textContent = "macOS hides this for processes owned by the system or another user. Most of these are started by macOS itself.";
  }
  out.append(who);

  if (p.launched_by.length) {
    out.append(el("div", "d-section", "Launched by (parent chain)"));
    const ul = el("ul", "chain");
    for (const a of p.launched_by) {
      const li = el("li");
      li.append("↳ ");
      li.append(pidLink(a.pid, a.name));
      ul.append(li);
    }
    out.append(ul);
  }

  out.append(el("div", "d-section", "Details"));
  const dl = el("dl", "d-grid");
  const add = (k, v, mono) => {
    if (v == null || v === "") return;
    dl.append(el("dt", null, k));
    const dd = el("dd", mono ? "mono" : null);
    if (v instanceof Node) dd.append(v); else dd.textContent = v;
    dl.append(dd);
  };
  add("PID", String(p.pid));
  const liveNode = el("span", null, `${fmtCpu(p.cpu)} CPU · ${fmtMem(p.memory)}`);
  liveNode.dataset.live = "1";
  add("Using", liveNode);
  add("User", p.user ? (p.is_root ? `${p.user} (administrator)` : p.user) : "unknown");
  add("Running for", fmtDuration(p.run_time));
  add("Started", new Date(p.start_time * 1000).toLocaleString());
  if (p.exe) {
    const exe = el("span");
    exe.append(p.exe + " ");
    const rv = el("a", "link", "Show in Finder");
    rv.onclick = () => invoke("reveal", { path: appBundlePath(p) && (p.kind === "app" || p.kind === "menubar") ? appBundlePath(p) : p.exe }).catch((e) => toast(String(e), true));
    exe.append(rv);
    add("Program file", exe, true);
  } else {
    add("Program file", "Hidden by macOS (owned by the system or another user)");
  }
  const app = state.snap.apps.find((a) => a.pid === p.pid);
  if (app && app.bundle_id) add("Bundle ID", app.bundle_id, true);
  out.append(dl);

  if (p.command) {
    out.append(el("div", "d-section", "Command"));
    out.append(el("div", "cmd mono", p.command));
  }

  const search = el("a", "link", "Search the web for this process ↗");
  search.onclick = () => invoke("search_web", { name: p.name }).catch((e) => toast(String(e), true));
  const sp = el("div", "note");
  sp.append(search);
  out.append(sp);

  // Actions
  out.append(el("div", "d-section", "Stop it"));
  const actions = el("div", "actions");
  const self = p.pid === state.snap.self_pid;
  if (p.protected || self) {
    out.append(el("div", "note warn-note", self
      ? "This is Process Viewer itself."
      : "This is a core part of macOS. Stopping it would log you out or freeze the Mac, so it can't be stopped from here."));
  } else {
    const isApp = p.kind === "app" || p.kind === "menubar";
    const quit = el("button", "btn danger", isApp ? "Quit" : "End process");
    quit.onclick = () => stop(p, "quit");
    const force = el("button", "btn danger-outline", "Force quit");
    force.onclick = () => stop(p, "force");
    actions.append(quit, force);
    const mine = p.user && state.snap.procs.find((x) => x.pid === state.snap.self_pid)?.user === p.user;
    if (!mine) {
      const admin = el("button", "btn", "Force quit as admin…");
      admin.onclick = () => stop(p, "admin");
      actions.append(admin);
    }
    out.append(actions);
    const note = isApp
      ? "Quit asks the app to close normally, so it can offer to save your work. Force quit stops it straight away; unsaved work is lost."
      : p.kind === "helper"
        ? `Stopping a helper can make part of ${p.app_name || "its app"} stop working or reload. The app may start it again.`
        : p.kind === "system"
          ? "This is part of macOS. It's usually safe to leave alone; macOS will often restart it if stopped."
          : "End process asks it to stop cleanly. Force quit stops it straight away. Background programs may be restarted automatically.";
    out.append(el("div", "note", note));
  }

  pane.replaceChildren(out);
}

function pidLink(pid, name) {
  const a = el("a", null, `${name || "?"} (${pid})`);
  a.onclick = () => {
    if (state.view === "startup") switchView("all");
    select(pid, true);
  };
  return a;
}

function select(pid, reveal) {
  const p = state.byPid.get(pid);
  setPick(MultiSelect.click(state.pick, state.rows, pid));
  if (reveal && p) {
    // Make sure it's visible: open its app group / show system.
    if (p.app_pid != null && p.app_pid !== pid) state.expanded.add(p.app_pid);
    if (p.kind === "system" && !state.showSystem) { state.showSystem = true; $("#show-system").checked = true; }
    if (state.query && !matches(p, state.query.trim().toLowerCase())) { state.query = ""; $("#search").value = ""; }
  }
  render();
  const r = document.querySelector(`.row[data-pid="${pid}"]`);
  if (r) r.scrollIntoView({ block: "nearest" });
}

// ---------- stopping ----------
async function stop(p, mode) {
  const isApp = p.kind === "app" || p.kind === "menubar";
  const name = isApp ? (p.app_name || p.name) : p.name;
  const titles = {
    quit: isApp ? `Quit “${name}”?` : `End “${name}”?`,
    force: `Force quit “${name}”?`,
    admin: `Force quit “${name}” as administrator?`,
  };
  let body = {
    quit: isApp ? "The app will close normally and may ask you to save your work." : "The process will be asked to stop.",
    force: "It will be stopped immediately. Any unsaved work in it will be lost.",
    admin: "macOS will ask for your password. The process will be stopped immediately.",
  }[mode];
  const helpers = state.snap.procs.filter((x) => x.app_pid === p.pid && x.pid !== p.pid).length;
  if (isApp && helpers) body += ` Its ${helpers} helper process${helpers > 1 ? "es" : ""} will close with it.`;
  if (p.kind === "system") body += " This is part of macOS — only do this if you know why.";

  const ok = await confirmDialog(titles[mode], body,
    mode === "quit" ? (isApp ? "Quit" : "End process") : "Force quit");
  if (!ok) return;

  try {
    const msg = await invoke("terminate", { pid: p.pid, startTime: p.start_time, mode });
    toast(msg);
  } catch (e) {
    toast(String(e), true);
  }
  setTimeout(refresh, 400);
  setTimeout(refresh, 1500);
}

const nameOf = (p) => (p.kind === "app" || p.kind === "menubar") ? (p.app_name || p.name) : p.name;

function namesText(names) {
  const shown = names.slice(0, 5).join(", ");
  return names.length > 5 ? `${shown} and ${names.length - 5} more` : shown;
}

function namesList(names) {
  const ul = el("ul", "pick-list");
  for (const n of names.slice(0, 10)) ul.append(el("li", null, n));
  if (names.length > 10) ul.append(el("li", "muted", `and ${names.length - 10} more`));
  return ul;
}

/** Toast a summary of a batch result: [{ ok, message }]. */
function reportMany(outcomes, doneText) {
  const failed = outcomes.filter((o) => !o.ok);
  const ok = outcomes.length - failed.length;
  if (!failed.length) { toast(doneText(ok)); return; }
  if (failed.every((o) => o.message === "Cancelled.")) { toast("Cancelled.", true); return; }
  const lead = ok ? doneText(ok) + " " : "";
  toast(`${lead}${plural(failed.length, "item")} didn't work: ${failed[0].message}`, true);
}

function renderMultiDetail(pane) {
  const ps = pickedProcs();
  const out = document.createDocumentFragment();
  const head = el("div", "d-head");
  const h = el("div");
  h.append(el("div", "d-title", `${plural(ps.length, "process", "processes")} selected`));
  // Count apps with their helpers, as the rows do, without counting any
  // process twice.
  const counted = new Set(ps.map((p) => p.pid));
  for (const p of ps) {
    if (p.kind !== "app" && p.kind !== "menubar") continue;
    for (const x of state.snap.procs) if (x.app_pid === p.pid) counted.add(x.pid);
  }
  let cpu = 0;
  let mem = 0;
  for (const pid of counted) {
    const x = state.byPid.get(pid);
    if (x) { cpu += x.cpu; mem += x.memory; }
  }
  h.append(el("div", "d-sub", `${fmtCpu(cpu)} CPU · ${fmtMem(mem)} together (apps include their helpers)`));
  head.append(h);
  out.append(head, namesList(ps.map(nameOf)));

  out.append(el("div", "d-section", "Stop them"));
  const self = state.snap.self_pid;
  const stoppable = ps.filter((p) => !p.protected && p.pid !== self);
  const skipped = ps.length - stoppable.length;
  if (skipped) {
    out.append(el("div", "note warn-note",
      `${plural(skipped, "of these is", "of these are")} core macOS or this app, so ${skipped === 1 ? "it" : "they"} will be left alone.`));
  }
  if (stoppable.length) {
    const actions = el("div", "actions");
    const quit = el("button", "btn danger", `Quit / end ${stoppable.length}`);
    quit.onclick = () => stopMany(stoppable, "quit");
    const force = el("button", "btn danger-outline", `Force quit ${stoppable.length}`);
    force.onclick = () => stopMany(stoppable, "force");
    actions.append(quit, force);
    const me = state.byPid.get(self)?.user;
    if (stoppable.some((p) => !p.user || p.user !== me)) {
      const admin = el("button", "btn", "Force quit as admin…");
      admin.onclick = () => stopMany(stoppable, "admin");
      actions.append(admin);
    }
    out.append(actions);
    out.append(el("div", "note",
      "Quit asks apps to close normally, so they can offer to save your work. Force quit stops everything straight away; unsaved work is lost. As admin asks for your password once for all of them."));
  }
  out.append(el("div", "note", "⌘-click to add or remove a process, ⇧-click for a range, Esc to go back to one."));
  pane.replaceChildren(out);
}

async function stopMany(ps, mode) {
  // Helpers of a picked app close with it when it quits, so leave them out.
  const picked = new Set(ps.map((p) => p.pid));
  const targets = mode === "quit"
    ? ps.filter((p) => !(p.kind === "helper" && p.app_pid != null && picked.has(p.app_pid)))
    : ps;
  if (!targets.length) return;
  const n = plural(targets.length, "process", "processes");
  const titles = {
    quit: `Quit or end ${n}?`,
    force: `Force quit ${n}?`,
    admin: `Force quit ${n} as administrator?`,
  };
  const body = {
    quit: "Apps will close normally and may ask you to save your work; other processes will be asked to stop.",
    force: "They will be stopped immediately. Any unsaved work in them will be lost.",
    admin: "macOS will ask for your password once. They will be stopped immediately.",
  }[mode];
  const sys = targets.some((p) => p.kind === "system") ? " Some are part of macOS — only do this if you know why." : "";
  const ok = await confirmDialog(titles[mode], `${namesText(targets.map(nameOf))}. ${body}${sys}`,
    mode === "quit" ? "Quit / end" : "Force quit");
  if (!ok) return;
  try {
    const out = await invoke("terminate_many", {
      targets: targets.map((p) => ({ pid: p.pid, startTime: p.start_time })),
      mode,
    });
    reportMany(out, (k) => mode === "quit" ? `Asked ${plural(k, "process", "processes")} to stop.` : `Force quit ${plural(k, "process", "processes")}.`);
  } catch (e) {
    toast(String(e), true);
  }
  setTimeout(refresh, 400);
  setTimeout(refresh, 1500);
}

let toastTimer;
function toast(msg, error) {
  const t = $("#toast");
  t.textContent = msg;
  t.className = "toast" + (error ? " error" : "");
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), error ? 6000 : 3500);
}

// ---------- events ----------
document.querySelectorAll(".tab").forEach((b) =>
  b.addEventListener("click", () => switchView(b.dataset.view))
);
let searchTimer;
$("#search").addEventListener("input", (e) => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(() => { state.query = e.target.value; render(); }, 80);
});
$("#show-system").addEventListener("change", (e) => { state.showSystem = e.target.checked; render(); });
$("#live").addEventListener("change", (e) => { state.live = e.target.checked; });
$("#refresh").addEventListener("click", async () => {
  if (state.view === "startup") await loadStartup();
  refresh();
});
document.querySelectorAll(".sortable").forEach((s) =>
  s.addEventListener("click", () => {
    const key = s.dataset.sort;
    if (state.sort.key === key) state.sort.asc = !state.sort.asc;
    else state.sort = { key, asc: key === "name" || key === "user" || key === "pid" };
    render();
  })
);
$("#list").addEventListener("click", (e) => {
  const tog = e.target.closest(".disclosure");
  const r = e.target.closest(".row");
  if (tog && !tog.classList.contains("none")) {
    const pid = Number(tog.dataset.toggle);
    if (state.view === "tree") {
      state.treeCollapsed.has(pid) ? state.treeCollapsed.delete(pid) : state.treeCollapsed.add(pid);
    } else {
      state.expanded.has(pid) ? state.expanded.delete(pid) : state.expanded.add(pid);
    }
    render();
    return;
  }
  if (!r || (!r.dataset.plist && !r.dataset.pid)) return;
  const key = r.dataset.plist ?? Number(r.dataset.pid);
  const mods = { toggle: e.metaKey, extend: e.shiftKey };
  applyPick((sel) => MultiSelect.click(sel, state.rows, key, mods));
});
$("#list").addEventListener("dblclick", (e) => {
  const r = e.target.closest(".row.group");
  if (!r) return;
  const pid = Number(r.dataset.pid);
  state.expanded.has(pid) ? state.expanded.delete(pid) : state.expanded.add(pid);
  render();
});
document.addEventListener("keydown", (e) => {
  if ((e.metaKey && e.key === "f")) { e.preventDefault(); $("#search").focus(); return; }
  if ((e.metaKey && e.key === "r")) { e.preventDefault(); refresh(); return; }
  if ($("#confirm").open || e.target === $("#search")) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const delta = e.key === "ArrowDown" ? 1 : -1;
    applyPick((sel) => MultiSelect.step(sel, state.rows, delta, e.shiftKey));
    return;
  }
  if (e.metaKey && e.key === "a") {
    e.preventDefault();
    applyPick(() => MultiSelect.all(state.rows));
    return;
  }
  if (e.key === "Escape" && pickCount() > 1) {
    applyPick((sel) => sel.focus != null ? MultiSelect.click(sel, state.rows, sel.focus) : MultiSelect.empty());
    return;
  }
  if (state.view === "startup") return;
  if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && state.selected != null) {
    const set = state.view === "tree" ? state.treeCollapsed : state.expanded;
    const open = e.key === "ArrowRight";
    if (state.view === "tree") open ? set.delete(state.selected) : set.add(state.selected);
    else open ? set.add(state.selected) : set.delete(state.selected);
    render();
  } else if ((e.key === "Backspace" || e.key === "Delete") && e.metaKey && state.pick.keys.size > 1) {
    const ps = pickedProcs().filter((p) => !p.protected && p.pid !== state.snap.self_pid);
    if (ps.length) stopMany(ps, "quit");
  } else if ((e.key === "Backspace" || e.key === "Delete") && e.metaKey && state.selected != null) {
    const p = state.byPid.get(state.selected);
    if (p && !p.protected) stop(p, "quit");
  }
});

setInterval(() => {
  if (state.live && !document.hidden && !$("#confirm").open) refresh();
}, 2000);
refresh();
