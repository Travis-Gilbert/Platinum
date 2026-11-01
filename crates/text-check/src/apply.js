// Make one correction in the field the probe described, or change nothing.
//
// `d` is {editor, revision, start, end, expected, replacement}. The checks
// below and the edit run in one turn of the page's event loop, so nothing can
// change the field between them. Offsets are UTF-16 units. The reply is a
// JSON string: {ok:true, after} or {ok:false, why}.
const fail = (why) => JSON.stringify({ ok: false, why });
const st = window.__theoremTextCheck;
const e = st && st.el;
if (!e || !e.isConnected) return fail('gone');
if (st.editor !== d.editor) return fail('kind');
if (e.disabled || e.readOnly) return fail('readonly');

if (d.editor !== 'editable') {
  const text = e.value;
  if (rev(text) !== d.revision) return fail('stale');
  if (text.slice(d.start, d.end) !== d.expected) return fail('changed');
  e.setRangeText(d.replacement, d.start, d.end, 'end');
  e.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertReplacementText', data: d.replacement }));
  return JSON.stringify({ ok: true, after: rev(e.value) });
}

const f = flat(e);
if (rev(f.text) !== d.revision) return fail('stale');
if (f.text.slice(d.start, d.end) !== d.expected) return fail('changed');
// Every unit of the range must be in a text node: a newline the flattening
// added at a block boundary cannot be replaced.
let covered = 0;
for (const p of f.parts) covered += Math.max(0, Math.min(p.end, d.end) - Math.max(p.start, d.start));
if (covered !== d.end - d.start) return fail('range');
const locate = (offset, closing) => {
  for (const p of f.parts) {
    if (closing ? p.start < offset && offset <= p.end : p.start <= offset && offset < p.end) {
      return { node: p.node, offset: offset - p.start };
    }
  }
  return null;
};
const from = locate(d.start, false);
const to = locate(d.end, true);
if (!from || !to) return fail('range');
// The range is edited in place in its one text node, so no character outside
// it can change. `execCommand('insertText')` is not used: Servo turns the space
// before the inserted text into a no-break space. A range across two nodes
// (a word split by inline markup) is refused rather than guessed at.
if (from.node !== to.node) return fail('range');
const beforeInput = new InputEvent('beforeinput', {
  bubbles: true, cancelable: true, inputType: 'insertReplacementText', data: d.replacement,
});
// A page that takes over its own editing cancels this and is left alone.
if (!e.dispatchEvent(beforeInput)) return fail('refused');
// Whatever the page did on beforeinput, the text must still be what was checked.
if (rev(flat(e).text) !== d.revision) return fail('stale');
if (!e.contains(from.node)) return fail('gone');
from.node.replaceData(from.offset, to.offset - from.offset, d.replacement);
e.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertReplacementText', data: d.replacement }));
return JSON.stringify({ ok: true, after: rev(flat(e).text) });
