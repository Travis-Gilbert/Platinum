---
tags: [dataview]
---
# Dataview Fields

Inline fields:

status:: active
rating:: 4

A bracketed field [mood:: calm] and a parenthesised one (due:: 2026-11-01) in a sentence.

An inline query: `= this.status`

```dataview
TABLE status, rating
FROM #project
SORT file.name ASC
```

```dataview
LIST
FROM "callouts"
```

```dataview
TASK
WHERE !completed
```

```dataview
CALENDAR file.cday
```
