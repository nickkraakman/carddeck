// CardDeck UI. Talks to the Rust side through Tauri commands; no build step.
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (id) => document.getElementById(id);

const state = { card: null, view: 'front', zoom: 1, fit: true };

function setStatus(text, kind) {
  const el = $('status');
  el.textContent = text || '';
  el.className = 'status' + (kind ? ' ' + kind : '');
}
function esc(s) { return String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c])); }

// ---- viewer -----------------------------------------------------------------------------------

function render() {
  const c = state.card;
  $('empty').classList.toggle('hidden', !!c);
  $('stage').classList.toggle('hidden', !c);
  if (!c) { $('meta').textContent = ''; return; }
  $('meta').innerHTML = `<b>${esc(c.name)}</b> · ${esc(c.type_name)} ${c.width}×${c.height}px${c.portrait ? ' portrait' : ''} · Hub ${esc(c.hub_version || '?')} · ${c.double_sided ? 'double-sided' : 'single-sided'}`;
  const sides = state.view === 'both' ? ['front', 'back'] : [state.view];
  const stage = $('stage');
  stage.innerHTML = '';
  const avail = $('viewer').clientWidth - 40;
  let zoom = state.zoom;
  if (state.fit) {
    const need = sides.length * c.width + (sides.length - 1) * 22;
    zoom = Math.min(1, avail / need);
    state.zoom = zoom;
  }
  $('zoom-label').textContent = Math.round(zoom * 100) + '%';
  for (const s of sides) {
    const div = document.createElement('div');
    div.className = 'side';
    const empty = s === 'back' && !c.double_sided && c.back_elements === 0;
    div.innerHTML = `<div class="label">${s === 'front' ? 'Front' : 'Back'}${empty ? ' (empty, card is single-sided)' : ''}</div>`;
    const img = new Image();
    img.src = c[s];
    img.width = Math.round(c.width * zoom);
    img.alt = s;
    div.appendChild(img);
    stage.appendChild(div);
  }
  const w = $('warnings');
  if (c.warnings.length) {
    w.classList.remove('hidden');
    w.innerHTML = `<details><summary>${c.warnings.length} warning${c.warnings.length > 1 ? 's' : ''}: some elements were skipped or approximated</summary><ul>${c.warnings.map((x) => `<li>${esc(x)}</li>`).join('')}</ul></details>`;
  } else {
    w.classList.add('hidden');
  }
  updatePrintButtons();
  const bothOpt = $('sides').querySelector('option[value="both"]');
  if (!c.double_sided) { $('sides').value = 'front'; } else if ($('sides').value === 'front' && !state.userSides) { $('sides').value = 'both'; }
  bothOpt.textContent = c.double_sided ? 'Both (front + back)' : 'Both (card is not marked double-sided)';
}

function setView(v) {
  state.view = v;
  document.querySelectorAll('#view-seg button').forEach((b) => b.classList.toggle('on', b.dataset.view === v));
  render();
}
function setZoom(z) { state.fit = false; state.zoom = Math.max(0.2, Math.min(3, z)); render(); }

async function loadCard(promise) {
  try {
    setStatus('Opening…');
    const c = await promise;
    state.card = c;
    state.fit = true;
    state.userSides = false;
    setupColour(c);
    render();
    setStatus(`Opened ${c.name}`, 'ok');
  } catch (e) {
    setStatus(String(e), 'error');
  }
}
const openPath = (p) => loadCard(invoke('open_card', { path: p }));

async function chooseFile() {
  const p = await T.dialog.open({ multiple: false, filters: [{ name: 'Card design', extensions: ['card'] }] });
  if (p) openPath(Array.isArray(p) ? p[0] : p);
}

// ---- colour correction ------------------------------------------------------------------------

const SLIDERS = ['brightness', 'contrast', 'saturation', 'red', 'green', 'blue'];
const SLIDER_DEFAULTS = { brightness: 0, contrast: 0, saturation: 0, red: 1, green: 1, blue: 1 };
let ccTimer = null;
let ccBusy = false;
let ccPending = null;

function currentAdjustments() {
  const a = { correction: $('cc-correction').value };
  for (const k of SLIDERS) a[k] = parseFloat($('cc-' + k).value);
  return a;
}

function fmt(k, v) {
  return k === 'red' || k === 'green' || k === 'blue' ? Number(v).toFixed(2) : Number(v).toFixed(k === 'brightness' ? 2 : 1);
}

function showAdjustments(a) {
  $('cc-correction').value = a.correction;
  for (const k of SLIDERS) { $('cc-' + k).value = a[k]; $('cc-' + k + '-v').textContent = fmt(k, a[k]); }
}

function setupColour(c) {
  const sel = $('cc-correction');
  sel.innerHTML = c.corrections.map((o) => `<option value="${esc(o.id)}">${esc(o.label)}</option>`).join('');
  showAdjustments(c.adjustments);
  $('colour-box').classList.toggle('disabled', !c);
  setCcState('', '');
}

function setCcState(text, kind) {
  const el = $('cc-state');
  el.className = 'note save-state' + (kind ? ' ' + kind : '');
  el.textContent = text;
  if (kind === 'ok') {
    el.classList.remove('flash');
    void el.offsetWidth;
    el.classList.add('flash');
  }
}

function savedLabel() {
  const time = new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', second: '2-digit' });
  return `Saved to file · ${time}`;
}

function applyPreviews(p) {
  state.card.front = p.front;
  state.card.back = p.back;
  document.querySelectorAll('#stage img').forEach((img) => { img.src = img.alt === 'back' ? p.back : p.front; });
}

async function pushAdjustments(save) {
  // one request at a time; the latest settings win
  if (ccBusy) { ccPending = { save: save || (ccPending && ccPending.save) }; return; }
  ccBusy = true;
  try {
    const r = await invoke('set_adjustments', { adj: currentAdjustments(), save: !!save });
    applyPreviews(r.previews);
    if (save && r.saved) setCcState(savedLabel(), 'ok');
    else if (save) setCcState('Could not save', 'dirty');
  } catch (e) {
    setStatus(String(e), 'error');
    if (save) setCcState('Could not save', 'dirty');
  } finally {
    ccBusy = false;
    if (ccPending) { const p = ccPending; ccPending = null; pushAdjustments(p.save); }
  }
}

function onAdjust(commit) {
  for (const k of SLIDERS) $('cc-' + k + '-v').textContent = fmt(k, $('cc-' + k).value);
  const save = commit && !!(state.card && state.card.can_save);
  if (save) setCcState('Saving…', '');
  if (!commit) { clearTimeout(ccTimer); ccTimer = setTimeout(() => pushAdjustments(false), 30); return; }
  clearTimeout(ccTimer);
  pushAdjustments(save);
}

// ---- printing ---------------------------------------------------------------------------------

function jobOpts() {
  return {
    printer: $('printer').value || null,
    sides: $('sides').value,
    copies: Math.max(1, parseInt($('copies').value, 10) || 1),
    extra: $('extra').value.split('\n').map((s) => s.trim()).filter(Boolean),
  };
}

async function refreshPrinters() {
  const sel = $('printer');
  const keep = sel.value;
  const r = await invoke('list_printers');
  sel.innerHTML = '';
  if (!r.printers.length) {
    sel.innerHTML = '<option value="">No printers found</option>';
  }
  for (const p of r.printers) {
    const o = document.createElement('option');
    o.value = p.name;
    o.textContent = p.name + (p.enabled ? '' : ' (disabled)');
    sel.appendChild(o);
  }
  const likely = r.printers.find((p) => /magicard|pro ?300|300/i.test(p.name));
  sel.value = keep || (likely && likely.name) || r.default || (r.printers[0] && r.printers[0].name) || '';
  $('printer-note').textContent = r.error ? r.error
    : r.printers.length ? '' : 'No CUPS queue yet. Install the Magicard driver and add the printer (see README).';
  updatePrintButtons();
}

function updatePrintButtons() {
  $('btn-print').disabled = !state.card || !$('printer').value;
}

async function printJob() {
  const o = jobOpts();
  const what = `${state.card.name}: ${o.sides === 'both' ? 'front + back' : o.sides} × ${o.copies} to “${o.printer}”`;
  const ok = await T.dialog.ask(`Send to printer?\n\n${what}\n\nThis uses a card and ribbon panels.`, { title: 'Print', kind: 'warning', okLabel: 'Print', cancelLabel: 'Cancel' });
  if (!ok) return;
  try {
    setStatus('Sending to CUPS…');
    const out = await invoke('print_job', { opts: o });
    setStatus(out || 'Sent to CUPS', 'ok');
  } catch (e) { setStatus(String(e), 'error'); }
}

// ---- wiring -----------------------------------------------------------------------------------

async function drainPending() {
  const list = await invoke('take_pending_open');
  if (list.length) openPath(list[list.length - 1]);
}

window.addEventListener('DOMContentLoaded', async () => {
  $('btn-open').onclick = chooseFile;
  $('btn-cal').onclick = () => loadCard(invoke('open_calibration'));
  document.querySelectorAll('#view-seg button').forEach((b) => (b.onclick = () => setView(b.dataset.view)));
  $('zoom-in').onclick = () => setZoom(state.zoom * 1.25);
  $('zoom-out').onclick = () => setZoom(state.zoom / 1.25);
  $('zoom-fit').onclick = () => { state.fit = true; render(); };
  window.addEventListener('resize', () => state.fit && render());
  $('printer-refresh').onclick = refreshPrinters;
  $('printer').onchange = updatePrintButtons;
  $('sides').onchange = () => { state.userSides = true; };
  $('cc-correction').onchange = () => onAdjust(true);
  for (const k of SLIDERS) {
    $('cc-' + k).oninput = () => onAdjust(false);
    $('cc-' + k).onchange = () => onAdjust(true);
  }
  $('cc-reset').onclick = () => { showAdjustments({ correction: 'Gamma1', ...SLIDER_DEFAULTS }); onAdjust(true); };
  $('colour-box').classList.add('disabled');
  $('btn-print').onclick = printJob;

  // drag and drop (Tauri delivers real file paths)
  const viewer = $('viewer');
  await T.event.listen('tauri://drag-enter', () => viewer.classList.add('drag'));
  await T.event.listen('tauri://drag-leave', () => viewer.classList.remove('drag'));
  await T.event.listen('tauri://drag-drop', (e) => {
    viewer.classList.remove('drag');
    const p = (e.payload.paths || []).find((x) => x.toLowerCase().endsWith('.card'));
    if (p) openPath(p); else setStatus('Drop a .card file', 'error');
  });
  // macOS: double click in Finder / Open With
  await T.event.listen('open-file', drainPending);

  render();
  updatePrintButtons();
  refreshPrinters().catch(() => {});
  drainPending();
});
