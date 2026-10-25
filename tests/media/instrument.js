// Test-only instrumentation, installed before page scripts. Never records URLs,
// cookies, request headers, or appended media bytes.
(() => {
  const trace = window.__frameMediaTrace = [];
  const sources = window.__frameMediaSources = [];
  const record = data => { if (trace.length < 300) trace.push(data); };
  const ranges = buffer => {
    try { return Array.from({length: buffer.buffered.length}, (_, i) => [buffer.buffered.start(i), buffer.buffered.end(i)]); }
    catch (e) { return String(e); }
  };
  addEventListener("error", event => record({error: String(event.message)}));
  addEventListener("unhandledrejection", event => record({rejection: String(event.reason)}));
  if (typeof MediaSource === "undefined") return;
  const create = URL.createObjectURL;
  URL.createObjectURL = function(object) {
    if (object instanceof MediaSource) {
      sources.push(object);
      for (const event of ["sourceopen", "sourceended", "sourceclose"])
        object.addEventListener(event, () => record({event, state: object.readyState}));
    }
    return create.call(this, object);
  };
  const add = MediaSource.prototype.addSourceBuffer;
  MediaSource.prototype.addSourceBuffer = function(type) {
    record({call: "addSourceBuffer", type});
    try {
      const buffer = add.call(this, type);
      for (const event of ["updatestart", "updateend", "error", "abort"])
        buffer.addEventListener(event, () => record({type, event, updating: buffer.updating, buffered: ranges(buffer)}));
      return buffer;
    } catch (e) { record({call: "addSourceBuffer", error: String(e)}); throw e; }
  };
  for (const method of ["appendBuffer", "remove", "abort", "changeType"]) {
    const original = SourceBuffer.prototype[method];
    SourceBuffer.prototype[method] = function(...args) {
      record({call: method, args: method === "appendBuffer" ? [args[0].byteLength] : args});
      try { return original.apply(this, args); }
      catch (e) { record({call: method, error: String(e)}); throw e; }
    };
  }
})();
