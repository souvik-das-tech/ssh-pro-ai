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
  target: 'all',
  terms: new Map(),
  activeTerm: null,
  tile: false,
  broadcast: false,
  jobs: new Map(),
  batches: new Map(),
  chat: null, // chat history id shown in the AI page (null = new chat, created on its first message)
  lastChat: 0, // highest chat id seen
  chatFloor: 0, // a new chat's id is above this
  aiBusy: false,
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
  if (act === 'ask') { S.target = `server:${id}`; renderTargets(); showView('ai'); $('#prompt').focus(); }
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
];
$('#suggest').innerHTML = SUGGESTIONS.map((s, i) => `<div class="sg" data-i="${i}"><div class="ico ${s.c}">${icon(s.ico)}</div><div><b>${s.title}</b><span>${s.text}</span></div></div>`).join('');
$('#suggest').addEventListener('click', (e) => {
  const sg = e.target.closest('.sg');
  if (!sg) return;
  setMode('ai');
  $('#prompt').value = SUGGESTIONS[+sg.dataset.i].prompt;
  autoGrow();
  send();
});

function renderTargets() {
  const all = serverList();
  const count = (f) => all.filter((s) => folderName(s) === f).length;
  const online = (ids) => ids.filter((id) => S.servers.get(id)?.last_status === 'online').length;
  if (S.target === 'selected' && !S.selected.size) S.target = 'all';
  if (S.target.startsWith('folder:') && !folders().includes(S.target.slice(7))) S.target = 'all';
  if (S.target.startsWith('server:') && !S.servers.has(+S.target.slice(7))) S.target = 'all';
  const chips = [['all', 'All servers', all.length, icon('layers')], ...folders().map((f) => [`folder:${f}`, f, count(f), icon('folder')])];
  if (S.selected.size) chips.push(['selected', 'Selected', S.selected.size, icon('check')]);
  $('#targetChips').innerHTML = chips.map(([value, label, n, ic]) =>
    `<button class="chip ${S.target === value ? 'on' : ''}" data-target="${esc(value)}" title="${esc(label)}: ${n} servers">${ic}${esc(label)} <b>${n}</b></button>`).join('');
  const sel = $('#targetSel');
  sel.innerHTML = `<option value="">One server…</option>` + all.map((s) => `<option value="server:${s.id}">${esc(s.name)}</option>`).join('');
  sel.value = S.target.startsWith('server:') ? S.target : '';
  sel.classList.toggle('on', !!sel.value);
  const ids = targetIds();
  $('#targetInfo').textContent = `${online(ids)}/${ids.length} online`;
  updateComposerHint();
}
function setTarget(value) { S.target = value || 'all'; renderTargets(); }
$('#targetChips').addEventListener('click', (e) => { const c = e.target.closest('[data-target]'); if (c) setTarget(c.dataset.target); });
$('#targetSel').onchange = (e) => setTarget(e.target.value);

function targetIds() {
  const all = serverList();
  if (S.target === 'selected') return [...S.selected];
  if (S.target.startsWith('folder:')) { const f = S.target.slice(7); return all.filter((s) => folderName(s) === f).map((s) => s.id); }
  if (S.target.startsWith('server:')) return [+S.target.slice(7)];
  return all.map((s) => s.id);
}
function targetLabel() {
  if (S.target === 'selected') return `Selected (${S.selected.size})`;
  if (S.target.startsWith('folder:')) return `Folder: ${S.target.slice(7)}`;
  if (S.target.startsWith('server:')) return S.servers.get(+S.target.slice(7))?.name || 'server';
  return 'All servers';
}

function setMode(mode) {
  S.mode = mode;
  $$('#modeSeg button').forEach((b) => b.classList.toggle('on', b.dataset.mode === mode));
  $('#composer').classList.toggle('direct', mode === 'direct');
  const noAgy = mode === 'ai' && S.meta.agy === null;
  $('#agyMissing').hidden = !noAgy;
  $('#chatEmpty').classList.toggle('dim', noAgy);
  $('#prompt').placeholder = noAgy ? 'AI chat needs the AGY CLI — install it (see above), or switch to Direct mode'
    : mode === 'ai' ? 'Ask in any language — e.g. “sob server er node version check koro”' : 'Shell command to run on every target server — e.g. df -h /';
  updateComposerHint();
}
$$('#modeSeg button').forEach((b) => (b.onclick = () => setMode(b.dataset.mode)));

function updateComposerHint() {
  const n = targetIds().length;
  $('#composerHint').textContent = S.mode === 'ai'
    ? `AI mode · ${n} target server${n === 1 ? '' : 's'} · read-only checks run at once, changes wait for your approval · Enter to send`
    : `Direct mode · runs exactly what you type on ${n} server${n === 1 ? '' : 's'} in parallel · Enter to run`;
}

function autoGrow() {
  const t = $('#prompt');
  t.style.height = 'auto';
  t.style.height = Math.min(200, t.scrollHeight) + 'px';
}
$('#prompt').addEventListener('input', autoGrow);
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
$('#btnStop').onclick = () => api('/api/ai/stop', {});
function clearChat() {
  $$('#chatInner > :not(#chatEmpty)').forEach((el) => el.remove());
  $('#chatEmpty').hidden = false;
  S.jobs.clear();
  S.batches.clear();
  S.chatFloor = Math.max(S.lastChat, S.chat ?? 0);
  S.chat = null;
}

/* ---------------------------------------------------------------- chat history */
async function openChat(id) {
  const chat = await api(`/api/chats/${id}/open`, {});
  clearChat();
  S.chat = chat.id;
  S.lastChat = Math.max(S.lastChat, chat.id);
  for (const ev of chat.events) chatEvent(ev, true);
  // steps that never reported back (stopped, app closed) stop spinning
  const states = [...S.batches.values(), ...[...S.jobs.values()].flatMap((j) => [...j.rounds.values()])];
  for (const st of states) {
    for (const r of st.rows.values()) {
      if (r.got >= r.expected) continue;
      $('.st', r.row).className = 'st warn';
      $('.st', r.row).innerHTML = svg('alert');
      if (!r.got) $('.rpreview', r.row).textContent = 'not run';
    }
  }
  forceScroll();
}

function ago(ts) {
  const min = Math.round((Date.now() - new Date(ts)) / 60000);
  if (min < 1) return 'just now';
  if (min < 60) return `${min} min ago`;
  if (min < 1440) return `${Math.round(min / 60)} h ago`;
  return `${Math.round(min / 1440)} d ago`;
}

async function showHistory() {
  const { chats, current, days } = await api('/api/chats');
  $('#historySub').textContent = `Each chat is kept for ${days} days after its last message`;
  const body = $('#historyBody');
  body.innerHTML = chats.length
    ? chats.map((c) => `<div class="hrow ${c.id === current ? 'on' : ''}" data-id="${c.id}">
        <div class="hmain"><div class="htitle">${esc(c.title)}</div>
        <div class="hmeta">${esc(ago(c.updated_at))} · ${c.messages} message${c.messages === 1 ? '' : 's'} · expires in ${Math.max(0, Math.ceil(days - (Date.now() - new Date(c.updated_at)) / 86400000))} d${c.id === current ? ' · <b>open now</b>' : ''}</div></div>
        <button class="btn btn-ghost btn-icon btn-sm" data-del title="Delete this chat">${icon('trash')}</button></div>`).join('')
    : '<p class="muted">No chats yet. Your conversations show up here.</p>';
  body.onclick = async (e) => {
    const row = e.target.closest('.hrow');
    if (!row) return;
    const id = Number(row.dataset.id);
    try {
      if (e.target.closest('[data-del]')) {
        if (!(await confirmBox('Delete chat?', 'This chat and its results are removed for good. The audit log keeps its commands.'))) return;
        await api(`/api/chats/${id}`, undefined, 'DELETE');
        if (id === S.chat) clearChat();
        return showHistory();
      }
      closeModals();
      await openChat(id);
    } catch (err) { toast(err.message, 'bad'); }
  };
  $('#historyModal').hidden = false;
}
$('#btnHistory').onclick = () => showHistory().catch((err) => toast(err.message, 'bad'));
$('#btnNewChat').onclick = async () => {
  await api('/api/ai/reset', {});
  clearChat();
  toast('Fresh AI conversation started', 'ok');
};

function setBusy(busy) {
  S.aiBusy = busy;
  $('#btnStop').hidden = !busy;
  $('#btnSend').disabled = busy && S.mode === 'ai';
}

function scrollChat() {
  const c = $('#chat');
  if (c.scrollHeight - c.scrollTop - c.clientHeight < 260) c.scrollTop = c.scrollHeight;
}
function forceScroll() { const c = $('#chat'); c.scrollTop = c.scrollHeight; }

function addBlock(kind) {
  $('#chatEmpty').hidden = true;
  const block = document.createElement('div');
  block.className = 'ai-block';
  block.innerHTML = `<div class="ai-avatar ${kind === 'direct' ? 'direct' : ''}">${icon(kind === 'direct' ? 'terminal' : 'sparkles')}</div><div class="ai-col"></div>`;
  $('#chatInner').appendChild(block);
  return $('.ai-col', block);
}

async function send() {
  const text = $('#prompt').value.trim();
  if (!text) return;
  const ids = targetIds();
  if (!ids.length) return toast('No target servers selected', 'bad');
  const target = targetLabel();
  const user = userBubble({ mode: S.mode, text, target });
  $('#chatEmpty').hidden = true;
  if (S.mode === 'ai') {
    if (S.meta.agy === null) { $('#agyMissing').hidden = false; return toast('AI chat needs the AGY CLI — install it first', 'bad'); }
    if (S.aiBusy) return toast('The AI is still working — stop it or wait', 'bad');
    try {
      $('#chatInner').appendChild(user);
      const col = addBlock('ai');
      setBusy(true);
      const { job } = await api('/api/ai/ask', { prompt: text, ids: S.target === 'all' ? [] : ids, target });
      S.jobs.set(job, { col, rounds: new Map(), statusEl: null, servers: ids.length });
      setStatus(job, 'thinking');
    } catch (err) {
      setBusy(false);
      toast(err.message, 'bad');
      return;
    }
  } else {
    $('#chatInner').appendChild(user);
    api('/api/exec', { command: text, ids, target }).catch((err) => toast(err.message, 'bad'));
  }
  remember(text);
  $('#prompt').value = '';
  autoGrow();
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
  if (!S.jobs.has(id)) S.jobs.set(id, { col: addBlock('ai'), rounds: new Map(), statusEl: null });
  return S.jobs.get(id);
}

function setStatus(jobId, state, text) {
  const job = S.jobs.get(jobId);
  if (!job) return;
  if (!job.statusEl) { job.statusEl = document.createElement('div'); job.statusEl.className = 'thinking'; }
  const labels = {
    thinking: text || (job.rounds.size ? 'AGY is analysing the results…' : 'AGY is planning…'),
    running: text || 'Running on servers…',
    waiting: 'Waiting for your approval',
  };
  if (labels[state]) {
    job.statusEl.innerHTML = state === 'waiting' ? `${icon('shield')}<span>${labels[state]}</span>` : `<span class="dots"><i></i><i></i><i></i></span><span>${esc(labels[state])}</span>`;
    job.col.appendChild(job.statusEl);
  } else {
    job.statusEl.remove();
    setBusy(false);
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
    bar.addEventListener('click', async (e) => {
      const a = e.target.closest('[data-a]')?.dataset.a;
      if (!a) return;
      await api('/api/ai/approve', { approve: a !== 'no', mode: a === 'all' ? 'parallel' : 'canary' });
      markDecision(bar, a !== 'no', a === 'all' ? 'parallel' : 'canary');
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
  // the first message of a new chat just created it (chat ids only grow, so a late event of the
  // chat that was just closed cannot claim the new one)
  if (S.chat == null && ev.chat > S.chatFloor) S.chat = ev.chat;
  if (ev.chat !== S.chat) return; // another chat (e.g. one still running when you switched away)
  if (ev.type === 'ai.status') return setStatus(ev.job, ev.state, ev.text);
  chatEvent(ev, false);
}

// Draws one chat event; used live and to replay a reopened chat from its history.
function chatEvent(ev, replay) {
  switch (ev.type) {
    case 'chat.user':
      $('#chatEmpty').hidden = true;
      return $('#chatInner').appendChild(userBubble(ev));
    case 'ai.plan': return renderPlan(ev, replay);
    case 'ai.decision': {
      const bar = S.jobs.get(ev.job)?.rounds.get(ev.round)?.approveBar;
      if (bar) markDecision(bar, ev.approve, ev.mode);
      return;
    }
    case 'ai.result': {
      const job = S.jobs.get(ev.job);
      const state = job?.rounds.get(ev.round);
      if (state) applyResult(state, ev);
      return;
    }
    case 'ai.answer': return renderAnswer(ev);
    case 'ai.error': {
      const job = ensureJob(ev.job);
      const el = document.createElement('div');
      el.className = 'error-box';
      el.textContent = /AIAuthError|not signed in/i.test(ev.text) ? 'AGY is not signed in. Open a terminal, run `agy`, sign in, then try again.' : ev.text;
      job.col.appendChild(el);
      return scrollChat();
    }
    case 'exec.start': {
      const col = addBlock('direct');
      const state = resultsPanel(`$ ${ev.command}`, ev.servers.map((s) => ({ ...s, count: 1 })));
      col.appendChild(state.panel);
      S.batches.set(ev.batch, state);
      return forceScroll();
    }
    case 'exec.result': {
      const state = S.batches.get(ev.batch);
      if (state) applyResult(state, ev);
      return;
    }
    default:
  }
}

/* ================================================================ terminals */
const TERM_THEME = {
  background: '#0b1019', foreground: '#dbe2ef', cursor: '#22d3ee', cursorAccent: '#0b1019',
  selectionBackground: 'rgba(99,102,241,0.45)',
  black: '#1b2130', red: '#f87171', green: '#34d399', yellow: '#fbbf24', blue: '#60a5fa', magenta: '#c084fc', cyan: '#22d3ee', white: '#dbe2ef',
  brightBlack: '#5b6478', brightRed: '#fca5a5', brightGreen: '#6ee7b7', brightYellow: '#fde68a', brightBlue: '#93c5fd', brightMagenta: '#d8b4fe', brightCyan: '#67e8f9', brightWhite: '#ffffff',
};

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
    theme: TERM_THEME, fontFamily: '"Cascadia Code", "Cascadia Mono", Consolas, monospace', fontSize: 13.5, lineHeight: 1.15,
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

async function loadModels() {
  const sel = $('#modelSel');
  try {
    const { models, current } = await api('/api/ai/models');
    const list = models.some((m) => m.id === current) ? models : [{ id: current, label: `${current} (not available)` }, ...models];
    sel.innerHTML = list.map((m) => `<option value="${esc(m.id)}">${esc(m.label)}</option>`).join('');
    sel.value = current;
    sel.hidden = false;
    $('#modelName').hidden = true;
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
    clearChat();
    toast(`AI model: ${sel.selectedOptions[0].textContent} — new chat started`, 'ok');
  } catch (err) {
    sel.value = prev;
    toast(err.message, 'bad');
  } finally {
    sel.disabled = false;
  }
};

$('#agyInstallLink').onclick = () => api('/api/open', { url: S.meta.agy_install_url }).catch((err) => toast(err.message, 'bad'));

async function boot() {
  hydrateIcons();
  setMode('ai');
  showView('overview');
  connectEvents();
  try {
    await loadServers();
    S.meta = await api('/api/meta');
    $('#modelName').textContent = S.meta.model || 'AGY';
    $('#brandSub').textContent = `v${S.meta.version} · AGY brain${S.meta.agy ? '' : ' (not found)'}`;
    if (!S.meta.agy) {
      $('#modelPill .pulse').style.background = 'var(--bad)';
      $('#modelName').textContent = 'AGY not installed';
      setMode(S.mode);
    } else {
      loadModels();
    }
    if (S.meta.chat) await openChat(S.meta.chat); // page reload: show the conversation that is still open
  } catch (err) {
    toast(err.message, 'bad');
  }
}
boot();
