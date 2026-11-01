// Describe the focused editable field, or say why it is not one to check.
//
// The kind is decided from the element's attributes BEFORE its text is read,
// so a password, one-time code or payment field is reported without its value
// ever leaving the page. The reply is a JSON string.
const reply = (o) => JSON.stringify(o);
let e = document.activeElement;
while (e && e.shadowRoot && e.shadowRoot.activeElement) e = e.shadowRoot.activeElement;
if (!e) return reply({ kind: 'none' });

const tag = e.tagName;
const isInput = tag === 'INPUT';
const isArea = tag === 'TEXTAREA';
const isEditable = !isInput && !isArea && e.isContentEditable === true;
if (!isInput && !isArea && !isEditable) return reply({ kind: 'none' });
if (e.disabled || e.readOnly) return reply({ kind: 'other' });

const attr = (name) => (e.getAttribute(name) || '').toLowerCase();
const tokens = attr('autocomplete').split(/\s+/);
const type = isInput ? attr('type') || 'text' : 'text';
if (
  type === 'password' || type === 'hidden' ||
  tokens.some((t) => t === 'one-time-code' || t === 'current-password' || t === 'new-password' || t.startsWith('cc-'))
) return reply({ kind: 'protected' });
if (
  (isInput && type !== 'text') ||
  tokens.some((t) => t === 'username' || t === 'email' || t === 'url' || t.startsWith('tel') || t === 'postal-code') ||
  /^(numeric|decimal|tel|email|url)$/.test(attr('inputmode'))
) return reply({ kind: 'not_prose' });

const stated = (name) => {
  const n = e.closest('[' + name + ']');
  return n ? n.getAttribute(name) : null;
};
const sc = stated('spellcheck');
const spellcheck = sc === null ? null : sc.toLowerCase() === 'false' ? false : true;
const lang = stated('lang') || document.documentElement.lang || null;

let text;
let selection = null;
let editor;
if (isEditable) {
  editor = 'editable';
  const f = flat(e);
  text = f.text;
  const s = window.getSelection();
  if (s && s.rangeCount && e.contains(s.anchorNode)) {
    const at = (container, offset) => {
      if (container.nodeType === 3) {
        const p = f.parts.find((q) => q.node === container);
        return p ? p.start + offset : null;
      }
      const b = document.createRange();
      b.setStart(container, offset);
      b.collapse(true);
      for (const p of f.parts) if (b.comparePoint(p.node, 0) >= 0) return p.start;
      return f.text.length;
    };
    const r = s.getRangeAt(0);
    const a = at(r.startContainer, r.startOffset);
    const z = at(r.endContainer, r.endOffset);
    if (a !== null && z !== null) selection = [a, z];
  }
} else {
  editor = isArea ? 'textarea' : 'input';
  text = e.value;
  if (typeof e.selectionStart === 'number') selection = [e.selectionStart, e.selectionEnd];
}
// Remembered so the edit can insist on the very same element.
Object.defineProperty(window, '__theoremTextCheck', { value: { el: e, editor }, configurable: true });
return reply({ kind: 'field', editor, text, selection, spellcheck, lang });
