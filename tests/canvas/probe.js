// Only local fixtures use this probe. A fresh document gets a fresh identity.
(() => {
  const documentId = String(Date.now()) + ':' + String(Math.random());
  const events = [];
  const viewport = () => ({innerWidth, innerHeight, devicePixelRatio});
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
    events.push({at: performance.now(), ...viewport()});
    const output = document.querySelector('#output');
    if (output) output.textContent = JSON.stringify(snapshot(), null, 2);
  });
  window.canvasProbe = {snapshot};
  const output = document.querySelector('#output');
  if (output) output.textContent = JSON.stringify(snapshot(), null, 2);
})();
