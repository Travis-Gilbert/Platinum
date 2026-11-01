// The text of a contenteditable region, flattened the same way for the probe
// and for the edit: text nodes in document order, a newline for <br> and at
// block boundaries. `parts` maps each text node to its offsets in the flat
// text (UTF-16 units, as the DOM counts them). A newline that is not in a text
// node belongs to no part, so an edit can never straddle one.
const BLOCK = /^(ADDRESS|ARTICLE|ASIDE|BLOCKQUOTE|DD|DIV|DL|DT|FIELDSET|FIGCAPTION|FIGURE|FOOTER|FORM|H[1-6]|HEADER|HR|LI|MAIN|NAV|OL|P|PRE|SECTION|TABLE|TR|UL)$/;
const flat = (root) => {
  const parts = [];
  let text = '';
  const walk = (n) => {
    if (n.nodeType === 3) {
      parts.push({ node: n, start: text.length, end: text.length + n.data.length });
      text += n.data;
      return;
    }
    if (n.nodeType !== 1) return;
    const tag = n.tagName;
    if (tag === 'BR') { text += '\n'; return; }
    if (tag === 'SCRIPT' || tag === 'STYLE') return;
    const block = n !== root && BLOCK.test(tag);
    if (block && text.length && !text.endsWith('\n')) text += '\n';
    for (let c = n.firstChild; c; c = c.nextSibling) walk(c);
    if (block && !text.endsWith('\n')) text += '\n';
  };
  walk(root);
  return { text, parts };
};
