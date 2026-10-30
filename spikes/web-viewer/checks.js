// Synthetic DOM events test the platform bridge, not trusted OS input or IME UI.
const params = new URLSearchParams(location.search);
const stack = params.get('stack') || 'gpui-pre';
const browser = params.get('browser') || 'unknown';
const status = document.querySelector('#status');
const result = {
  schema: 1, stack, browser, userAgent: navigator.userAgent,
  timestamp: new Date().toISOString(), devicePixelRatio,
  viewport: [innerWidth, innerHeight], checks: {}, errors: [], snapshots: {},
  manual: { japaneseCandidateWindow: 'manual', trackpadPinch: 'manual',
    realTabEscape: 'manual', textQuality: 'manual' },
  console: [],
  visibilityAtStart: document.visibilityState,
  url: location.href, phase: 'loading',
};
for (const level of ['error', 'warn', 'log', 'info']) {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    result.console.push({ level, message: args.map(String).join(' ') });
    original(...args);
  };
}
addEventListener('error', e => result.errors.push(e.message));
addEventListener('unhandledrejection', e => result.errors.push(String(e.reason)));
let api, canvas, input;
const frame = () => new Promise(resolve => requestAnimationFrame(resolve));
// GPUI counts clicks by position/time, ignoring synthetic event.detail.
// Separate independent gestures so select/drag does not become double-click edit.
const separateGesture = () => new Promise(resolve => setTimeout(resolve, 550));
async function settle() { await frame(); await frame(); await frame(); }
const state = () => JSON.parse(api.snapshot());
const check = (name, pass, evidence) => {
  result.checks[name] = { status: pass ? 'pass' : 'fail', mode: 'automated', evidence };
};
const key = (key, extra = {}) => {
  for (const type of ['keydown', 'keyup']) {
    input.dispatchEvent(new KeyboardEvent(type, { key, bubbles: true, cancelable: true, ...extra }));
  }
};
function pointer(type, x, y, extra = {}) {
  const event = new PointerEvent(type, { pointerId: 1, pointerType: 'mouse',
    isPrimary: true, bubbles: true, cancelable: true, clientX: x, clientY: y,
    button: 0, buttons: type === 'pointerup' ? 0 : 1, ...extra });
  // Chromium's untrusted PointerEvent offsets were observed at half client
  // coordinates on this DPR=2 display. Supply the intended CSS-local offsets
  // explicitly; trusted pointer coordinate routing remains a manual check.
  if (params.get('rawpointer') !== '1') {
    const rect = canvas.getBoundingClientRect();
    Object.defineProperties(event, {
      offsetX: { value: x - rect.left }, offsetY: { value: y - rect.top },
    });
  }
  canvas.dispatchEvent(event);
}
async function click(x, y, count = 1) {
  await separateGesture();
  for (let n = 1; n <= count; n++) {
    pointer('pointerdown', x, y, { detail: n });
    pointer('pointerup', x, y, { detail: n });
    await settle();
  }
}
function wheel(dx, dy, zoom = false) {
  canvas.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true,
    clientX: innerWidth / 2, clientY: innerHeight / 2, deltaX: dx, deltaY: dy,
    deltaMode: 0, ctrlKey: zoom }));
}
function pointFor(id, dx = 10, dy = 10) {
  const s = state();
  const geom = Object.values(s.elements.find(e => e.id === id).kind)[0];
  // Inverse of the board's Camera::screen_to_world (scene.rs).
  return [(geom.x - s.camera.x) * s.camera.zoom + dx,
    (geom.y - s.camera.y) * s.camera.zoom + dy];
}
async function typeProbe() {
  // Collapse a double-click word selection before probing, so Backspace
  // restores the text instead of deleting the originally selected word.
  key('ArrowRight'); await settle();
  const before = state(); key('q'); await settle();
  const after = state();
  const edited = after.elements.filter((e, i) => JSON.stringify(e) !== JSON.stringify(before.elements[i]));
  if (edited.length) { key('Backspace'); await settle(); }
  return edited.map(e => e.id);
}
async function run() {
  window.focus(); // Only this W0 page; browsers may ignore the request.
  api = await import(`/bundles/${stack}/viewer.js`);
  await api.default();
  result.phase = 'wasm-ready';
  if (params.get('font') !== 'bundled') {
    const response = await fetch('/assets/NotoSansJP.ttf');
    if (!response.ok) throw new Error(`Board font returned ${response.status}`);
    const bytes = new Uint8Array(await response.arrayBuffer());
    api.set_board_font(bytes);
    result.font = { family: 'Noto Sans JP', asset: '/assets/NotoSansJP.ttf', bytes: bytes.length };
  } else result.font = { family: 'JetBrains Mono', bundled: true };
  api.mount();
  result.phase = 'mount-called';
  const deadline = performance.now() + 20000;
  while (!(document.querySelector('canvas') && document.querySelector('textarea') && api.snapshot())) {
    if (performance.now() > deadline) throw new Error('Mount did not expose a board and IME textarea');
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  canvas = document.querySelector('canvas'); input = document.querySelector('textarea');
  result.inputCoordinates = [];
  result.pointerCoordinates = params.get('rawpointer') === '1' ? 'browser-derived synthetic offsets' :
    'explicit CSS-local synthetic offsets; trusted pointer mapping not verified';
  for (const type of ['pointerdown', 'pointermove']) canvas.addEventListener(type, event => {
    if (result.inputCoordinates.length < 25) result.inputCoordinates.push({ type,
      client: [event.clientX, event.clientY], offset: [event.offsetX, event.offsetY],
      canvasRect: [canvas.getBoundingClientRect().x, canvas.getBoundingClientRect().y,
        canvas.getBoundingClientRect().width, canvas.getBoundingClientRect().height] });
  }, { capture: true });
  canvas.setAttribute('aria-hidden', 'true');
  const gl = canvas.getContext('webgl2');
  check('webgl2', !!gl, { version: gl?.getParameter(gl.VERSION), renderer: gl?.getParameter(gl.RENDERER) });
  result.snapshots.initial = state();
  check('200_elements', state().elements.length === 200, state().elements.length);
  result.phase = 'board-mounted';
  if (params.get('manual') === '1') {
    status.textContent = 'Manual mode: double-click Text 2 to edit. H pans; V selects. Reload without manual=1 to run checks.';
    return;
  }
  if (document.visibilityState !== 'visible') throw new Error('Tab must be foreground for frame timings');
  await settle();
  result.phase = 'board-ready';
  // Pan via pointer input and read back the board-owned camera.
  const panStart = state().camera;
  await click(40, 85); key('h'); await settle();
  await separateGesture();
  pointer('pointerdown', 40, 85);
  pointer('pointermove', 70, 100); await settle();
  pointer('pointerup', 70, 100); await settle();
  const panEnd = state().camera;
  check('pointer_pan', panEnd.x !== panStart.x || panEnd.y !== panStart.y, { panStart, panEnd });
  // Wheel pan runs for ten seconds. RAF measures browser cadence, not GPU completion.
  const intervals = [], cameras = [];
  result.phase = 'pan-timing';
  let previous = await frame(), started = previous, n = 0, hidden = false;
  while (previous - started < 10000) {
    wheel(n++ % 120 < 60 ? 1 : -1, 0);
    const now = await frame(); intervals.push(now - previous); previous = now;
    hidden ||= document.visibilityState !== 'visible';
    if (n % 60 === 0) cameras.push(state().camera);
  }
  const sorted = [...intervals].sort((a, b) => a - b);
  const percentile = p => sorted[Math.max(0, Math.ceil(sorted.length * p) - 1)];
  result.frameTimes = { medianMs: percentile(0.5), p95Ms: percentile(0.95),
    durationMs: previous - started, sampleCount: intervals.length, intervalsMs: intervals,
    hidden, metric: 'requestAnimationFrame interval; not GPU presentation latency' };
  check('ten_second_pan', !hidden && new Set(cameras.map(JSON.stringify)).size > 1,
    { cameras, ...result.frameTimes, intervalsMs: undefined });
  const zoomBefore = state().camera.zoom;
  result.phase = 'interaction-checks';
  wheel(0, -40, true); await settle();
  check('zoom', state().camera.zoom !== zoomBefore, { before: zoomBefore, after: state().camera.zoom });
  key('v'); await settle();
  await click(...pointFor(1, 30, 30)); key('Backspace'); await settle();
  check('select', !state().elements.some(e => e.id === 1),
    { method: 'click rectangle, Backspace removes selected id 1', count: state().elements.length });
  key('z', { metaKey: true }); await settle();
  if (!state().elements.some(e => e.id === 1)) throw new Error('Undo did not restore selection fixture');
  const beforeDrag = state().elements.find(e => e.id === 1);
  const [x, y] = pointFor(1, 30, 30);
  await separateGesture();
  pointer('pointerdown', x, y);
  for (let i = 1; i <= 6; i++) { pointer('pointermove', x + i * 5, y + i * 3); await frame(); }
  pointer('pointerup', x + 30, y + 18); await settle();
  const afterDrag = state().elements.find(e => e.id === 1);
  check('drag', beforeDrag.kind.rect.x !== afterDrag.kind.rect.x,
    { before: beforeDrag, after: afterDrag });
  await click(...pointFor(3), 2);
  const clickEdited = await typeProbe();
  check('click_into_text', clickEdited.includes(3), { editedIds: clickEdited });
  const textBefore = state().elements.find(e => e.id === 3).kind.text.content;
  for (const char of 'abc') key(char);
  await settle();
  const textAfter = state().elements.find(e => e.id === 3).kind.text.content;
  check('typing', textAfter.includes('abc') && textAfter.length === textBefore.length + 3, { textBefore, textAfter });
  input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true, data: '' }));
  for (const data of ['に', '日本語']) {
    input.dispatchEvent(new CompositionEvent('compositionupdate', { bubbles: true, data }));
    await settle();
  }
  const composing = state();
  input.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '日本語' }));
  await settle();
  const composed = state().elements.find(e => e.id === 3).kind.text.content;
  check('synthetic_composition', composed.includes('日本語') && composed.length === textAfter.length + 3,
    { composing, composed, candidateWindowProven: false });
  // Read immediately after wgpu presents to the default framebuffer. Reading
  // on a later RAF sees a cleared canvas when preserveDrawingBuffer is false.
  let captured = false;
  const originals = new Map();
  for (const method of ['drawArrays', 'blitFramebuffer']) {
    const original = gl[method];
    originals.set(method, original);
    gl[method] = function (...args) {
      const returned = original.apply(this, args);
      if (!captured && this.getParameter(this.DRAW_FRAMEBUFFER_BINDING) === null) {
        captured = true;
        try {
          result.capture = { stage: 'after-japanese-composition',
            method: 'canvas.toDataURL immediately after WebGL presentation',
            width: canvas.width, height: canvas.height,
            dataUrl: canvas.toDataURL('image/png') };
        } catch (error) { result.capture = { error: String(error) }; }
      }
      return returned;
    };
  }
  wheel(0, 0); // Request a repaint without changing camera geometry.
  await settle();
  for (const [method, original] of originals) gl[method] = original;
  if (!captured) result.capture = { error: 'No presentation observed during capture' };
  key('Escape'); await settle();
  const escapeEdits = await typeProbe();
  check('synthetic_escape_to_board', escapeEdits.length === 0, { editedIds: escapeEdits });
  // Native board has no public editing state. Verify focus by actual text mutations.
  await click(...pointFor(3), 2);
  const clickReentry = await typeProbe();
  await click(40, 85);
  const boardEdits = await typeProbe();
  check('click_back_to_board', clickReentry.includes(3) && boardEdits.length === 0,
    { entryEditedIds: clickReentry, editedIds: boardEdits });
  key('Tab'); await settle();
  const tabEdits = await typeProbe();
  check('synthetic_tab_into_text', tabEdits.length > 0, { editedIds: tabEdits });
  await click(...pointFor(3), 2);
  const tabReentry = await typeProbe();
  key('Tab'); await settle();
  const tabOutEdits = await typeProbe();
  check('synthetic_tab_back_to_board', tabReentry.includes(3) && tabOutEdits.length === 0,
    { entryEditedIds: tabReentry, editedIds: tabOutEdits });
  result.snapshots.final = state();
  result.phase = 'complete';

}
let watchdog;
try {
  await Promise.race([run(), new Promise((_, reject) => {
    watchdog = setTimeout(() => reject(new Error('Automated run exceeded 45 seconds')), 45000);
  })]);
}
catch (error) { result.errors.push(String(error)); result.runStatus = 'blocked_or_failed'; }
finally { clearTimeout(watchdog); }
if (params.get('manual') !== '1') {
  result.runStatus ??= Object.values(result.checks).every(c => c.status === 'pass') && !result.errors.length ? 'pass' : 'fail';
  try {
    const response = await fetch('/result', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(result) });
    if (!response.ok) throw new Error(`Evidence server returned ${response.status}`);
    status.textContent = `${stack}: ${result.runStatus}. Evidence saved. Close this test tab when finished.`;
    if (params.get('autoclose') === '1') {
      const lifecycle = event => navigator.sendBeacon('/lifecycle', JSON.stringify({
        stack, browser, event, timestamp: new Date().toISOString(), url: location.href,
      }));
      addEventListener('pagehide', () => lifecycle('pagehide-after-close-attempt'), { once: true });
      setTimeout(async () => {
        await fetch('/lifecycle', { method: 'POST', body: JSON.stringify({
          stack, browser, event: 'close-attempt', url: location.href,
        }) });
        window.close();
        setTimeout(() => lifecycle('script-close-refused'), 500);
      }, 500);
    }
  } catch (error) { status.textContent = `Evidence POST failed: ${error}`; }
}
