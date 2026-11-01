// The revision of a string: its length in UTF-16 units and a 32-bit FNV-1a
// digest over those units. Identical to `revision_of` in field.rs; a test pins
// the two to each other.
const rev = (s) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193) >>> 0;
  return s.length + ':' + h.toString(16).padStart(8, '0');
};
