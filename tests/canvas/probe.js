// Only local fixtures use this probe. A fresh document gets a fresh identity.
(() => {
  const documentId = String(Date.now()) + ':' + String(Math.random());
  const events = [];
  const trace = [];
  const viewport = () => ({innerWidth, innerHeight, devicePixelRatio});
  let recording = false;
  const record = kind => {
    const entry = {kind, at: performance.now(), ...viewport()};
    if (recording) trace.push(entry);
    return entry;
  };
  const snapshot = () => {
    const textarea = document.querySelector('textarea');
    return {
      documentId, ...viewport(), resizeCount: events.length, resizeEvents: events.slice(),
      scrollX, scrollY, cookie: document.cookie,
      text: textarea?.value ?? null,
      selectionStart: textarea?.selectionStart ?? null,
      selectionEnd: textarea?.selectionEnd ?? null,
      selectionDirection: textarea?.selectionDirection ?? null,
      textareaScrollTop: textarea?.scrollTop ?? null,
      focused: document.activeElement?.id,
      input: window.canvasInput ?? null,
    };
  };
  addEventListener('resize', () => {
    events.push(record('resize'));
    const output = document.querySelector('#output');
    if (output) output.textContent = JSON.stringify(snapshot(), null, 2);
  });
  // Start after initial placement. Keep every event and animation-frame sample
  // through the zoom session, including frames between the gate's snapshots.
  const frame = () => {
    if (!recording) return;
    record('frame');
    requestAnimationFrame(frame);
  };
  window.canvasProbe = {
    snapshot,
    startTrace() {
      trace.length = 0;
      recording = true;
      record('baseline');
      requestAnimationFrame(frame);
      return snapshot();
    },
    finishTrace() {
      record('final');
      recording = false;
      return trace.slice();
    },
  };
  const output = document.querySelector('#output');
  if (output) output.textContent = JSON.stringify(snapshot(), null, 2);
})();
