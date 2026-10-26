---
tags: [tagging]
---
# Tags

Nested: #project #project/alpha #project/alpha/beta #project/gamma

Case is ignored: #Mixed-Case and #mixed-case and #MIXED-CASE name one tag.

Valid shapes: #y2026 #2026-plan #a_b #a-b #tag/with/many/levels #_leading-underscore #trailing-dash-

Probes whose validity the reference run settles: #1/2 #a//b #a/ #-dash-first

## Tags in other places #heading-tag

- A list item #list-tag
- [ ] A task #task-tag
> A quote #quote-tag

| Column | Tags |
| ------ | ---- |
| cell   | #table-tag |

> [!note] A callout #callout-tag
> Its body #callout-body-tag

## Things that are not tags

- A number alone: #2026
- A heading marker, as on the lines above, or a hash and a space: # no-tag
- In a URL: https://example.com/page#anchor
- C# and F# are languages
- Escaped: \#escaped
- In code: `#in-code`
- In a link: [[Headings Target#First heading]]
- Inside a word: word#notatag

```
#in-a-fenced-block
```

    #in-an-indented-block
