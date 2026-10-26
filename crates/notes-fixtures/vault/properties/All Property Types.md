---
text: Plain text value
list:
  - alpha
  - beta
  - gamma
number: 42
decimal: 3.14
checkbox: true
unchecked: false
date: 2026-10-25
datetime: 2026-10-25T14:30:00
tags:
  - properties
  - types
aliases:
  - Every Property
  - Property Zoo
cssclasses:
  - wide-page
  - zoo
link: "[[Headings Target]]"
links:
  - "[[Link Forms]]"
  - "[[a/Note]]"
markdown_link: "[Headings Target](Headings%20Target.md)"
empty:
null_value: null
multiline: |
  first line
  second line
nested:
  key: value
  inner:
    deep: 1
unicode-key-ñ: ok
"key with spaces": ok
---
# All Property Types

Every property type Obsidian offers (Text, List, Number, Checkbox, Date, Date & time, Tags), the
default `tags`, `aliases` and `cssclasses`, and shapes the Properties view does not edit: nested
objects and multi-line text.

The `types.json` file in `.obsidian/` fixes each name's type, because a type belongs to the
name and not to the note.
