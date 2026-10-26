---
tags:
  - home
  - dashboard
aliases:
  - Start
  - Welcome
cssclasses:
  - dashboard
---
# Home

The vault's startup note ("Default file to open"). Every hub below is linked from here, so the
only notes without an incoming link are the ones `COVERAGE.md` names.

## Hubs

- Links: [[Link Forms]], [[Headings Target]], [[Blocks Target]], [[Aliases]], [[Markdown Links]], [[Unresolved Links]], [[Mentions]]
- The same name in two folders: [[a/Note]], [[b/Note]], [[Links From A]], [[Links From B]]
- Embeds: [[Embed Notes]], [[Embed Images]], [[Embed Media]], [[Embed Canvas and Base]], [[Embed Query]]
- Callouts: [[All Callouts]], [[Callout Titles and Folding]], [[Nested Callouts]], [[Custom Callout]]
- Tags: [[Tags]], [[Unicode Tags]], [[Frontmatter Tags]], [[Tags As Text]]
- Properties: [[All Property Types]], [[Aliases and Cssclasses]], [[Link Properties]], [[Type Mismatch]], [[Broken Frontmatter]], [[Empty Frontmatter]], [[Frontmatter Only]]
- Tasks: [[Task Statuses]]
- Syntax: [[Footnotes]], [[Comments and Highlights]], [[Math]], [[Mermaid]], [[Code Blocks]], [[Tables]], [[Raw HTML]], [[Inline Formatting]], [[Folding and Lists]], [[Bidirectional Text]], [[Line Endings]], [[Byte Order Mark]]
- Review cases: [[Heading With Hash]], [[Missing Note Creation]]
- Search: [[Search Corpus]], [[Case Sensitivity]]
- Other plugins: [[Deck]], [[Dataview Fields]], [[2026-10-23]], [[2026-10-24]], [[2026-10-25]], [[2026-W43]]
- Projects: [[Project Alpha]], [[Project Beta]], [[Project Gamma]], [[Project Delta]]
- Boards: [[Board.canvas]], [[Groups.canvas]], [[Edges.canvas]], [[Cards.canvas]]
- Bases: [[Notes.base]], [[Projects.base]], [[Tasks.base]]
- Names with Unicode: [[Unicode é 日本]]
- A note nothing leads out of: [[Dead End]]
- Excluded from search and the graph by `userIgnoreFilters`: [[Hidden Note]]

## Dashboard

A named view of a base:

![[Notes.base#All notes]]

A `base` code block:

```base
filters:
  and:
    - file.inFolder("tasks")
views:
  - type: list
    name: Task notes
```
