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

function syncColourCorrectFromCard() {
  const c = state.card;
  const sel = $('colour-correct');
  const v = (c && c.colour_correction) || 'None';
  if (![...sel.options].some((o) => o.value === v)) {
    const o = document.createElement('option');
    o.value = v;
    o.textContent = v;
    sel.appendChild(o);
  }
  sel.value = v;
  $('colour-note').textContent = c && c.path
    ? 'Saved in this .card file (adjustments.correction), same field Hub uses.'
    : 'Not saved until the card is a real .card file on disk.';
}

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
  syncColourCorrectFromCard();
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
    $('print-preview').classList.add('hidden');
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

async function onColourCorrectChange() {
  const v = $('colour-correct').value;
  if (!state.card) return;
  state.card.colour_correction = v;
  try {
    await invoke('set_colour_correction', { correction: v });
    setStatus(state.card.path ? `Colour correct saved: ${v}` : `Colour correct: ${v}`, 'ok');
  } catch (e) {
    setStatus(String(e), 'error');
  }
}

// ---- printing ---------------------------------------------------------------------------------

function jobOpts() {
  return {
    printer: $('printer').value || null,
    sides: $('sides').value,
    copies: Math.max(1, parseInt($('copies').value, 10) || 1),
    overcoat_front: $('oc-front').checked,
    overcoat_back: $('oc-back').checked,
    colour_correction: $('colour-correct').value || null,
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
    : r.printers.length ? '' : 'No CUPS queue yet. Install the Magicard driver and add the printer (see README). You can still save jobs.';
  updatePrintButtons();
}

function updatePrintButtons() {
  $('btn-print').disabled = !state.card || !$('printer').value;
  $('btn-save').disabled = !state.card;
  $('btn-preview').disabled = !state.card;
}

function showProblems(r) {
  const lines = [...(r.warnings || []), ...(r.problems || [])];
  $('problems').textContent = lines.join('\n');
  if (r.command) { $('cmd-box').classList.remove('hidden'); $('cmd').textContent = r.command; }
}

async function previewJob() {
  try {
    setStatus('Building print pages…');
    const r = await invoke('preview_job', { opts: jobOpts() });
    const box = $('print-preview');
    box.classList.remove('hidden');
    box.innerHTML = `<h3>Print preview: the raster pages sent to the driver (638×1013, portrait)</h3>
      <div class="small" style="color:var(--muted)">These are the pages CardDeck sends to the Magicard CUPS driver.</div>
      <div class="pages">${r.pages.map((p) => `<div><div class="small" style="color:var(--muted)">${esc(p.label)}</div><img src="${p.png}" alt="${esc(p.label)}"></div>`).join('')}</div>`;
    showProblems(r);
    setStatus('Preview ready', 'ok');
    box.scrollIntoView({ behavior: 'smooth', block: 'start' });
  } catch (e) { setStatus(String(e), 'error'); }
}

async function saveJob() {
  try {
    const dir = await T.dialog.open({ directory: true, multiple: false, title: 'Choose a folder for the job files' });
    if (!dir) return;
    setStatus('Saving job…');
    const files = await invoke('save_job', { opts: jobOpts(), dir: Array.isArray(dir) ? dir[0] : dir });
    setStatus('Saved:\n' + files.join('\n'), 'ok');
  } catch (e) { setStatus(String(e), 'error'); }
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
  $('colour-correct').onchange = onColourCorrectChange;
  $('btn-preview').onclick = previewJob;
  $('btn-save').onclick = saveJob;
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
