'use strict';

/* ================================================================ basics */
const TOKEN = new URLSearchParams(location.search).get('t') || '';
const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => [...el.querySelectorAll(sel)];
const esc = (v) => String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const store = {
  get(key, fallback) { try { const v = localStorage.getItem('sshpro.' + key); return v === null ? fallback : JSON.parse(v); } catch { return fallback; } },
  set(key, value) { try { localStorage.setItem('sshpro.' + key, JSON.stringify(value)); } catch { /* private mode */ } },
};

const ICONS = {
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-2.64-6.36L21 8"/><path d="M21 3v5h-5"/>',
  terminal: '<polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>',
  sparkles: '<path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z"/><path d="M19 3v4M17 5h4M5 17v3M3.5 18.5h3"/>',
  server: '<rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  grid: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
  broadcast: '<circle cx="12" cy="12" r="2"/><path d="M16.24 7.76a6 6 0 0 1 0 8.49M7.76 16.24a6 6 0 0 1 0-8.49M19.07 4.93a10 10 0 0 1 0 14.14M4.93 19.07a10 10 0 0 1 0-14.14"/>',
  edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
  trash: '<path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/>',
  import: '<path d="M12 3v12M7 10l5 5 5-5"/><path d="M5 21h14"/>',
  export: '<path d="M12 15V3M7 8l5-5 5 5"/><path d="M5 21h14"/>',
  history: '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l3 2"/>',
  chev: '<path d="m9 6 6 6-6 6"/>',
  send: '<path d="M22 2 11 13"/><path d="M22 2 15 22l-4-9-9-4z"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  zap: '<path d="M13 2 3 14h9l-1 8 10-12h-9z"/>',
  shield: '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>',
  alert: '<path d="M12 9v4M12 17h.01"/><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/>',
  pin: '<path d="M12 17v5M9 3h6l-1 7 4 3v2H6v-2l4-3z"/>',
  copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>',
  activity: '<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>',
  disk: '<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>',
  cpu: '<rect x="5" y="5" width="14" height="14" rx="2"/><path d="M9 9h6v6H9zM9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/>',
  key: '<circle cx="7.5" cy="15.5" r="4.5"/><path d="m10.7 12.3 9.3-9.3M17 6l3 3M14 9l2 2"/>',
  wifi_off: '<path d="M2 2l20 20"/><path d="M8.5 16.5a5 5 0 0 1 7 0M5 12.9a10 10 0 0 1 5.2-2.8M19 12.9a10 10 0 0 0-2-1.5M2 8.8a15 15 0 0 1 4.2-2.6M22 8.8a15 15 0 0 0-11.9-3.7"/><path d="M12 20h.01"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  layers: '<path d="m12 2 10 5-10 5L2 7z"/><path d="m2 17 10 5 10-5M2 12l10 5 10-5"/>',
  package: '<path d="M16.5 9.4 7.5 4.2M21 16V8a2 2 0 0 0-1-1.7l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.7l7 4a2 2 0 0 0 2 0l7-4a2 2 0 0 0 1-1.7z"/><path d="M3.3 7 12 12l8.7-5M12 22V12"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7"/><path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7"/>',
  loader: '<path d="M21 12a9 9 0 1 1-6.2-8.6"/>',
  sliders: '<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="18" cy="18" r="2"/>',
  minus: '<path d="M5 12h14"/>',
  star: '<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.3 6.4 20.2l1.1-6.2L3 9.6l6.2-.9z"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>',
  reconnect: '<path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/><path d="M3 21v-5h5"/>',
};
const svg = (name) => `<svg viewBox="0 0 24 24">${ICONS[name] || ''}</svg>`;
const icon = (name) => `<span class="i">${svg(name)}</span>`;
function hydrateIcons(root = document) {
  $$('[data-icon]', root).forEach((el) => { if (!el.firstElementChild) el.innerHTML = svg(el.dataset.icon); });
}

async function api(path, body, method) {
  const res = await fetch(path, {
    method: method || (body === undefined ? 'GET' : 'POST'),
    headers: { 'X-Token': TOKEN, 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}

function toast(text, kind = 'info') {
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  el.innerHTML = icon(kind === 'ok' ? 'check' : kind === 'bad' ? 'alert' : 'zap') + `<span>${esc(text)}</span>`;
  $('#toasts').appendChild(el);
  setTimeout(() => { el.classList.add('out'); setTimeout(() => el.remove(), 300); }, kind === 'bad' ? 6000 : 3500);
}

function fmtUptime(sec) {
  if (!sec) return '—';
  const d = Math.floor(sec / 86400), h = Math.floor((sec % 86400) / 3600), m = Math.floor((sec % 3600) / 60);
  return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${m}m`;
}
const level = (pct, warn = 75, bad = 90) => (pct >= bad ? 'bad' : pct >= warn ? 'warn' : '');

/* ================================================================ state */
const S = {
  servers: new Map(),
  probing: new Set(),
  probeErr: new Map(),
  selected: new Set(),
  collapsed: new Set(store.get('collapsed', [])),
  view: 'overview',
  folder: 'all',
  mode: 'ai',
  // target / custom / mode are the shown chat window's (each window keeps its own; see activateWin)
  target: 'all',
  custom: new Set(), // hand-picked servers from any folders (the "Custom" target)
  targetSets: store.get('targetSets', []), // saved custom selections: [{ name, ids }]
  terms: new Map(),
  activeTerm: null,
  tile: false,
  broadcast: false,
  wins: new Map(), // chat windows (they work at the same time), in tab order
  win: null, // id of the window shown
  lastChat: 0, // highest chat id seen
  meta: {},
};
let termSeq = 0;

const serverList = () => [...S.servers.values()];
const status = (s) => (S.probing.has(s.id) ? 'checking' : s.last_status || 'unknown');
const folderName = (s) => s.folder || 'Ungrouped';
const folders = () => [...new Set(serverList().map(folderName))].sort((a, b) => a.localeCompare(b));

let renderQueued = false;
function render() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    renderSidebar();
    renderOverview();
    renderTargets();
  });
}

/* ================================================================ sidebar */
function serverRow(s) {
  const st = status(s);
  const disk = s.facts?.disk?.used_pct;
  const mini = st === 'online' && disk != null ? `<span class="mini ${level(disk, 80, 90)}" title="Disk used">${Math.round(disk)}%</span>` : '';
  const active = S.activeTerm && S.terms.get(S.activeTerm)?.serverId === s.id && S.view === 'terms';
  return `<div class="srv ${S.selected.has(s.id) ? 'selected' : ''} ${active ? 'active' : ''}" data-id="${s.id}" title="${esc(s.name)} — click: terminal · Ctrl+click: select">
    <div class="cb" data-act="sel">${svg('check')}</div>
    <div class="dot-wrap"><span class="dot ${st}"></span></div>
    <div class="srv-main">
      <div class="srv-name">${s.pinned ? icon('pin') : ''}<span>${esc(s.name)}</span></div>
      <div class="srv-meta">${esc(s.username)}@${esc(s.host)}${s.port !== 22 ? ':' + s.port : ''}</div>
    </div>
    ${mini}
    <div class="srv-tools">
      <button data-act="term" title="Open terminal">${icon('terminal')}</button>
      <button data-act="edit" title="Edit">${icon('edit')}</button>
    </div>
  </div>`;
}

function renderSidebar() {
  const q = $('#search').value.trim().toLowerCase();
  const list = serverList().filter((s) => !q || [s.name, s.host, s.username, s.folder, s.tags, s.facts?.os].join(' ').toLowerCase().includes(q));
  const groups = new Map();
  const pinned = list.filter((s) => s.pinned);
  if (pinned.length) groups.set('★ Pinned', pinned);
  for (const s of list) {
    if (s.pinned) continue;
    const f = folderName(s);
    if (!groups.has(f)) groups.set(f, []);
    groups.get(f).push(s);
  }
  const el = $('#serverList');
  el.classList.toggle('selecting', S.selected.size > 0);
  if (!S.servers.size) {
    el.innerHTML = `<div class="empty-state" style="margin:8px 6px;padding:28px 14px">${icon('server')}<p style="margin:8px 0 0">No servers yet</p></div>`;
  } else if (!list.length) {
    el.innerHTML = `<p class="sub" style="padding:10px 12px">No server matches “${esc(q)}”.</p>`;
  } else {
    el.innerHTML = [...groups].map(([name, items]) => {
      const online = items.filter((s) => s.last_status === 'online').length;
      return `<div class="group ${S.collapsed.has(name) && !q ? 'collapsed' : ''}" data-group="${esc(name)}">
        <button class="group-head" data-act="toggle">${icon('chev').replace('class="i"', 'class="i chev"')}<span>${esc(name)}</span>
          <span class="count" title="online / total">${online}/${items.length}</span>
          ${name === '★ Pinned' ? '' : '<span class="gsel link" data-act="gask" title="Ask the AI about this folder only">ask AI</span>'}
          <span class="gsel link" data-act="gsel" title="Select all in this group">select</span></button>
        <div class="group-items">${items.map(serverRow).join('')}</div>
      </div>`;
    }).join('');
  }
  const n = S.selected.size;
  $('#selectionBar').hidden = !n;
  $('#selectionText').textContent = `${n} server${n === 1 ? '' : 's'} selected`;
}

$('#serverList').addEventListener('click', (e) => {
  const act = e.target.closest('[data-act]')?.dataset.act;
  const group = e.target.closest('.group');
  if (act === 'gsel' && group) {
    e.stopPropagation();
    const ids = $$('.srv', group).map((r) => +r.dataset.id);
    const all = ids.every((id) => S.selected.has(id));
    ids.forEach((id) => (all ? S.selected.delete(id) : S.selected.add(id)));
    return render();
  }
  if (act === 'gask' && group) {
    e.stopPropagation();
    setTarget(`folder:${group.dataset.group}`);
    return showView('ai');
  }
  if (act === 'toggle' && group) {
    const name = group.dataset.group;
    S.collapsed.has(name) ? S.collapsed.delete(name) : S.collapsed.add(name);
    store.set('collapsed', [...S.collapsed]);
    return group.classList.toggle('collapsed');
  }
  const row = e.target.closest('.srv');
  if (!row) return;
  const id = +row.dataset.id;
  if (act === 'edit') return openServerModal(S.servers.get(id));
  if (act === 'sel' || e.ctrlKey || e.shiftKey || (S.selected.size && act !== 'term')) {
    S.selected.has(id) ? S.selected.delete(id) : S.selected.add(id);
    return render();
  }
  openTerminal(id);
});
$('#search').addEventListener('input', renderSidebar);
$('#btnClearSel').onclick = () => { S.selected.clear(); render(); };
$('#btnOpenSelected').onclick = () => {
  const ids = [...S.selected];
  ids.forEach((id) => openTerminal(id, false));
  if (ids.length > 1) setTile(true);
  showView('terms');
};
$('#btnRefresh').onclick = async () => {
  await api('/api/probe', {});
  toast('Checking all servers…');
};

/* ================================================================ overview */
function renderOverview() {
  const all = serverList();
  const fs = folders();
  if (S.folder !== 'all' && !fs.includes(S.folder)) S.folder = 'all';
  $('#folderChips').innerHTML = [['all', `All <b>${all.length}</b>`], ...fs.map((f) => [f, `${esc(f)} <b>${all.filter((s) => folderName(s) === f).length}</b>`])]
    .map(([k, label]) => `<button class="chip ${S.folder === k ? 'on' : ''}" data-folder="${esc(k)}">${label}</button>`).join('');

  const online = all.filter((s) => s.last_status === 'online');
  const offline = all.filter((s) => s.last_status === 'offline');
  const attention = all.filter((s) => s.last_status === 'offline' || (s.facts?.disk?.used_pct ?? 0) >= 85 || (s.facts?.mem?.used_pct ?? 0) >= 90);
  const freeTb = online.reduce((a, s) => a + (s.facts?.disk?.free_gb || 0), 0);
  const checking = S.probing.size;
  $('#ovSub').textContent = checking ? `Checking ${checking} server${checking === 1 ? '' : 's'}…` : `Live health of ${all.length} servers · ${fs.length} folder${fs.length === 1 ? '' : 's'}`;
  $('#stats').innerHTML = [
    { label: 'Servers', icon: 'server', value: all.length, note: `${fs.length} folders`, c: 'linear-gradient(90deg,#6366f1,#a855f7)' },
    { label: 'Online', icon: 'activity', value: `${online.length}<small>/ ${all.length}</small>`, note: checking ? 'checking…' : 'reachable over SSH', c: 'linear-gradient(90deg,#34d399,#22d3ee)' },
    { label: 'Need attention', icon: 'alert', value: attention.length, note: `${offline.length} offline · disk ≥85% · RAM ≥90%`, c: attention.length ? 'linear-gradient(90deg,#f59e0b,#ef4444)' : 'linear-gradient(90deg,#34d399,#22d3ee)' },
    { label: 'Free disk (/)', icon: 'disk', value: freeTb >= 1024 ? `${(freeTb / 1024).toFixed(1)}<small>TB</small>` : `${Math.round(freeTb)}<small>GB</small>`, note: 'across online servers', c: 'linear-gradient(90deg,#22d3ee,#6366f1)' },
  ].map((t) => `<div class="stat" style="--c:${t.c}"><div class="stat-label">${icon(t.icon)}${t.label}</div><div class="stat-value">${t.value}</div><div class="stat-note">${t.note}</div></div>`).join('');

  const shown = all.filter((s) => S.folder === 'all' || folderName(s) === S.folder);
  if (!all.length) {
    $('#cards').innerHTML = `<div class="empty-state"><h3>No servers yet</h3><p>Add a server, or import the ones from Termix (ssh.sqlite).</p>
      <button class="btn btn-primary" onclick="openServerModal()">${icon('plus')}Add server</button>
      <button class="btn" onclick="importTermix()">${icon('import')}Import Termix</button></div>`;
    return;
  }
  $('#cards').innerHTML = shown.map(card).join('');
}

function metric(label, pct, right) {
  if (pct == null) return '';
  return `<div><div class="metric-row"><span>${label}</span><b>${right}</b></div><div class="bar ${level(pct)}"><i style="width:${Math.min(100, Math.max(2, pct))}%"></i></div></div>`;
}

function card(s) {
  const st = status(s);
  const f = s.facts || {};
  const cpuPct = f.load && f.cpus ? Math.round((f.load[0] / f.cpus) * 100) : null;
  const label = { online: 'Online', offline: 'Offline', checking: 'Checking', unknown: 'Unknown' }[st];
  let body;
  if (st === 'checking' && !s.facts) {
    body = `<div class="metrics"><div class="skel" style="height:30px"></div><div class="skel" style="height:30px"></div><div class="skel" style="height:30px"></div></div>`;
  } else if (st === 'offline') {
    body = `<div class="card-err">${esc(S.probeErr.get(s.id) || 'Not reachable')}</div>`;
  } else {
    body = `<div class="metrics">
      ${metric('CPU load', cpuPct, f.load ? `${f.load[0].toFixed(2)} · ${f.cpus} cores` : '—')}
      ${metric('Memory', f.mem?.used_pct, f.mem ? `${f.mem.used_pct}% of ${(f.mem.total_mb / 1024).toFixed(1)} GB` : '—')}
      ${metric('Disk /', f.disk?.used_pct, f.disk ? `${f.disk.free_gb} GB free · ${f.disk.used_pct}%` : '—')}
    </div>`;
  }
  return `<div class="card ${st}" data-id="${s.id}">
    <div class="card-top">
      <div class="card-title"><div class="card-name">${esc(s.name)}</div><div class="card-os">${esc(f.os || `${s.username}@${s.host}`)}</div></div>
      <span class="pill ${st}"><span class="dot ${st}"></span>${label}</span>
    </div>
    ${body}
    <div class="card-foot">
      <span class="meta">${f.uptime ? 'up ' + fmtUptime(f.uptime) + ' · ' : ''}${esc(folderName(s))}</span>
      <button class="btn btn-ghost btn-sm btn-icon" data-act="edit" title="Edit">${icon('edit')}</button>
      <button class="btn btn-ghost btn-sm" data-act="ask" title="Ask the AI about this server">${icon('sparkles')}Ask</button>
      <button class="btn btn-sm" data-act="term">${icon('terminal')}Terminal</button>
    </div>
  </div>`;
}

$('#folderChips').addEventListener('click', (e) => {
  const chip = e.target.closest('[data-folder]');
  if (chip) { S.folder = chip.dataset.folder; renderOverview(); }
});
$('#cards').addEventListener('click', (e) => {
  const act = e.target.closest('[data-act]')?.dataset.act;
  const id = +e.target.closest('.card')?.dataset.id;
  if (!act || !id) return;
  if (act === 'term') openTerminal(id);
  if (act === 'edit') openServerModal(S.servers.get(id));
  if (act === 'ask') { setTarget(`server:${id}`); showView('ai'); $('#prompt').focus(); }
});

/* ================================================================ views / tabs */
function showView(view) {
  S.view = view;
  $$('.view').forEach((v) => v.classList.toggle('on', v.id === `view-${view}`));
  $$('#tabs > .tab').forEach((t) => t.classList.toggle('on', t.dataset.view === view));
  $$('#termTabs .tab').forEach((t) => t.classList.toggle('on', view === 'terms' && t.dataset.term === S.activeTerm));
  $('#termActions').hidden = view !== 'terms' || !S.terms.size;
  if (view === 'terms') requestAnimationFrame(fitVisible);
  if (view === 'ai') setTimeout(() => $('#prompt').focus(), 30);
  renderSidebar();
}
$$('#tabs > .tab').forEach((t) => (t.onclick = () => showView(t.dataset.view)));

/* ================================================================ AI Ops */
const SUGGESTIONS = [
  { ico: 'disk', c: 'c', title: 'Free disk space', text: 'Check free disk space on all servers and flag anything above 80%', prompt: 'Check free disk space on all servers and flag anything above 80% used' },
  { ico: 'layers', c: '', title: 'PHP versions', text: 'Which PHP version runs where, and which servers have no PHP', prompt: 'Sob server er PHP version check koro, ar kon server e PHP nai seta bolo' },
  { ico: 'package', c: 'g', title: 'Node.js versions', text: 'Node.js version on every server, flag missing or outdated', prompt: 'Sob server er Node.js version check koro, kono server e Node.js na thakle janao' },
  { ico: 'shield', c: 'o', title: 'Pending security updates', text: 'Count pending (security) package updates per server', prompt: 'How many package updates (and security updates) are pending on each server?' },
  // these need your server names: they fill the box instead of sending
  { ico: 'layers', c: 'g', fill: true, title: 'Move GeoServer', text: 'GeoServer with its data dir, verified, nothing lost', prompt: 'Move GeoServer with all its data from SOURCE to TARGET without losing anything. Inspect both first, then show me the plan.' },
  { ico: 'disk', c: 'c', fill: true, title: 'Move a database', text: 'PostgreSQL / MySQL to another server, row counts checked', prompt: 'Move the database DBNAME from SOURCE to TARGET without data loss, then compare the row counts of every table.' },
];
$('#suggest').innerHTML = SUGGESTIONS.map((s, i) => `<div class="sg" data-i="${i}"><div class="ico ${s.c}">${icon(s.ico)}</div><div><b>${s.title}</b><span>${s.text}</span></div></div>`).join('');
$('#suggest').addEventListener('click', (e) => {
  const sg = e.target.closest('.sg');
  if (!sg) return;
  setMode('ai');
  const sug = SUGGESTIONS[+sg.dataset.i];
  $('#prompt').value = sug.prompt;
  autoGrow();
  if (!sug.fill) return send();
  const t = $('#prompt'); // select the first placeholder to type over
  const at = t.value.search(/\b(SOURCE|DBNAME)\b/);
  t.focus();
  if (at >= 0) t.setSelectionRange(at, at + t.value.slice(at).match(/^\w+/)[0].length);
});

function renderTargets() {
  const all = serverList();
  const count = (f) => all.filter((s) => folderName(s) === f).length;
  const online = (ids) => ids.filter((id) => S.servers.get(id)?.last_status === 'online').length;
  if (S.target === 'selected' && !S.selected.size) S.target = 'all';
  if (S.servers.size) { // the saved target is only checked once the server list has loaded
    if (S.target.startsWith('folder:') && !folders().includes(S.target.slice(7))) S.target = 'all';
    if (S.target.startsWith('server:') && !S.servers.has(+S.target.slice(7))) S.target = 'all';
    pruneCustom();
  }
  const chips = [['all', 'All servers', all.length, icon('layers')], ...folders().map((f) => [`folder:${f}`, f, count(f), icon('folder')])];
  if (S.selected.size) chips.push(['selected', 'Selected', S.selected.size, icon('check')]);
  const tip = (label, n) => `${label}: ${n} servers · Ctrl+click to add/remove it in a custom selection`;
  $('#targetChips').innerHTML = chips.map(([value, label, n, ic]) =>
    `<button class="chip ${S.target === value ? 'on' : ''}" data-target="${esc(value)}" title="${esc(tip(label, n))}">${ic}${esc(label)} <b>${n}</b></button>`).join('')
    + `<button class="chip chip-custom ${S.target === 'custom' ? 'on' : ''}" data-target="custom" title="Pick any servers from any folders">${icon('sliders')}Custom <b>${S.custom.size}</b></button>`;
  const sel = $('#targetSel');
  sel.innerHTML = `<option value="">One server…</option>` + all.map((s) => `<option value="server:${s.id}">${esc(s.name)}</option>`).join('');
  sel.value = S.target.startsWith('server:') ? S.target : '';
  sel.classList.toggle('on', !!sel.value);
  const ids = targetIds();
  $('#targetInfo').textContent = `${online(ids)}/${ids.length} online`;
  updateComposerHint();
  renderWinTabs();
  if (!$('#tpick').hidden) renderPicker();
}
function setTarget(value) {
  S.target = value || 'all';
  saveWins();
  renderTargets();
}
$('#targetChips').addEventListener('click', (e) => {
  const c = e.target.closest('[data-target]');
  if (!c) return;
  const value = c.dataset.target;
  if (value === 'custom') {
    if (S.target !== 'custom' && !S.custom.size) setCustom(targetIds()); // start from what was targeted
    setTarget('custom');
    return togglePicker();
  }
  if (e.ctrlKey || e.metaKey || e.shiftKey) { // combine folders: Ctrl+click adds / removes a whole folder
    const base = new Set(S.target === 'custom' ? S.custom : targetIds());
    const ids = idsFor(value, S.custom);
    const has = ids.length && ids.every((id) => base.has(id));
    ids.forEach((id) => (has ? base.delete(id) : base.add(id)));
    setCustom(base);
    return setTarget('custom');
  }
  setTarget(value);
});
$('#targetSel').onchange = (e) => setTarget(e.target.value);

/* ---- custom target picker: any servers from any folders ---- */
function setCustom(ids) {
  S.custom = new Set(ids);
  saveWins();
}
function pruneCustom() { // forget servers that were deleted
  const gone = [...S.custom].filter((id) => !S.servers.has(id));
  if (gone.length) setCustom([...S.custom].filter((id) => S.servers.has(id)));
  const sets = S.targetSets.map((t) => ({ ...t, ids: t.ids.filter((id) => S.servers.has(id)) })).filter((t) => t.ids.length);
  if (sets.length !== S.targetSets.length || sets.some((t, i) => t.ids.length !== S.targetSets[i].ids.length)) saveSets(sets);
}
function saveSets(sets) { S.targetSets = sets; store.set('targetSets', sets); }
// A short human label for a set of servers: whole folders by name, then the rest by name or count.
function describeIds(ids) {
  const want = new Set(ids);
  const all = serverList();
  if (want.size && want.size === all.length) return 'All servers';
  const parts = [];
  const left = new Set(want);
  for (const f of folders()) {
    const members = all.filter((s) => folderName(s) === f).map((s) => s.id);
    if (members.length > 1 && members.every((id) => want.has(id))) { parts.push(f); members.forEach((id) => left.delete(id)); }
  }
  const names = [...left].map((id) => S.servers.get(id)?.name).filter(Boolean);
  if (names.length <= 3) parts.push(...names);
  else parts.push(`${names.length} servers`);
  return parts.join(', ') || 'nothing';
}

const picker = { q: '', saving: false };
function togglePicker(open = $('#tpick').hidden) {
  $('#tpick').hidden = !open;
  if (!open) return;
  picker.saving = false;
  renderPicker();
  $('#tpickQ').focus();
}
function pickerShown() {
  const q = picker.q.trim().toLowerCase();
  return serverList()
    .filter((s) => !q || [s.name, s.host, s.username, folderName(s)].some((v) => String(v || '').toLowerCase().includes(q)))
    .sort((a, b) => folderName(a).localeCompare(folderName(b)) || a.name.localeCompare(b.name));
}
function renderPicker() {
  const shown = pickerShown();
  const byFolder = new Map();
  shown.forEach((s) => { const f = folderName(s); if (!byFolder.has(f)) byFolder.set(f, []); byFolder.get(f).push(s); });
  const box = (state) => `<span class="tbox ${state}">${svg(state === 'mixed' ? 'minus' : 'check')}</span>`;
  $('#tpickList').innerHTML = byFolder.size ? [...byFolder].map(([f, items]) => {
    const n = items.filter((s) => S.custom.has(s.id)).length;
    const state = n === 0 ? '' : n === items.length ? 'on' : 'mixed';
    return `<div class="tp-group">
      <button class="tp-folder" data-folder="${esc(f)}">${box(state)}${icon('folder')}<span>${esc(f)}</span><b>${n}/${items.length}</b></button>
      ${items.map((s) => `<button class="tp-srv" data-id="${s.id}">${box(S.custom.has(s.id) ? 'on' : '')}<span class="dot ${status(s)}"></span>
        <span class="tp-name">${esc(s.name)}</span><span class="tp-host">${esc(s.username)}@${esc(s.host)}</span></button>`).join('')}
    </div>`;
  }).join('') : `<div class="tp-empty">No servers match “${esc(picker.q)}”</div>`;

  const sets = S.targetSets.map((t, i) => {
    const same = t.ids.length === S.custom.size && t.ids.every((id) => S.custom.has(id));
    return `<span class="tp-set ${same ? 'on' : ''}" data-set="${i}" title="${esc(describeIds(t.ids))}">${esc(t.name)} <b>${t.ids.length}</b><button data-del="${i}" title="Delete this saved selection">${svg('x')}</button></span>`;
  }).join('');
  const save = picker.saving
    ? `<input id="tpickName" class="tp-name-in" maxlength="40" placeholder="Name, then Enter">`
    : `<button class="tp-save" id="tpickSave" ${S.custom.size ? '' : 'disabled'} title="Save this selection to reuse later">${svg('plus')}Save selection</button>`;
  $('#tpickSets').innerHTML = `<span class="tp-sets-label">Saved</span>${sets || '<span class="tp-none">none yet</span>'}${save}`;
  if (picker.saving) $('#tpickName').focus();

  const ids = [...S.custom];
  const on = ids.filter((id) => S.servers.get(id)?.last_status === 'online').length;
  $('#tpickCount').innerHTML = ids.length
    ? `<b>${ids.length}</b> selected · ${on} online · <span class="tp-desc">${esc(describeIds(ids))}</span>`
    : 'Nothing selected — tick servers or whole folders';
}
function changeCustom(fn) {
  const next = new Set(S.custom);
  fn(next);
  setCustom(next);
  setTarget('custom');
}
$('#tpickQ').addEventListener('input', (e) => { picker.q = e.target.value; renderPicker(); });
$('#tpickQ').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { // Enter ticks everything the filter shows
    e.preventDefault();
    const shown = pickerShown().map((s) => s.id);
    changeCustom((set) => shown.forEach((id) => set.add(id)));
  }
});
$('#tpick').addEventListener('click', (e) => {
  const q = e.target.closest('[data-q]')?.dataset.q;
  const shown = pickerShown();
  if (q) {
    return changeCustom((set) => {
      if (q === 'all') shown.forEach((s) => set.add(s.id));
      if (q === 'none') shown.forEach((s) => set.delete(s.id));
      if (q === 'online') shown.forEach((s) => { if (s.last_status !== 'online') set.delete(s.id); });
      if (q === 'invert') shown.forEach((s) => (set.has(s.id) ? set.delete(s.id) : set.add(s.id)));
    });
  }
  const del = e.target.closest('[data-del]');
  if (del) { saveSets(S.targetSets.filter((_, i) => i !== +del.dataset.del)); return renderPicker(); }
  const setEl = e.target.closest('[data-set]');
  if (setEl) return changeCustom((set) => { set.clear(); S.targetSets[+setEl.dataset.set].ids.forEach((id) => set.add(id)); });
  if (e.target.closest('#tpickSave')) { picker.saving = true; return renderPicker(); }
  const folder = e.target.closest('.tp-folder');
  if (folder) {
    const ids = shown.filter((s) => folderName(s) === folder.dataset.folder).map((s) => s.id);
    const all = ids.every((id) => S.custom.has(id));
    return changeCustom((set) => ids.forEach((id) => (all ? set.delete(id) : set.add(id))));
  }
  const row = e.target.closest('.tp-srv');
  if (row) { const id = +row.dataset.id; return changeCustom((set) => (set.has(id) ? set.delete(id) : set.add(id))); }
  if (e.target.closest('#tpickDone')) togglePicker(false);
});
$('#tpick').addEventListener('keydown', (e) => {
  if (e.target.id === 'tpickName') {
    if (e.key === 'Enter') {
      e.preventDefault();
      const name = e.target.value.trim();
      if (name && S.custom.size) {
        saveSets([...S.targetSets.filter((t) => t.name.toLowerCase() !== name.toLowerCase()), { name, ids: [...S.custom] }]);
        toast(`Saved selection “${name}” (${S.custom.size} servers)`, 'ok');
      }
      picker.saving = false;
      renderPicker();
    }
    if (e.key === 'Escape') { e.stopPropagation(); picker.saving = false; renderPicker(); }
    return;
  }
  if (e.key === 'Escape') { e.stopPropagation(); togglePicker(false); $('#prompt').focus(); }
});
document.addEventListener('mousedown', (e) => {
  if (!$('#tpick').hidden && !e.target.closest('#tpick') && !e.target.closest('.chip-custom')) togglePicker(false);
});

function idsFor(target, custom) {
  const all = serverList();
  if (target === 'selected') return [...S.selected];
  if (target === 'custom') return [...custom];
  if (target.startsWith('folder:')) { const f = target.slice(7); return all.filter((s) => folderName(s) === f).map((s) => s.id); }
  if (target.startsWith('server:')) return [+target.slice(7)];
  return all.map((s) => s.id);
}
function labelFor(target, custom) {
  if (target === 'selected') return `Selected (${S.selected.size})`;
  if (target === 'custom') return `Custom (${custom.size}): ${describeIds([...custom])}`;
  if (target.startsWith('folder:')) return `Folder: ${target.slice(7)}`;
  if (target.startsWith('server:')) return S.servers.get(+target.slice(7))?.name || 'server';
  return 'All servers';
}
const targetIds = () => idsFor(S.target, S.custom);
const targetLabel = () => labelFor(S.target, S.custom);

function setMode(mode) {
  S.mode = mode;
  saveWins();
  busyUI();
  $$('#modeSeg button').forEach((b) => b.classList.toggle('on', b.dataset.mode === mode));
  $('#composer').classList.toggle('direct', mode === 'direct');
  const noAgy = mode === 'ai' && S.meta.agy === null;
  $('#agyMissing').hidden = !noAgy;
  $('#chatEmpty').classList.toggle('dim', noAgy);
  $('#prompt').placeholder = noAgy ? 'AI chat needs the AGY CLI — see above, or switch to Direct mode'
    : mode === 'ai' ? 'Ask in any language — e.g. “sob server er node version check koro”' : 'Shell command to run on every target server — e.g. df -h /';
  updateComposerHint();
}
$$('#modeSeg button').forEach((b) => (b.onclick = () => setMode(b.dataset.mode)));

function updateComposerHint() {
  const n = targetIds().length;
  const w = AW();
  $('#composerHint').textContent = (w && S.wins.size > 1 ? `${winName(w)} · ` : '') + (S.mode === 'ai'
    ? `AI mode · ${n} target server${n === 1 ? '' : 's'} · read-only checks run at once, changes wait for your approval · Enter to send`
    : `Direct mode · runs exactly what you type on ${n} server${n === 1 ? '' : 's'} in parallel · Enter to run`);
}

function autoGrow() {
  const t = $('#prompt');
  t.style.height = 'auto';
  const h = t.value ? t.scrollHeight : 0; // empty: one line, whatever the placeholder's length
  t.style.height = h ? Math.min(200, h) + 'px' : '';
  t.style.overflowY = h > 200 ? 'auto' : 'hidden'; // a scrollbar only when it is really needed
}
$('#prompt').addEventListener('input', autoGrow);
window.addEventListener('resize', () => autoGrow());
// text put in by the app (a draft, a suggestion, "ask again") resizes the box too
const promptValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value');
Object.defineProperty($('#prompt'), 'value', {
  get() { return promptValue.get.call(this); },
  set(v) { promptValue.set.call(this, v); requestAnimationFrame(autoGrow); },
});
// Up / Down recall earlier prompts like a shell; only from the first / last line, so multi-line
// editing keeps working. What was being typed comes back after the newest prompt.
const recall = { list: store.get('promptHistory', []), pos: -1, draft: '' };
function remember(text) {
  if (recall.list[recall.list.length - 1] !== text) recall.list.push(text);
  recall.list = recall.list.slice(-100);
  store.set('promptHistory', recall.list);
  recall.pos = -1;
}
function recallStep(t, dir) {
  const n = recall.list.length;
  if (!n) return false;
  if (recall.pos === -1) {
    if (dir > 0) return false;
    recall.draft = t.value;
    recall.pos = n;
  }
  const pos = Math.min(Math.max(recall.pos + dir, 0), n);
  if (pos === recall.pos) return true;
  recall.pos = pos;
  t.value = pos === n ? recall.draft : recall.list[pos];
  if (pos === n) recall.pos = -1;
  t.setSelectionRange(t.value.length, t.value.length);
  autoGrow();
  return true;
}
$('#prompt').addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
  const t = e.target;
  if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
    const up = e.key === 'ArrowUp';
    const edge = up ? !t.value.slice(0, t.selectionStart).includes('\n') : !t.value.slice(t.selectionEnd).includes('\n');
    if (edge && recallStep(t, up ? -1 : 1)) e.preventDefault();
  }
});
$('#btnSend').onclick = send;
$('#btnStop').onclick = () => api('/api/ai/stop', { win: S.win }).catch((err) => toast(err.message, 'bad'));

/* ================================================================ chat windows */
// Several chats work at the same time: each window has its own AI session on the backend, its own
// conversation, targets and mode — e.g. one upgrades the web servers while another checks the
// databases. Events carry their window id, so windows in the background keep updating.
const MAX_WINS = 6;
let RW = null; // the window being drawn into (an event may belong to a window that is not shown)
const AW = () => S.wins.get(S.win); // the window shown
const winName = (w) => w.title || `Chat ${[...S.wins.keys()].indexOf(w.id) + 1}`;

function makeWin(saved = {}) {
  const log = document.createElement('div');
  log.className = 'win-log';
  log.hidden = true;
  $('#chatInner').appendChild(log);
  const w = {
    id: saved.id || `w${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`,
    title: saved.title || '', chat: saved.chat ?? null, chatFloor: S.lastChat, favorite: !!saved.favorite,
    jobs: new Map(), batches: new Map(), busy: false, state: 'idle', unseen: false,
    target: saved.target || 'all', custom: new Set(saved.custom || []), mode: saved.mode || 'ai',
    draft: '', scroll: 0, log,
  };
  S.wins.set(w.id, w);
  return w;
}
function syncActive() { // the live target / mode belong to the window shown
  const w = AW();
  if (w) Object.assign(w, { target: S.target, custom: S.custom, mode: S.mode });
}
function saveWins() {
  if (!S.wins.size) return;
  syncActive();
  store.set('wins', [...S.wins.values()].map((w) => ({
    id: w.id, title: w.title, chat: w.chat, favorite: !!w.favorite, target: w.target === 'selected' ? 'all' : w.target, custom: [...w.custom], mode: w.mode,
  })));
  store.set('win', S.win);
}
function activateWin(id) {
  const prev = AW();
  if (prev) {
    syncActive();
    prev.draft = $('#prompt').value;
    prev.scroll = $('#chat').scrollTop;
    prev.log.hidden = true;
  }
  S.win = id;
  const w = AW();
  w.unseen = false;
  w.log.hidden = false;
  S.target = w.target;
  S.custom = w.custom;
  $('#prompt').value = w.draft;
  autoGrow();
  if (!$('#tpick').hidden) togglePicker(false);
  setMode(w.mode);
  renderTargets();
  updateEmpty();
  const c = $('#chat');
  c.style.scrollBehavior = 'auto';
  c.scrollTop = prev ? w.scroll : c.scrollHeight;
  c.style.scrollBehavior = '';
  saveWins();
}
async function newWin(focus = true) {
  if (S.wins.size >= MAX_WINS) { toast(`At most ${MAX_WINS} chat windows — close one first`, 'bad'); return null; }
  const w = makeWin();
  try {
    await api('/api/ai/windows', { win: w.id }); // starts its AI session now, so the first answer is quick
  } catch (err) {
    w.log.remove();
    S.wins.delete(w.id);
    toast(err.message, 'bad');
    return null;
  }
  activateWin(w.id);
  if (focus) $('#prompt').focus();
  return w;
}
async function closeWin(id) {
  const w = S.wins.get(id);
  if (!w) return;
  if (w.busy && !(await confirmBox(`Close ${winName(w)}?`, 'The AI is still working in this window. Closing it stops that work; the chat stays in History.'))) return;
  try { await api(`/api/ai/windows/${encodeURIComponent(id)}`, undefined, 'DELETE'); } catch (err) { return toast(err.message, 'bad'); }
  const order = [...S.wins.keys()];
  const i = order.indexOf(id);
  w.log.remove();
  S.wins.delete(id);
  if (!S.wins.size) makeWin({ id: 'main' });
  if (S.win === id) { S.win = null; activateWin([...S.wins.keys()][Math.max(0, i - 1)]); } else { saveWins(); renderWinTabs(); }
}

function renderWinTabs() {
  const el = $('#winTabs');
  if (!el || !S.wins.size) return;
  syncActive();
  const tabs = [...S.wins.values()].map((w, i) => {
    const st = w.busy ? (w.state === 'waiting' ? 'waiting' : 'busy') : w.unseen ? (w.state === 'error' ? 'error' : 'unseen') : '';
    const ind = st === 'busy' ? '<span class="wspin"></span>' : `<span class="wdot ${st}"></span>`;
    const n = idsFor(w.target, w.custom).length;
    const why = { busy: 'working…', waiting: 'waiting for your approval', unseen: 'new results', error: 'failed' }[st] || 'idle';
    return `<div class="wtab ${w.id === S.win ? 'on' : ''} ${st}" data-win="${esc(w.id)}" title="${esc(winName(w))} → ${esc(labelFor(w.target, w.custom))} · ${why}${i < 9 ? ` · Alt+${i + 1}` : ''}">
      ${ind}${w.favorite ? `<span class="wt-star">${svg('star')}</span>` : ''}<span class="wt-name">${esc(winName(w))}</span><span class="wt-n" title="target servers">${n}</span>
      <button class="wt-x" data-close title="Close this window">${svg('x')}</button></div>`;
  }).join('');
  const hint = S.wins.size === 1 ? '<span class="wtabs-hint">Open another window to work on other servers at the same time</span>' : '';
  el.innerHTML = tabs + `<button class="wtab-add" data-add title="New chat window — works in parallel with the others (Alt+N)" ${S.wins.size >= MAX_WINS ? 'disabled' : ''}>${svg('plus')}<span>New window</span></button>${hint}`;
}
$('#winTabs').addEventListener('click', (e) => {
  if (e.target.closest('[data-add]')) return newWin();
  const tab = e.target.closest('[data-win]');
  if (!tab) return;
  if (e.target.closest('[data-close]')) return closeWin(tab.dataset.win);
  if (tab.dataset.win !== S.win) activateWin(tab.dataset.win);
});
$('#winTabs').addEventListener('auxclick', (e) => { // middle click closes, like browser tabs
  const tab = e.target.closest('[data-win]');
  if (tab && e.button === 1) closeWin(tab.dataset.win);
});
document.addEventListener('keydown', (e) => {
  if (S.view !== 'ai' || !e.altKey || e.ctrlKey || e.metaKey) return;
  if (/^[1-9]$/.test(e.key)) {
    const id = [...S.wins.keys()][+e.key - 1];
    if (id) { e.preventDefault(); activateWin(id); }
  } else if (e.key.toLowerCase() === 'n') { e.preventDefault(); newWin(); }
});

function updateEmpty() { $('#chatEmpty').hidden = !!AW()?.log.childElementCount; }

function clearChat(w = AW()) {
  w.log.replaceChildren();
  w.jobs.clear();
  w.batches.clear();
  w.chatFloor = Math.max(S.lastChat, w.chat ?? 0);
  Object.assign(w, { chat: null, title: '', state: 'idle', unseen: false, favorite: false });
  updateEmpty();
  saveWins();
  renderWinTabs();
}

/* ---------------------------------------------------------------- chat history */
async function openChat(id, w = AW(), restoreTargets = true) {
  const chat = await api(`/api/chats/${id}/open`, { win: w.id });
  clearChat(w);
  w.chat = chat.id;
  w.title = chat.title || '';
  w.favorite = !!chat.favorite;
  S.lastChat = Math.max(S.lastChat, chat.id);
  RW = w;
  for (const ev of chat.events) chatEvent(ev, true);
  // steps that never reported back (stopped, app closed) stop spinning
  const states = [...w.batches.values(), ...[...w.jobs.values()].flatMap((j) => [...j.rounds.values()])];
  for (const st of states) {
    for (const r of st.rows.values()) {
      if (r.got >= r.expected) continue;
      $('.st', r.row).className = 'st warn';
      $('.st', r.row).innerHTML = svg('alert');
      if (!r.got) $('.rpreview', r.row).textContent = 'not run';
    }
  }
  updateEmpty();
  if (restoreTargets) {
    const last = [...chat.events].reverse().find((ev) => ev.type === 'chat.user');
    const sc = scopeOf(last);
    if (applyScope(w, sc)) {
      toast(`Servers of this chat selected: ${labelFor(w.target, w.custom)}${sc.lost ? ` (${sc.lost} no longer exist)` : ''}`, 'ok');
    } else if (last) toast('The servers this chat used no longer exist — pick targets above', 'bad');
  }
  saveWins();
  renderWinTabs();
  if (w === AW()) forceScroll();
}

/* ---- a chat remembers its servers: reopening it targets them again */
function scopeNow(ids) {
  return { spec: S.target === 'selected' ? 'custom' : S.target, ids };
}
function scopeOf(ev) {
  if (!ev) return null;
  const sc = ev.scope;
  if (sc) {
    const spec = sc.spec || '';
    if (spec === 'all') return { target: 'all' };
    if (spec.startsWith('folder:') && folders().includes(spec.slice(7))) return { target: spec };
    if (spec.startsWith('server:') && S.servers.has(+spec.slice(7))) return { target: spec };
    const want = sc.ids || [];
    const ids = want.filter((id) => S.servers.has(id));
    return ids.length ? { target: 'custom', custom: ids, lost: want.length - ids.length } : null;
  }
  const label = ev.target || ''; // chats from before this kept only the label
  if (label === 'All servers') return { target: 'all' };
  if (label.startsWith('Folder: ') && folders().includes(label.slice(8))) return { target: `folder:${label.slice(8)}` };
  const one = serverList().find((x) => x.name === label);
  return one ? { target: `server:${one.id}` } : null;
}
function applyScope(w, sc) {
  if (!sc) return false;
  w.target = sc.target;
  if (sc.custom) w.custom = new Set(sc.custom);
  if (w === AW()) {
    S.target = w.target;
    S.custom = w.custom;
    renderTargets();
  }
  saveWins();
  return true;
}

// Open a chat from History / Favorites: a chat already open just shows its window; a window that is
// working keeps its chat and the old one opens in a new window next to it.
async function openFromList(id, { newWindow = false, prefill = null } = {}) {
  const holder = [...S.wins.values()].find((x) => x.chat === id);
  if (holder) activateWin(holder.id);
  else {
    const w = newWindow || AW().busy ? await newWin(false) : AW();
    if (!w) return;
    await openChat(id, w);
    if (w !== AW()) activateWin(w.id);
  }
  if (prefill != null) {
    $('#prompt').value = prefill;
    autoGrow();
    const t = $('#prompt');
    t.focus();
    t.setSelectionRange(t.value.length, t.value.length);
  }
}
async function setFavorite(id, favorite) {
  await api(`/api/chats/${id}`, { favorite }, 'PATCH');
  S.wins.forEach((x) => { if (x.chat === id) x.favorite = favorite; });
  renderWinTabs();
  toast(favorite ? 'Added to favorites — kept forever, reopens with the same servers' : 'Removed from favorites', 'ok');
}
// Inline rename: the title turns into an input; Enter saves, Esc cancels.
function renameInline(titleEl, chat, done) {
  const input = document.createElement('input');
  input.className = 'rename-in';
  input.value = chat.title;
  input.maxLength = 120;
  titleEl.replaceWith(input);
  input.focus();
  input.select();
  let finished = false;
  const finish = async (save) => {
    if (finished) return;
    finished = true;
    const title = input.value.trim();
    if (save && title && title !== chat.title) {
      try {
        await api(`/api/chats/${chat.id}`, { title }, 'PATCH');
        S.wins.forEach((x) => { if (x.chat === chat.id) x.title = title; });
        renderWinTabs();
        saveWins();
      } catch (err) { toast(err.message, 'bad'); }
    }
    done();
  };
  input.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') finish(true);
    if (e.key === 'Escape') finish(false);
  });
  input.addEventListener('blur', () => finish(true));
  input.addEventListener('click', (e) => e.stopPropagation());
}

function ago(ts) {
  const min = Math.round((Date.now() - new Date(ts)) / 60000);
  if (min < 1) return 'just now';
  if (min < 60) return `${min} min ago`;
  if (min < 1440) return `${Math.round(min / 60)} h ago`;
  return `${Math.round(min / 1440)} d ago`;
}

function chatRow(c, days, winOf) {
  const w = winOf.get(c.id);
  const here = w && w.id === S.win;
  const left = Math.max(0, Math.ceil(days - (Date.now() - new Date(c.updated_at)) / 86400000));
  const where = c.last?.target ? ` · → ${esc(c.last.target)}` : '';
  return `<div class="hrow ${here ? 'on' : ''} ${c.favorite ? 'fav' : ''}" data-id="${c.id}">
    <button class="hstar ${c.favorite ? 'on' : ''}" data-star title="${c.favorite ? 'Remove from favorites' : 'Add to favorites (kept forever)'}">${svg('star')}</button>
    <div class="hmain"><div class="htitle">${esc(c.title)}</div>
    <div class="hmeta">${esc(ago(c.updated_at))} · ${c.messages} message${c.messages === 1 ? '' : 's'}${where} · ${c.favorite ? 'kept forever' : `expires in ${left} d`}${w ? ` · <b>open in ${here ? 'this window' : esc(winName(w))}</b>` : ''}</div></div>
    <button class="btn btn-ghost btn-icon btn-sm" data-rename title="Rename">${icon('edit')}</button>
    ${w ? '' : `<button class="btn btn-ghost btn-icon btn-sm" data-new title="Open in a new chat window">${icon('plus')}</button>`}
    <button class="btn btn-ghost btn-icon btn-sm" data-del title="Delete this chat">${icon('trash')}</button></div>`;
}

const hist = { q: '' };
async function showHistory() {
  const { chats, days } = await api('/api/chats');
  const winOf = new Map([...S.wins.values()].filter((w) => w.chat != null).map((w) => [w.chat, w]));
  $('#historySub').textContent = `Chats are kept for ${days} days after their last message — favorites (★) forever`;
  const body = $('#historyBody');
  const draw = () => {
    const q = hist.q.trim().toLowerCase();
    const shown = chats.filter((c) => !q || [c.title, c.last?.text, c.last?.target].some((v) => String(v || '').toLowerCase().includes(q)));
    const favs = shown.filter((c) => c.favorite);
    const rest = shown.filter((c) => !c.favorite);
    const section = (title, list) => (list.length ? `<div class="hsec">${title}</div>${list.map((c) => chatRow(c, days, winOf)).join('')}` : '');
    body.innerHTML = `<label class="search hsearch">${icon('search')}<input id="histQ" placeholder="Search chats…" value="${esc(hist.q)}" autocomplete="off"></label>`
      + (shown.length ? section('★ Favorites', favs) + section(favs.length ? 'Recent' : '', rest)
        : `<p class="muted">${chats.length ? 'No chat matches.' : 'No chats yet. Your conversations show up here.'}</p>`);
    const qi = $('#histQ');
    qi.oninput = () => { hist.q = qi.value; draw(); const again = $('#histQ'); again.focus(); again.setSelectionRange(again.value.length, again.value.length); };
  };
  draw();
  body.onclick = async (e) => {
    const row = e.target.closest('.hrow');
    if (!row) return;
    const id = Number(row.dataset.id);
    const chat = chats.find((c) => c.id === id);
    try {
      if (e.target.closest('[data-star]')) {
        chat.favorite = !chat.favorite;
        await setFavorite(id, chat.favorite);
        return draw();
      }
      if (e.target.closest('[data-rename]')) return renameInline($('.htitle', row), chat, () => showHistory());
      if (e.target.closest('[data-del]')) {
        if (!(await confirmBox('Delete chat?', `${chat.favorite ? 'This is a favorite. ' : ''}This chat and its results are removed for good. The audit log keeps its commands.`))) return;
        await api(`/api/chats/${id}`, undefined, 'DELETE');
        const w = winOf.get(id);
        if (w) clearChat(w);
        return showHistory();
      }
      if (e.target.closest('input')) return;
      closeModals();
      await openFromList(id, { newWindow: !!e.target.closest('[data-new]') });
    } catch (err) { toast(err.message, 'bad'); }
  };
  $('#historyModal').hidden = false;
}

/* ---- Favorites: one click back into a saved chat, with its servers selected */
async function showFavs() {
  const pop = $('#favPop');
  const { chats } = await api('/api/chats');
  const favs = chats.filter((c) => c.favorite);
  const w = AW();
  const cur = w.chat != null ? chats.find((c) => c.id === w.chat) : null;
  const top = cur
    ? `<button class="fav-cur ${cur.favorite ? 'on' : ''}" data-cur>${svg('star')}<span>${cur.favorite ? 'This chat is a favorite — remove' : 'Add this chat to favorites'}</span></button>`
    : '<div class="fav-note">Send a message, then star the chat to keep it here.</div>';
  pop.innerHTML = `<div class="fav-head"><b>Favorites</b><span class="muted">kept forever · reopen with the same servers</span></div>${top}
    <div class="fav-list">${favs.length ? favs.map((c) => `<div class="fav-row" data-id="${c.id}" title="Open — its servers are selected again">
      <div class="fav-main"><div class="fav-title">${esc(c.title)}</div>
        <div class="fav-meta">${c.last?.target ? `→ ${esc(c.last.target)} · ` : ''}${esc(ago(c.updated_at))}</div></div>
      ${c.last?.text ? `<button class="btn btn-ghost btn-sm" data-again title="Open it and put its last request in the box: ${esc(c.last.text.slice(0, 120))}">${icon('reconnect')}Ask again</button>` : ''}
      <button class="btn btn-ghost btn-icon btn-sm" data-unstar title="Remove from favorites">${svg('star')}</button></div>`).join('')
    : '<div class="fav-empty">No favorites yet. Star a chat here or in History.</div>'}</div>`;
  pop.hidden = false;
  pop.onclick = async (e) => {
    try {
      if (e.target.closest('[data-cur]')) { await setFavorite(cur.id, !cur.favorite); return showFavs(); }
      const row = e.target.closest('.fav-row');
      if (!row) return;
      const id = Number(row.dataset.id);
      const c = favs.find((x) => x.id === id);
      if (e.target.closest('[data-unstar]')) { await setFavorite(id, false); return showFavs(); }
      pop.hidden = true;
      await openFromList(id, { prefill: e.target.closest('[data-again]') ? c.last.text : null });
    } catch (err) { toast(err.message, 'bad'); }
  };
}
$('#btnFavs').onclick = () => ($('#favPop').hidden ? showFavs().catch((err) => toast(err.message, 'bad')) : ($('#favPop').hidden = true));
document.addEventListener('mousedown', (e) => {
  if (!$('#favPop').hidden && !e.target.closest('#favPop') && !e.target.closest('#btnFavs')) $('#favPop').hidden = true;
});
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !$('#favPop').hidden) $('#favPop').hidden = true; });

$('#btnHistory').onclick = () => showHistory().catch((err) => toast(err.message, 'bad'));
$('#btnNewChat').onclick = async () => {
  const w = AW();
  if (w.busy) return toast('The AI is still working in this window — stop it, or open a new window', 'bad');
  try {
    await api('/api/ai/reset', { win: w.id });
    clearChat(w);
    toast('Fresh AI conversation started', 'ok');
  } catch (err) { toast(err.message, 'bad'); }
};

function busyUI() {
  const w = AW();
  if (!w) return;
  $('#btnStop').hidden = !w.busy;
  $('#btnSend').disabled = w.busy && S.mode === 'ai';
}
function setBusy(busy) {
  RW.busy = busy;
  if (!busy && RW !== AW()) RW.unseen = true;
  busyUI();
  renderWinTabs();
}

function scrollChat() {
  if (RW && RW !== AW()) return;
  const c = $('#chat');
  if (c.scrollHeight - c.scrollTop - c.clientHeight < 260) c.scrollTop = c.scrollHeight;
}
function forceScroll() { if (RW && RW !== AW()) return; const c = $('#chat'); c.scrollTop = c.scrollHeight; }

function addBlock(kind) {
  const block = document.createElement('div');
  block.className = 'ai-block';
  block.innerHTML = `<div class="ai-avatar ${kind === 'direct' ? 'direct' : ''}">${icon(kind === 'direct' ? 'terminal' : 'sparkles')}</div><div class="ai-col"></div>`;
  RW.log.appendChild(block);
  updateEmpty();
  return $('.ai-col', block);
}

async function send() {
  const text = $('#prompt').value.trim();
  if (!text) return;
  const w = AW();
  const ids = targetIds();
  if (!ids.length) return toast('No target servers selected', 'bad');
  const target = targetLabel();
  const user = userBubble({ mode: S.mode, text, target });
  if (S.mode === 'ai') {
    if (S.meta.agy === null) { $('#agyMissing').hidden = false; return toast('AI chat needs the AGY CLI — install it first', 'bad'); }
    if (w.busy) return toast('The AI is still working in this window — wait, stop it, or open a new window (Alt+N)', 'bad');
    RW = w;
    w.log.appendChild(user);
    const col = addBlock('ai');
    w.state = 'thinking';
    if (!w.title) w.title = text.slice(0, 48);
    setBusy(true);
    try {
      const { job } = await api('/api/ai/ask', { win: w.id, prompt: text, ids: S.target === 'all' ? [] : ids, target, scope: scopeNow(ids) });
      RW = w;
      if (!w.jobs.has(job)) w.jobs.set(job, { col, rounds: new Map(), statusEl: null, servers: ids.length });
      else col.parentElement.remove(); // its first event arrived before this reply and drew the job already
      setStatus(job, 'thinking');
    } catch (err) {
      RW = w;
      setBusy(false);
      toast(err.message, 'bad');
      return;
    }
  } else {
    w.log.appendChild(user);
    updateEmpty();
    if (!w.title) w.title = text.slice(0, 48);
    renderWinTabs();
    api('/api/exec', { win: w.id, command: text, ids, target, scope: scopeNow(ids) }).catch((err) => toast(err.message, 'bad'));
  }
  remember(text);
  if (w === AW()) { $('#prompt').value = ''; autoGrow(); }
  w.draft = '';
  saveWins();
  RW = w;
  forceScroll();
}

function userBubble(ev) {
  const user = document.createElement('div');
  user.className = `msg-user ${ev.mode === 'direct' ? 'direct' : ''}`;
  user.innerHTML = `${esc(ev.text)}${ev.target ? `<span class="target-note">→ ${esc(ev.target)}</span>` : ''}`;
  return user;
}

// A job seen without a block of its own (reopened chat, or a page reload mid-job) gets one here.
function ensureJob(id) {
  if (!RW.jobs.has(id)) RW.jobs.set(id, { col: addBlock('ai'), rounds: new Map(), statusEl: null });
  return RW.jobs.get(id);
}

function setStatus(jobId, state, text, model, why) {
  const working = ['thinking', 'running', 'waiting'].includes(state);
  if (!working && RW.busy) setBusy(false); // also when the job is not drawn (e.g. a page reload mid-job)
  const job = RW.jobs.get(jobId);
  if (!job) return;
  if (!job.statusEl) { job.statusEl = document.createElement('div'); job.statusEl.className = 'thinking'; }
  const who = model || brainName(); // the model actually working (under Auto it changes per request)
  const labels = {
    thinking: text || (job.rounds.size ? `${who} is analysing the results…` : `${who} is planning…${why ? ` (${why})` : ''}`),
    running: text || 'Running on servers…',
    waiting: 'Waiting for your approval',
  };
  if (labels[state]) {
    job.statusEl.innerHTML = state === 'waiting' ? `${icon('shield')}<span>${labels[state]}</span>` : `<span class="dots"><i></i><i></i><i></i></span><span>${esc(labels[state])}</span>`;
    job.col.appendChild(job.statusEl);
  } else {
    job.statusEl.remove();
  }
  scrollChat();
}

function groupTasks(tasks) {
  const groups = new Map();
  for (const t of tasks) {
    const key = JSON.stringify(t.commands);
    if (!groups.has(key)) groups.set(key, { commands: t.commands, names: [] });
    groups.get(key).names.push(t.name);
  }
  return [...groups.values()];
}

function resultsPanel(title, entries) {
  const panel = document.createElement('div');
  panel.className = 'panel results';
  panel.innerHTML = `<div class="panel-head rhead"><span class="title">${esc(title)}</span><span class="muted rcount"></span><div class="progress"><i style="width:0%"></i></div><div class="grow"></div>
    <button class="btn btn-ghost btn-sm" data-act="expand">Expand all</button></div><div class="rrows"></div>`;
  const rows = new Map();
  const rowsEl = $('.rrows', panel);
  for (const e of entries) {
    const row = document.createElement('div');
    row.className = 'rrow';
    row.innerHTML = `<div class="rline"><span class="st wait"></span><span class="rname">${esc(e.name)}</span><span class="rpreview">${e.queued ? 'queued' : 'running…'}</span><span class="rmeta"></span>${icon('chev').replace('class="i"', 'class="i chev"')}</div><div class="rout"></div>`;
    $('.rline', row).onclick = () => row.classList.toggle('open');
    rowsEl.appendChild(row);
    rows.set(e.server_id, { row, expected: e.count || 1, got: 0, failed: false });
  }
  $('[data-act=expand]', panel).onclick = (ev) => {
    const open = ev.target.textContent === 'Expand all';
    $$('.rrow', panel).forEach((r) => r.classList.toggle('open', open));
    ev.target.textContent = open ? 'Collapse' : 'Expand all';
  };
  const state = { panel, rows, total: entries.reduce((a, e) => a + (e.count || 1), 0), done: 0 };
  updateProgress(state);
  return state;
}

function updateProgress(state) {
  $('.rcount', state.panel).textContent = `${state.done}/${state.total}`;
  $('.progress i', state.panel).style.width = `${state.total ? (100 * state.done) / state.total : 0}%`;
}

function applyResult(state, ev) {
  const r = state.rows.get(ev.server_id);
  if (!r) return;
  r.got += 1;
  state.done += 1;
  const ok = !ev.error && ev.exit === 0;
  if (!ok) r.failed = true;
  const st = $('.st', r.row);
  if (r.got >= r.expected || ev.error) {
    st.className = `st ${r.failed ? (ev.exit == null ? 'warn' : 'fail') : 'ok'}`;
    st.innerHTML = svg(r.failed ? (ev.exit == null ? 'alert' : 'x') : 'check');
  }
  const firstLine = (ev.error || ev.output || '(no output)').split('\n').find((l) => l.trim()) || '(no output)';
  const exit = ev.exit == null ? '—' : 'exit ' + ev.exit;
  r.time = (r.time || 0) + (ev.duration || 0);
  $('.rpreview', r.row).textContent = firstLine;
  $('.rmeta', r.row).textContent = r.expected > 1 ? `${r.got}/${r.expected} cmds · ${r.time.toFixed(2)}s` : `${exit} · ${ev.duration}s`;
  // one block per command, so multi-step runs show each command's own status, exit code and output
  const cls = ok ? 'ok' : ev.exit == null ? 'warn' : 'fail';
  $('.rout', r.row).insertAdjacentHTML('beforeend', `<div class="rexec ${cls}"><div class="rexec-head"><span class="rexec-n">${r.got}</span><code class="rexec-cmd">$ ${esc(ev.command)}</code><span class="rexec-meta">${exit} · ${ev.duration}s</span></div><pre>${esc((ev.error ? `[${ev.error}]\n` : '') + (ev.output || '(no output)'))}</pre></div>`);
  if (!ok || state.rows.size === 1) r.row.classList.add('open');
  updateProgress(state);
  scrollChat();
}

function decisionText(approve, mode) {
  return approve ? (mode === 'parallel' ? 'Approved — running on all servers.' : 'Approved — canary server first, then the rest.') : 'Cancelled — nothing was run.';
}
function markDecision(bar, approve, mode) {
  bar.classList.add('done');
  bar.innerHTML = `<div class="warn-text">${icon(approve ? 'check' : 'x')}<span>${decisionText(approve, mode)}</span></div>`;
}

function renderPlan(ev, replay) {
  const job = ensureJob(ev.job);
  const panel = document.createElement('div');
  panel.className = 'panel';
  const groups = groupTasks(ev.tasks);
  const remote = ev.tasks.filter((t) => t.server_id !== 0).length; // server_id 0 = this PC
  const local = remote < ev.tasks.length;
  const where = [local && 'this PC', remote && `${remote} server${remote === 1 ? '' : 's'}`].filter(Boolean).join(' and ');
  panel.innerHTML = `<div class="panel-head"><span class="title">Plan · step ${ev.round}</span>
      <span class="badge ${ev.risk}">${icon(ev.risk === 'change' ? 'alert' : 'shield')}${ev.risk === 'change' ? 'Makes changes' : 'Read-only'}</span>
      <div class="grow"></div><span class="muted">${esc(where)}</span></div>
    <div class="panel-body">
      ${ev.say ? `<div class="say">${esc(ev.say)}</div>` : ''}
      ${groups.map((g) => `<div class="cmd-group"><div class="cmd-targets">${g.names.length === ev.tasks.length && groups.length === 1 ? '<span class="tchip">all targets</span>' : g.names.map((n) => `<span class="tchip">${esc(n)}</span>`).join('')}</div>${g.commands.map((c) => `<pre class="code">${esc(c)}</pre>`).join('')}</div>`).join('')}
    </div>`;
  if (ev.needs_approval) {
    const bar = document.createElement('div');
    bar.className = 'approve';
    const canary = remote > 1;
    bar.innerHTML = `<div class="warn-text">${icon('alert')}<span>These steps can change ${esc(where)}. Review before running.</span></div>
      <button class="btn btn-ghost btn-sm" data-a="no">Cancel</button>
      ${canary ? '<button class="btn btn-sm" data-a="all">Run on all at once</button>' : ''}
      <button class="btn btn-warn btn-sm" data-a="${canary ? 'canary' : 'all'}">${icon('check')}${canary ? 'Approve · canary first' : 'Approve & run'}</button>`;
    bar.live = bar.innerHTML;
    const win = RW.id;
    bar.addEventListener('click', async (e) => {
      const a = e.target.closest('[data-a]')?.dataset.a;
      if (!a || bar.classList.contains('done')) return;
      try {
        await api('/api/ai/approve', { win, approve: a !== 'no', mode: a === 'all' ? 'parallel' : 'canary' });
        markDecision(bar, a !== 'no', a === 'all' ? 'parallel' : 'canary');
      } catch (err) { toast(err.message, 'bad'); }
    });
    // a reopened chat: the request is over; an ai.decision event that follows fills in the answer
    if (replay) {
      bar.classList.add('done');
      bar.innerHTML = `<div class="warn-text">${icon('alert')}<span>Approval was never given — nothing was run.</span></div>`;
    }
    panel.appendChild(bar);
  }
  job.col.appendChild(panel);
  const counts = new Map(ev.tasks.map((t) => [t.server_id, t.commands.length]));
  const state = resultsPanel(`Results · step ${ev.round}`, ev.tasks.map((t) => ({ server_id: t.server_id, name: t.name, count: counts.get(t.server_id), queued: ev.needs_approval })));
  state.approveBar = $('.approve', panel);
  job.rounds.set(ev.round, state);
  job.col.appendChild(state.panel);
  if (job.statusEl) job.col.appendChild(job.statusEl);
  scrollChat();
}

/* ---- markdown (small, safe: everything is escaped first) */
function inline(s) {
  return esc(s)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
    .replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<i>$2</i>');
}
function mdTable(lines) {
  const cells = (l) => l.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim());
  const head = cells(lines[0]);
  const body = lines.slice(2).map(cells);
  return tableHtml(head, body, true);
}
function cellClass(v) {
  const t = String(v).toLowerCase();
  if (/(not installed|not_installed|missing|unreachable|failed|error|offline|\bnai\b|not found|none)/.test(t)) return 'neg';
  if (/^(ok|yes|updated|installed|✓|success|online)\b/.test(t)) return 'pos';
  return '';
}
function tableHtml(head, rows, inlineMd) {
  const f = inlineMd ? inline : esc;
  return `<div class="tbl-wrap"><table class="tbl"><thead><tr>${head.map((h) => `<th>${f(h)}</th>`).join('')}</tr></thead><tbody>${rows.map((r) => `<tr>${r.map((c) => `<td class="${cellClass(c)}">${f(c ?? '')}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`;
}
function md(src) {
  const out = [];
  const lines = String(src || '').replace(/\r/g, '').split('\n');
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (/^```/.test(line)) {
      const buf = [];
      i++;
      while (i < lines.length && !/^```/.test(lines[i])) buf.push(lines[i++]);
      i++;
      out.push(`<pre class="code">${esc(buf.join('\n'))}</pre>`);
    } else if (/^\s*\|.*\|\s*$/.test(line) && /^\s*\|?[\s:-]+\|/.test(lines[i + 1] || '')) {
      const buf = [];
      while (i < lines.length && /^\s*\|/.test(lines[i])) buf.push(lines[i++]);
      out.push(mdTable(buf));
    } else if (/^\s*([-*•]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\./.test(line);
      const buf = [];
      while (i < lines.length && /^\s*([-*•]|\d+\.)\s+/.test(lines[i])) buf.push(lines[i++].replace(/^\s*([-*•]|\d+\.)\s+/, ''));
      out.push(`<${ordered ? 'ol' : 'ul'}>${buf.map((b) => `<li>${inline(b)}</li>`).join('')}</${ordered ? 'ol' : 'ul'}>`);
    } else if (/^#{1,6}\s/.test(line)) {
      out.push(`<h4>${inline(line.replace(/^#+\s*/, ''))}</h4>`);
      i++;
    } else if (!line.trim()) {
      i++;
    } else {
      const buf = [];
      while (i < lines.length && lines[i].trim() && !/^(```|#{1,6}\s|\s*([-*•]|\d+\.)\s|\s*\|)/.test(lines[i])) buf.push(lines[i++]);
      if (!buf.length) buf.push(lines[i++]);
      out.push(`<p>${buf.map(inline).join('<br>')}</p>`);
    }
  }
  return out.join('');
}

function renderAnswer(ev) {
  const job = ensureJob(ev.job);
  const el = document.createElement('div');
  const cancelled = /^Change cancelled/.test(ev.summary || '');
  el.className = `answer ${cancelled ? 'cancel' : ''}`;
  const t = ev.table;
  const table = t && Array.isArray(t.columns) && Array.isArray(t.rows) && t.rows.length ? tableHtml(t.columns, t.rows) : '';
  const say = ev.say && ev.say !== ev.summary && ev.say !== 'ready' ? `<p>${inline(ev.say)}</p>` : '';
  el.innerHTML = `<div class="answer-tools"><button class="btn btn-ghost btn-sm btn-icon" title="Copy">${icon('copy')}</button></div><div class="md">${say}${md(ev.summary)}</div>${table}`;
  $('.answer-tools button', el).onclick = () => {
    const rows = t?.rows ? '\n\n' + [t.columns, ...t.rows].map((r) => r.join('\t')).join('\n') : '';
    navigator.clipboard.writeText((ev.summary || '') + rows).then(() => toast('Copied', 'ok'));
  };
  job.col.appendChild(el);
  if (job.statusEl) job.col.appendChild(job.statusEl);
  scrollChat();
}

/* ================================================================ events */
function connectEvents() {
  const ws = new WebSocket(`ws://${location.host}/ws/events?t=${encodeURIComponent(TOKEN)}`);
  ws.onmessage = (m) => onEvent(JSON.parse(m.data));
  ws.onopen = () => loadServers().catch(() => {}); // re-sync anything missed while disconnected
  ws.onclose = () => setTimeout(connectEvents, 1500);
}

function onEvent(ev) {
  switch (ev.type) {
    case 'probe': {
      const s = S.servers.get(ev.server_id);
      if (!s) return;
      if (ev.status === 'checking') S.probing.add(s.id);
      else {
        S.probing.delete(s.id);
        s.last_status = ev.status;
        if (ev.facts) { s.facts = ev.facts; S.probeErr.delete(s.id); }
        if (ev.error) S.probeErr.set(s.id, ev.error);
      }
      return render();
    }
    default:
  }
  if (ev.chat == null) return;
  S.lastChat = Math.max(S.lastChat, ev.chat);
  const w = S.wins.get(ev.win || 'main');
  if (!w) return; // a window closed here
  // the first message of a new chat just created it (chat ids only grow, so a late event of the
  // chat that was just closed cannot claim the new one)
  if (w.chat == null && ev.chat > w.chatFloor) { w.chat = ev.chat; saveWins(); }
  if (ev.chat !== w.chat) return; // another chat (e.g. one still running when you switched away)
  RW = w;
  const shown = w === AW();
  if (ev.type === 'ai.status') {
    const was = w.state;
    w.state = ev.state;
    if (ev.state === 'waiting' && was !== 'waiting' && !shown) toast(`${winName(w)} is waiting for your approval`, 'info');
    if (ev.state === 'error' && !shown) w.unseen = true;
    setStatus(ev.job, ev.state, ev.text, ev.model, ev.why);
    return renderWinTabs();
  }
  if (!shown && ['ai.answer', 'ai.error', 'exec.result'].includes(ev.type)) w.unseen = true;
  chatEvent(ev, false);
  if (!shown) renderWinTabs();
}

// Draws one chat event; used live and to replay a reopened chat from its history.
function chatEvent(ev, replay) {
  switch (ev.type) {
    case 'chat.user':
      RW.log.appendChild(userBubble(ev));
      return updateEmpty();
    case 'ai.plan': return renderPlan(ev, replay);
    case 'ai.decision': {
      const bar = RW.jobs.get(ev.job)?.rounds.get(ev.round)?.approveBar;
      if (bar) markDecision(bar, ev.approve, ev.mode);
      return;
    }
    case 'ai.result': {
      const job = RW.jobs.get(ev.job);
      const state = job?.rounds.get(ev.round);
      if (state) applyResult(state, ev);
      return;
    }
    case 'ai.answer': return renderAnswer(ev);
    case 'ai.error': {
      const job = ensureJob(ev.job);
      const el = document.createElement('div');
      el.className = 'error-box';
      el.textContent = /not signed in/i.test(ev.text) ? ev.text.replace(/^AIAuthError:\s*/, '') : ev.text;
      job.col.appendChild(el);
      return scrollChat();
    }
    case 'exec.start': {
      const col = addBlock('direct');
      const state = resultsPanel(`$ ${ev.command}`, ev.servers.map((s) => ({ ...s, count: 1 })));
      col.appendChild(state.panel);
      RW.batches.set(ev.batch, state);
      return forceScroll();
    }
    case 'exec.result': {
      const state = RW.batches.get(ev.batch);
      if (state) applyResult(state, ev);
      return;
    }
    default:
  }
}

/* ================================================================ terminals */
const TERM_DARK = {
  background: '#0b1019', foreground: '#dbe2ef', cursor: '#22d3ee', cursorAccent: '#0b1019',
  selectionBackground: 'rgba(99,102,241,0.45)',
  black: '#1b2130', red: '#f87171', green: '#34d399', yellow: '#fbbf24', blue: '#60a5fa', magenta: '#c084fc', cyan: '#22d3ee', white: '#dbe2ef',
  brightBlack: '#5b6478', brightRed: '#fca5a5', brightGreen: '#6ee7b7', brightYellow: '#fde68a', brightBlue: '#93c5fd', brightMagenta: '#d8b4fe', brightCyan: '#67e8f9', brightWhite: '#ffffff',
};
// light terminal: ANSI colours dark enough to read on white (yellow, cyan and "white" text too)
const TERM_LIGHT = {
  background: '#ffffff', foreground: '#1e293b', cursor: '#0891b2', cursorAccent: '#ffffff',
  selectionBackground: 'rgba(99,102,241,0.22)',
  black: '#1e293b', red: '#dc2626', green: '#047857', yellow: '#a16207', blue: '#1d4ed8', magenta: '#9333ea', cyan: '#0e7490', white: '#64748b',
  brightBlack: '#64748b', brightRed: '#ef4444', brightGreen: '#059669', brightYellow: '#b45309', brightBlue: '#2563eb', brightMagenta: '#a855f7', brightCyan: '#0891b2', brightWhite: '#334155',
};
const termTheme = () => (document.documentElement.dataset.theme === 'dark' ? TERM_DARK : TERM_LIGHT);

/* ================================================================ light / dark */
function applyTheme(theme, save = true) {
  theme = theme === 'dark' ? 'dark' : 'light';
  const root = document.documentElement;
  if (root.dataset.theme !== theme) {
    document.body.classList.add('theme-anim');
    root.dataset.theme = theme;
    setTimeout(() => document.body.classList.remove('theme-anim'), 250);
  }
  store.set('theme', theme);
  const btn = $('#btnTheme');
  btn.innerHTML = icon(theme === 'dark' ? 'sun' : 'moon');
  btn.title = theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode';
  S.terms.forEach((t) => { t.term.options.theme = termTheme(); });
  if (save) api('/api/ui', { theme }).catch(() => {}); // the app window's title bar follows
}
$('#btnTheme').onclick = () => applyTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark');

function openTerminal(serverId, focus = true) {
  const s = S.servers.get(serverId);
  if (!s) return;
  const id = 't' + ++termSeq;
  const pane = document.createElement('div');
  pane.className = 'term-pane';
  pane.innerHTML = `<div class="term-head"><span class="dot checking"></span><span>${esc(s.name)}</span><span class="grow">${esc(s.username)}@${esc(s.host)}</span>
      <button data-a="reconnect" title="Reconnect">${icon('reconnect')}</button><button data-a="close" title="Close">${icon('x')}</button></div>
    <div class="term-body"><div class="term-overlay"><div class="box"><div class="spinner"></div><div class="msg">Connecting to ${esc(s.name)}…</div></div></div></div>`;
  $('#terms').appendChild(pane);
  const term = new Terminal({
    theme: termTheme(), fontFamily: '"Cascadia Code", "Cascadia Mono", Consolas, monospace', fontSize: 13.5, lineHeight: 1.15,
    cursorBlink: true, cursorStyle: 'bar', scrollback: 10000, allowProposedApi: true, macOptionIsMeta: true,
  });
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.loadAddon(new WebLinksAddon.WebLinksAddon((_e, uri) => api('/api/open', { url: uri }).catch(() => {})));
  const body = $('.term-body', pane);
  term.open(body);
  const t = { id, serverId, name: s.name, term, fit, pane, ws: null, ready: false, cols: 0, rows: 0 };
  S.terms.set(id, t);

  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== 'keydown') return true;
    const k = e.key.toLowerCase();
    if (e.ctrlKey && (k === 'c') && (e.shiftKey || term.hasSelection())) {
      if (term.hasSelection()) { navigator.clipboard.writeText(term.getSelection()); term.clearSelection(); }
      return false;
    }
    if (e.ctrlKey && k === 'v') { navigator.clipboard.readText().then((txt) => txt && term.paste(txt)); return false; }
    return true;
  });
  body.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    if (term.hasSelection()) { navigator.clipboard.writeText(term.getSelection()); term.clearSelection(); toast('Copied', 'ok'); }
    else navigator.clipboard.readText().then((txt) => txt && term.paste(txt));
  });
  term.onData((data) => sendInput(t, data));
  term.textarea?.addEventListener('focus', () => { S.activeTerm = id; markFocus(); });
  pane.addEventListener('click', (e) => {
    const a = e.target.closest('[data-a]')?.dataset.a;
    if (a === 'close') return closeTerminal(id);
    if (a === 'reconnect') return connectTerm(t);
    if (e.target.closest('[data-retry]')) return connectTerm(t);
  });
  new ResizeObserver(() => fitTerm(t)).observe(body);

  addTermTab(t);
  if (focus || !S.activeTerm) activateTerm(id);
  else showView('terms');
  requestAnimationFrame(() => { fitTerm(t); connectTerm(t); });
}

function addTermTab(t) {
  const tab = document.createElement('button');
  tab.className = 'tab';
  tab.dataset.term = t.id;
  const sameName = [...S.terms.values()].filter((x) => x.serverId === t.serverId).length;
  tab.innerHTML = `<span class="dot checking"></span><span>${esc(t.name)}${sameName > 1 ? ` (${sameName})` : ''}</span><span class="x" title="Close">${icon('x')}</span>`;
  tab.onclick = (e) => (e.target.closest('.x') ? closeTerminal(t.id) : activateTerm(t.id));
  tab.onauxclick = (e) => { if (e.button === 1) closeTerminal(t.id); };
  $('#termTabs').appendChild(tab);
  t.tab = tab;
}

function setTermState(t, state, msg, retry) {
  const cls = state === 'ready' ? 'online' : state === 'error' ? 'offline' : 'checking';
  $$('.dot', t.tab).concat($$('.term-head .dot', t.pane)).forEach((d) => (d.className = `dot ${cls}`));
  let overlay = $('.term-overlay', t.pane);
  if (state === 'ready') { overlay?.remove(); return; }
  if (!overlay) { overlay = document.createElement('div'); overlay.className = 'term-overlay'; $('.term-body', t.pane).appendChild(overlay); }
  overlay.classList.toggle('err', state === 'error');
  overlay.innerHTML = `<div class="box">${state === 'connecting' ? '<div class="spinner"></div>' : icon(state === 'error' ? 'wifi_off' : 'terminal')}
    <div class="msg">${esc(msg)}</div>${retry ? `<button class="btn btn-sm" data-retry>${icon('reconnect')}Reconnect</button>` : ''}</div>`;
}

function connectTerm(t) {
  if (t.ws) { t.ws.onclose = null; t.ws.close(); }
  t.ready = false;
  setTermState(t, 'connecting', `Connecting to ${t.name}…`);
  fitTerm(t);
  const cols = t.term.cols || 120, rows = t.term.rows || 32;
  const ws = new WebSocket(`ws://${location.host}/ws/term?server=${t.serverId}&cols=${cols}&rows=${rows}&t=${encodeURIComponent(TOKEN)}`);
  ws.binaryType = 'arraybuffer';
  t.ws = ws;
  ws.onmessage = (m) => {
    if (typeof m.data !== 'string') return t.term.write(new Uint8Array(m.data));
    const msg = JSON.parse(m.data);
    if (msg.type === 'ready') {
      t.ready = true;
      t.cols = cols; t.rows = rows;
      setTermState(t, 'ready');
      fitTerm(t, true);
      if (S.activeTerm === t.id) t.term.focus();
      updateBroadcast();
    } else if (msg.type === 'error') {
      t.ready = false;
      setTermState(t, 'error', msg.text, true);
    } else if (msg.type === 'exit') {
      t.ready = false;
      setTermState(t, 'ended', 'Session ended.', true);
    }
  };
  ws.onclose = () => {
    if (t.ready) { t.ready = false; setTermState(t, 'ended', 'Connection closed.', true); }
    updateBroadcast();
  };
}

function fitTerm(t, force) {
  if (!t.pane.offsetParent) return;
  try { t.fit.fit(); } catch { return; }
  const { cols, rows } = t.term;
  if (t.ready && (force || cols !== t.cols || rows !== t.rows)) {
    t.cols = cols; t.rows = rows;
    t.ws.send(JSON.stringify({ type: 'resize', cols, rows }));
  }
}
function fitVisible() { S.terms.forEach((t) => fitTerm(t)); }

function sendInput(from, data) {
  const targets = S.broadcast ? [...S.terms.values()].filter((x) => x.ready) : [from];
  for (const t of targets) if (t.ready) t.ws.send(JSON.stringify({ type: 'in', data }));
}

function activateTerm(id) {
  S.activeTerm = id;
  S.terms.forEach((t) => t.pane.classList.toggle('on', t.id === id));
  showView('terms');
  markFocus();
  const t = S.terms.get(id);
  requestAnimationFrame(() => { fitTerm(t); t.term.focus(); });
}
function markFocus() {
  S.terms.forEach((t) => t.pane.classList.toggle('focus', t.id === S.activeTerm));
  $$('#termTabs .tab').forEach((tab) => tab.classList.toggle('on', S.view === 'terms' && tab.dataset.term === S.activeTerm));
}

function closeTerminal(id) {
  const t = S.terms.get(id);
  if (!t) return;
  if (t.ws) { t.ws.onclose = null; t.ws.close(); }
  t.term.dispose();
  t.pane.remove();
  t.tab.remove();
  S.terms.delete(id);
  if (!S.terms.size) { setBroadcast(false); setTile(false); S.activeTerm = null; return showView('overview'); }
  if (S.activeTerm === id) activateTerm([...S.terms.keys()].pop());
  layoutTiles();
  updateBroadcast();
}

function layoutTiles() {
  const n = S.terms.size;
  const el = $('#terms');
  el.classList.toggle('tile', S.tile);
  const cols = S.tile ? (n <= 1 ? 1 : n <= 4 ? 2 : n <= 9 ? 3 : 4) : 1;
  el.style.gridTemplateColumns = `repeat(${cols}, minmax(0, 1fr))`;
  requestAnimationFrame(fitVisible);
}
function setTile(on) {
  S.tile = on;
  $('#btnTile').classList.toggle('on', on);
  layoutTiles();
}
$('#btnTile').onclick = () => setTile(!S.tile);

function setBroadcast(on) {
  S.broadcast = on;
  $('#btnBroadcast').classList.toggle('on', on);
  if (on && !S.tile && S.terms.size > 1) setTile(true);
  updateBroadcast();
}
function updateBroadcast() {
  const n = [...S.terms.values()].filter((t) => t.ready).length;
  $('#bcBanner').hidden = !S.broadcast;
  $('#bcCount').textContent = n;
  $('#terms').classList.toggle('broadcast', S.broadcast);
  requestAnimationFrame(fitVisible);
}
$('#btnBroadcast').onclick = () => setBroadcast(!S.broadcast);
$('#bcOff').onclick = () => setBroadcast(false);

/* ================================================================ server modal */
let editing = null;
let authType = 'key';
function setAuth(type) {
  authType = type;
  $$('#authSeg button').forEach((b) => b.classList.toggle('on', b.dataset.auth === type));
  $('#authKey').hidden = type !== 'key';
  $('#authPass').hidden = type !== 'password';
}
$$('#authSeg button').forEach((b) => (b.onclick = () => setAuth(b.dataset.auth)));

function openServerModal(s) {
  editing = s || null;
  const f = $('#serverForm');
  f.reset();
  $('#smTitle').textContent = s ? `Edit ${s.name}` : 'Add server';
  $('#smDelete').hidden = !s;
  $('#smForget').hidden = !s || !s.host_key;
  $('#smSecretHint').hidden = !s;
  $('#folderList').innerHTML = folders().filter((x) => x !== 'Ungrouped').map((x) => `<option value="${esc(x)}">`).join('');
  if (s) {
    for (const k of ['name', 'folder', 'host', 'port', 'username', 'tags', 'default_path']) f.elements[k].value = s[k] ?? '';
    f.elements.pinned.checked = !!s.pinned;
    f.elements.private_key.placeholder = s.has_private_key ? '•••• saved key (leave empty to keep)' : '-----BEGIN OPENSSH PRIVATE KEY-----';
    f.elements.password.placeholder = s.has_password ? '•••• saved (leave empty to keep)' : '';
    f.elements.sudo_password.placeholder = s.has_sudo_password ? '•••• saved (leave empty to keep)' : '';
    f.elements.key_passphrase.placeholder = s.has_key_passphrase ? '•••• saved' : '';
  } else if (S.folder !== 'all' && S.folder !== 'Ungrouped') {
    f.elements.folder.value = S.folder;
  }
  setAuth(s?.auth_type || 'key');
  $('#serverModal').hidden = false;
  setTimeout(() => f.elements[s ? 'name' : 'host'].focus(), 30);
}
window.openServerModal = openServerModal;

$('#keyFileBtn').onclick = () => $('#keyFile').click();
$('#keyFile').onchange = async (e) => {
  const file = e.target.files[0];
  if (file) $('#serverForm').elements.private_key.value = await file.text();
  e.target.value = '';
};

$('#serverForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const f = e.target.elements;
  const data = {
    name: f.name.value, folder: f.folder.value, host: f.host.value, port: +f.port.value || 22, username: f.username.value,
    tags: f.tags.value, default_path: f.default_path.value, pinned: f.pinned.checked, auth_type: authType,
  };
  for (const k of ['password', 'private_key', 'key_passphrase', 'sudo_password']) if (f[k].value) data[k] = f[k].value;
  if (!editing && authType === 'key' && !data.private_key) return toast('Paste or load a private key', 'bad');
  if (!editing && authType === 'password' && !data.password) return toast('Enter the password', 'bad');
  try {
    const saved = editing ? await api(`/api/servers/${editing.id}`, data, 'PUT') : await api('/api/servers', data);
    S.servers.set(saved.id, saved);
    S.probing.add(saved.id);
    closeModals();
    render();
    toast(`${saved.name} saved — testing connection…`, 'ok');
  } catch (err) { toast(err.message, 'bad'); }
});

$('#smDelete').onclick = async () => {
  if (!editing) return;
  if (!(await confirmBox(`Delete ${editing.name}?`, 'The server and its saved credentials are removed from SSH Pro AI. Nothing changes on the server itself.'))) return;
  await api(`/api/servers/${editing.id}`, undefined, 'DELETE');
  S.servers.delete(editing.id);
  S.selected.delete(editing.id);
  closeModals();
  render();
  toast('Server deleted', 'ok');
};
$('#smForget').onclick = async () => {
  await api(`/api/servers/${editing.id}/forget-key`, {});
  editing.host_key = null;
  $('#smForget').hidden = true;
  toast('Host key forgotten — the next connection trusts the new key', 'ok');
};

function confirmBox(title, text) {
  return new Promise((resolve) => {
    $('#cfTitle').textContent = title;
    $('#cfText').textContent = text;
    $('#confirmModal').hidden = false;
    const done = (v) => { $('#confirmModal').hidden = true; resolve(v); };
    $('#cfYes').onclick = () => done(true);
    $('#cfNo').onclick = () => done(false);
  });
}
function closeModals() { $$('.modal-back').forEach((m) => { if (m.id !== 'confirmModal') m.hidden = true; }); }
$$('[data-close]').forEach((b) => (b.onclick = closeModals));
$$('.modal-back').forEach((m) => m.addEventListener('mousedown', (e) => { if (e.target === m && m.id !== 'confirmModal') closeModals(); }));
$('#btnAdd').onclick = () => openServerModal();

/* ================================================================ import / audit */
async function importTermix() {
  try {
    const r = await api('/api/import', {});
    toast(`Imported ${r.imported} server${r.imported === 1 ? '' : 's'}${r.skipped ? ` · ${r.skipped} already here` : ''}`, 'ok');
    await loadServers();
  } catch (err) { toast(err.message, 'bad'); }
}
window.importTermix = importTermix;
$('#btnImport').onclick = importTermix;

/* ---------------------------------------------------------------- backup */
$('#btnBackup').onclick = () => {
  $('#backupExportForm').reset();
  $('#backupImportForm').reset();
  $('#backupModal').hidden = false;
};

$('#backupExportForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  try {
    const backup = await api('/api/backup/export', {});
    const blob = new Blob([JSON.stringify(backup, null, 1)], { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `sshpro-backup-${new Date().toISOString().slice(0, 10)}.sshpro`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 60000);
    closeModals();
    toast(`Backup of ${backup.servers.length} server${backup.servers.length === 1 ? '' : 's'} ready — choose where to save it`, 'ok');
  } catch (err) { toast(err.message, 'bad'); }
});

$('#backupImportForm').addEventListener('submit', async (e) => {
  e.preventDefault();
  const f = e.target.elements;
  let backup;
  try { backup = JSON.parse(await f.file.files[0].text()); } catch { return toast('That file is not an SSH Pro AI backup', 'bad'); }
  try {
    const r = await api('/api/backup/import', { backup });
    closeModals();
    toast(`Restored ${r.imported} server${r.imported === 1 ? '' : 's'}${r.skipped ? ` · ${r.skipped} already here` : ''}`, 'ok');
    await loadServers();
  } catch (err) { toast(err.message, 'bad'); }
});

$('#btnAudit').onclick = async () => {
  const rows = await api('/api/audit');
  $('#auditBody').innerHTML = rows.length
    ? `<div class="tbl-wrap" style="max-height:none"><table class="tbl"><thead><tr><th>Time</th><th>By</th><th>Server</th><th>Command</th><th>Exit</th><th>Took</th></tr></thead><tbody>${rows.map((r) => `<tr>
        <td style="white-space:nowrap">${esc(new Date(r.ts).toLocaleString())}</td><td><span class="kind ${r.kind}">${r.kind === 'ai' ? 'AI' : 'You'}</span></td>
        <td>${esc(r.server_name)}</td><td class="cmd" title="${esc(r.prompt || '')}">${esc(r.command)}</td>
        <td class="${r.exit_code === 0 ? 'pos' : 'neg'}">${r.exit_code ?? '—'}</td><td>${r.duration ?? '—'}s</td></tr>`).join('')}</tbody></table></div>`
    : '<p>No commands yet.</p>';
  $('#auditModal').hidden = false;
};

/* ================================================================ boot */
document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key.toLowerCase() === 'k') { e.preventDefault(); $('#search').focus(); $('#search').select(); }
  if (e.key === 'Escape' && !$('#serverModal').hidden) closeModals();
  if (e.key === 'Escape' && !$('#auditModal').hidden) closeModals();
  if (e.key === 'Escape' && !$('#historyModal').hidden) closeModals();
  if (e.key === 'Escape' && !$('#backupModal').hidden) closeModals();
});

async function loadServers() {
  const list = await api('/api/servers');
  S.servers = new Map(list.map((s) => [s.id, s]));
  list.forEach((s) => (s.last_error ? S.probeErr.set(s.id, s.last_error) : S.probeErr.delete(s.id)));
  render();
}

// The AI brain: every model runs on the AGY CLI.
const brainName = () => 'AGY';

const AUTO = 'auto';
const options = (list) => list.map((m) => `<option value="${esc(m.id)}">${esc(m.label)}</option>`).join('');

async function loadModels() {
  const sel = $('#modelSel');
  try {
    const { models, current, auto } = await api('/api/ai/models');
    const known = current === AUTO || models.some((m) => m.id === current);
    const list = known ? models : [{ id: current, label: `${current} (not available)` }, ...models];
    sel.innerHTML = `<option value="${AUTO}">⚡ Auto (fast ↔ smart)</option>` + options(list);
    sel.value = current;
    sel.hidden = false;
    $('#modelName').hidden = true;
    // Auto's two models: pick from the same list
    for (const [id, value] of [['#autoFast', auto?.fast], ['#autoSmart', auto?.smart]]) {
      const pick = $(id);
      const extra = value && !models.some((m) => m.id === value) ? [{ id: value, label: `${value} (not available)` }] : [];
      pick.innerHTML = options([...extra, ...models]);
      if (value) pick.value = value;
    }
    $('#autoPill').hidden = current !== AUTO;
  } catch (err) {
    toast(`Could not load AI models: ${err.message}`, 'bad');
  }
}

$('#modelSel').onchange = async (e) => {
  const sel = e.target;
  const prev = S.meta.model;
  sel.disabled = true;
  try {
    const { model } = await api('/api/ai/model', { model: sel.value });
    S.meta.model = model;
    S.wins.forEach((w) => clearChat(w)); // every window starts fresh on the new model
    $('#autoPill').hidden = model !== AUTO;
    if (model === AUTO) loadModels(); // shows the fast / smart models Auto will use
    toast(`AI model: ${sel.selectedOptions[0].textContent} — new chat started`, 'ok');
  } catch (err) {
    sel.value = prev;
    toast(err.message, 'bad');
  } finally {
    sel.disabled = false;
  }
};

const saveAuto = async () => {
  try {
    await api('/api/ai/auto', { fast: $('#autoFast').value, smart: $('#autoSmart').value });
    toast(`Auto: fast = ${$('#autoFast').selectedOptions[0].textContent}, smart = ${$('#autoSmart').selectedOptions[0].textContent}`, 'ok');
  } catch (err) {
    toast(err.message, 'bad');
  }
};
$('#autoFast').onchange = saveAuto;
$('#autoSmart').onchange = saveAuto;

$('#agyInstallLink').onclick = () => api('/api/open', { url: S.meta.agy_install_url }).catch((err) => toast(err.message, 'bad'));

// Windows come back after a reload or restart, each with its chat and targets. The backend's own
// state wins (a window still working shows its live progress and a pending approval).
async function restoreWins() {
  const live = S.meta.windows || {};
  for (const w of S.wins.values()) {
    const st = live[w.id];
    const chat = st ? st.chat : w.chat;
    if (chat) {
      try { await openChat(chat, w, false); } catch { clearChat(w); }
    } else if (w.chat) clearChat(w);
    if (st?.busy) {
      RW = w;
      w.state = st.waiting ? 'waiting' : 'running';
      setBusy(true);
      if (st.waiting) { // the approval it waits for can still be given
        const job = [...w.jobs.values()].pop();
        const bar = job && [...job.rounds.values()].pop()?.approveBar;
        if (bar?.live) { bar.classList.remove('done'); bar.innerHTML = bar.live; }
      }
    }
  }
  renderWinTabs();
}

async function boot() {
  applyTheme(document.documentElement.dataset.theme, false); // the button shows the other mode
  hydrateIcons();
  for (const saved of store.get('wins', [])) if (saved?.id && S.wins.size < MAX_WINS) makeWin(saved);
  if (!S.wins.size) makeWin({ id: 'main' });
  activateWin(S.wins.has(store.get('win')) ? store.get('win') : [...S.wins.keys()][0]);
  showView('overview');
  connectEvents();
  try {
    await loadServers();
    S.meta = await api('/api/meta');
    if (S.meta.theme && S.meta.theme !== document.documentElement.dataset.theme) applyTheme(S.meta.theme, false);
    $('#modelName').textContent = S.meta.model || 'AI';
    const brains = [S.meta.agy && 'AGY'].filter(Boolean);
    $('#brandSub').textContent = `v${S.meta.version} · ${brains.length ? brains.join(' + ') : 'AI'} brain${brains.length ? '' : ' (not found)'}`;
    if (!brains.length) {
      $('#modelPill .pulse').style.background = 'var(--bad)';
      $('#modelName').textContent = 'No AI brain installed';
      setMode(S.mode);
    } else {
      loadModels();
    }
    await restoreWins();
  } catch (err) {
    toast(err.message, 'bad');
  }
}
boot();
