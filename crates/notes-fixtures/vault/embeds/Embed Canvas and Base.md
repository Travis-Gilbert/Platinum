---
tags: [embeds]
---
# Embed Canvas and Base

A canvas:

![[Board.canvas]]

A base, which shows its first view:

![[Notes.base]]

A base, with a named view:

![[Projects.base#Board]]

Another base:

![[Tasks.base#By tag]]

A base in a code block:

```base
filters:
  and:
    - file.hasTag("project")
views:
  - type: table
    name: Projects
    order:
      - file.name
      - status
```
