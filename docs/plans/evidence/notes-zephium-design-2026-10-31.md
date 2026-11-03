# Zephium design reference for Platinum notes

Scope: the notes reading page (Servo, `assets/notes/reading.css`) and the
notes editor tab and chrome (GPUI). Source: Zephium
(https://github.com/zephium-browser/Zephium at commit
96257cd3f67dd639eb5decb85b135f2674444603, 2026-10-30; Tauri + Svelte 5 +
TipTap), cloned as `zephium-ref/src`. All Zephium paths below are relative to
`zephium-ref/src/frame/src/`. Dark values are the base; light values
are given where they differ.

Colours marked "composite" are my own alpha-composites of a Zephium rgba token
over the page colour: flat hex for GPUI or places that cannot take alpha. They
are not literal Zephium values.

## 1. Licence note (read first)

Zephium is MPL-2.0 (`zephium-ref/src/LICENSE`). MPL is file-level copyleft: a
Platinum file that contains copied Zephium source stays MPL-2.0, keeps the
notice, and its changes must be published under MPL-2.0. Colour, size and
spacing numbers are design facts, so using them in new Platinum code is
re-expression. This is not legal advice.

- "re-express": write your own CSS or Rust from the value. Default for all below.
- "verbatim (MPL file)": copy source text. I recommend it for nothing; Platinum
  has no Tailwind or Svelte, so no Zephium file drops in unchanged.
- Do not paste rule blocks from `editor.css` or `NotesPage.svelte`; write the
  rules from the numbers. Write your own UI copy (strings in 5.7 are reference).
- The one place a verbatim copy would help is the `linear()` spring easing in
  `tokens.css`; copying it makes that stylesheet MPL. Otherwise use
  `cubic-bezier(0.22,1,0.36,1)`. Do not copy Svelte structure or icons.

## 2. How Zephium thinks

- Tonal, no accent hue: state is luminance, and the brightest rung ("lit",
  `#ededf0` dark, `#222226` light) is the accent (`styles/tokens.css:136-147`).
  Greys are cool (hue 240, saturation 2 to 5%).
- Hover and selected are white overlays at 4 and 7.5 percent (ink in light).
  Radius is named by role (8, 12, 18, 22, capsule). Rhythm 8px with 4px steps.
  Only transform and opacity animate, 70 to 400 ms, `cubic-bezier(0.22,1,0.36,1)`.
- Reading text is small and tight: 15px at 1.62, a 664px column, negative
  letter-spacing on every heading.

## 3. Token table

Platinum "now" comes from `reading.css` (reading panel worktree), the theme JSON
(`themes/asciinema.json` dark, `themes/github-sharp.json` light) or `src/`.
"none" means Platinum has no equivalent.

### 3.1 Surfaces (dark)

| Role | Zephium | Source | Platinum now |
|---|---|---|---|
| page / canvas | `#1a1a1d` hsla(240,5%,10.8%) | tokens.css:14,26 | reading `#1e1e1e`; GPUI `#121314` |
| chrome | `#1f1f22` | tokens.css:15 | reading alt `#262626`; GPUI panel `#181919` |
| surface | `#26262a` | tokens.css:16 | none |
| float (menus, popovers) | `#3d3d44` | tokens.css:18 | GPUI popover (theme) |
| fill (code block plate) | white 5% = `#252528` composite | tokens.css:51 | reading `#262626` |
| fill-hover | white 8% = `#2c2c2f` | tokens.css:52 | reading hover white 7% |
| border | white 7% = `#2a2a2d` | tokens.css:72 | reading `#363636`; GPUI `#303030` |
| border-strong | white 13% = `#38383a` | tokens.css:73 | reading hover `#3f3f3f` |
| row-hover | white 4% = `#232326` | tokens.css:78 | GPUI tab bar white 5% |
| row-active | white 7.5% = `#2b2b2e` | tokens.css:79 | GPUI secondary.active `#1f2020` |
| row-rim | `inset 0 0 0 1px` white 6% | tokens.css:81 | none |
| scroll thumb | white 16%, hover 28% | tokens.css:93-94 | GPUI thumb `#2a2a2a` |

### 3.2 Text and accent (dark)

| Role | Zephium | Source | Platinum now |
|---|---|---|---|
| text | `#f4f4f6` | tokens.css:96 | reading `#dadada`; GPUI `#cccccc` |
| label-secondary | `#d8d8db` | tokens.css:97 | none |
| muted | rgb(244 244 246 / .68) = `#aeaeb1` | tokens.css:98 | reading `#b3b3b3`; GPUI `#6d6d6d` |
| faint | rgb(244 244 246 / .44) = `#7a7a7c` | tokens.css:99 | reading `#666666` |
| lit (accent) | `#ededf0`; hover `#f7f7fa`; on-lit `#161619` | tokens.css:136-139 | reading purple hsl(258 88% 66%); GPUI primary `#26b0d7` |

Contrast on the page (computed): text 15.8:1, label-secondary 12.2, muted 7.9,
faint 4.05. Faint passes only for large text and placeholders.

### 3.3 Surfaces and text (light)

Light overrides, `tokens.css:616-788` (full light palette also in 4.1):

| Role | Zephium |
|---|---|
| page / chrome / surface, raised, float | `#f1f1f3` / `#e6e6e9` / `#ffffff` |
| fill / fill-hover | `#e6e6e8` / `#e0e0e2` composite |
| border / border-strong | ink 7% = `#e2e2e4` / ink 13% = `#d5d5d7` |
| row-hover / row-active | ink 4% = `#e8e8ea` / ink 7.5% = `#e1e1e3` |
| text / label-secondary | `#1c1c1f` / `#55555c` |
| muted | rgb(28 28 31 / .62) = `#6d6d70` (4.57:1) |
| faint | rgb(28 28 31 / .42) = `#98989a` (2.55:1, fails body text) |
| lit / on-lit | `#222226` / `#ffffff` |

Platinum light now: reading `#fff`/`#222`/`#5c5c5c`/`#ababab`; GPUI
`#f9f9f9`/`#24292e`/muted `#586069` (6.06:1, better than Zephium's muted).

### 3.4 Soft hues (callouts, tags, alerts)

Zephium has six "soft" hues with one recipe: wash = hue at 15% (dark) or 13%
(light) over the page, edge = hue at 26% (dark) or 30% (light), ink = a lighter
mix of hue and text (`tokens.css:180-188`). Dark hue / ink / wash-on-page:

| Hue | Hue (dark) | Ink (dark) | Wash (dark) | Hue (light) | Ink (light) | Wash (light) |
|---|---|---|---|---|---|---|
| sky | `#8db9f2` | `#b4d0f5` | `#2a2f38` | `#4f8fe0` | `#4474b1` | `#dce5f2` |
| mint | `#88d0b3` | `#b3decc` | `#293131` | `#2f9c77` | `#307d62` | `#dae6e2` |
| lemon | `#e6cb80` | `#ebdbaf` | `#34312b` | `#b88c14` | `#927224` | `#eae4da` |
| peach | `#efaa89` | `#f3c6b3` | `#352d2c` | `#d2703f` | `#a65d3a` | `#efe0dc` |
| rose | `#ee9aae` | `#f3bcc9` | `#352b30` | `#d35d7a` | `#a74f64` | `#f0dee2` |
| lilac | `#b7a6f0` | `#cdc4f3` | `#2e2c38` | `#7e67d6` | `#6656aa` | `#e1dff1` |


### 3.5 Code colours

Mixes from `tokens.css:165-173`; the hex are my composites.

| Token | Mix | Dark | Light |
|---|---|---|---|
| keyword | rose 78% into text | `#f2a6aa` | `#a74950` |
| string | sage 80% | `#9dd2b4` | `#2f7251` |
| number | chart-4 72% | `#ba9e5e` | `#614b1c` |
| function | sky 78% | `#99c2f6` | `#366ab6` |
| type | chart-7 74% | `#9b8bbf` | `#605386` |
| attribute | chart-2 70% | `#c8977b` | `#7a4324` |
| property | label-secondary | `#d8d8db` | `#55555c` |
| comment | muted | `#aeaeb1` | `#6d6d70` |
| inline code ink | text 84% + rose 16% (`editor.css:16-20`) | `#f5e4e6` | `#372629` |

### 3.6 Shape, size, motion

| Token | Zephium | Source |
|---|---|---|
| radii | 3 swatch, 8 inset, 10 control-compact, 12 row, 14 control, 18 card, 22 panel | tokens.css:341-349 |
| shadow-float | `0 6px 22px rgb(0 0 0/.34), 0 0 0 .5px rgb(255 255 255/.06)` (dark outline .08) | tokens.css:392-420 |
| shadow-menu | `0 0 0 1px rgb(255 255 255/.08), 0 14px 44px -10px rgb(0 0 0/.34)` | tokens.css:392-420 |
| motion | 70 / 120 / 200 / 300 / 400 ms | tokens.css:501-513 |
| easings | out `(0.22,1,0.36,1)`; smooth `(0.2,0.8,0.2,1)`; emphasized `(0.16,1,0.3,1)`; spring is a `linear()` sample with 2% overshoot | tokens.css |
| font mono | `ui-monospace, "SF Mono", "Cascadia Mono", "Menlo", "Consolas", monospace` (sans stack already matches reading.css) | tokens.css:433-436 |

Platinum now: no radius scale (theme radius 6, radius_lg 8, GPUI Kit defaults),
no motion tokens, reading radii 4px (callout) and 5px (checkbox).

## 4. Reading page: changes to `reading.css`

Zephium has no HTML reading mode; its notes are a live editor styled by
`features/notes/components/editor/editor.css`. Values below map that look onto
Obsidian names so vault snippets keep working. Line refs are to `editor.css`.

### 4.1 Palette (set from the host if possible)

Platinum's two surfaces disagree (reading `#1e1e1e`, GPUI `#121314`). Best fix:
the host injects these variables from the active theme JSON, with the Zephium
values as fallback. Dark:

```
--background-primary:            #1a1a1d;
--background-primary-alt:        #252528;   /* fill: code plate, table header wash */
--background-secondary:          #1f1f22;
--background-secondary-alt:      #26262a;
--background-modifier-border:    #2a2a2d;
--background-modifier-border-hover: #38383a;
--background-modifier-hover:     rgba(255,255,255,.08);
--text-normal:  #f4f4f6;
--text-muted:   rgba(244,244,246,.68);
--text-faint:   rgba(244,244,246,.44);
--text-selection: rgba(244,244,246,.22);
--shadow-l: 0 6px 22px rgba(0,0,0,.34), 0 0 0 .5px rgba(255,255,255,.08);
```

Light: `--background-primary #f1f1f3`, alt `#e6e6e8`, secondary `#e6e6e9`,
border `rgba(24,24,27,.07)`, border-hover `rgba(24,24,27,.13)`,
modifier-hover `rgba(24,24,27,.075)`, text-normal `#1c1c1f`, muted
`rgba(28,28,31,.62)`, faint `rgba(28,28,31,.42)`, selection
`rgba(28,28,31,.22)`. Do not use the light faint for body text.

Accent: decision for the owner. Zephium is neutral (`--interactive-accent`
`#ededf0` dark, `#222226` light, on-accent `#161619` / `#fff`). Platinum's chrome
is cyan (`#26b0d7`) and the reading page is Obsidian purple, so the split shows
three accents. Recommendation: have the host inject `--accent-h/s/l` from the
chrome primary, and keep link text neutral (4.3) so the accent appears only on
checks, selection and wiki-link underlines.

### 4.2 Measure, type, rhythm

| Obsidian variable | Value | Zephium source | Now |
|---|---|---|---|
| `--file-line-width` | `664px` | NotesPage.svelte:529-538 (760 max less 2x48) | 700px |
| `--file-margins` | `48px`; `24px` when the panel is narrow | NotesPage.svelte:529-538 | 32px |
| `--font-text-size` | `15px` | editor.css:7 | 16px |
| `--line-height-normal` | `1.62` | editor.css:8 | 1.5 |
| `--p-spacing` | `0.7em` between blocks (apply as `> * + *` margin-block-start) | editor.css:62 | 1rem |
| `--list-indent` | `1.55em` | editor.css:181 | 2em |
| `--list-spacing` | `0.2em` between items; `0.35em` inside multi-block items | editor.css:195-210 | 0.075em |
| `--bold-weight` | `650` (fall back to 600 if Servo snaps it) | editor.css:125 | 600 |

Also set on the reading root: `font-kerning: normal; font-optical-sizing: auto;
text-rendering: optimizeLegibility; font-feature-settings: "kern","calt";
overflow-wrap: anywhere` (global.css:54-67, editor.css:45-62). Keep the 40vh
bottom padding. Top padding: 20px under Zephium's 48px bar; with no bar keep 32px.

### 4.3 Headings, title, links

At 15px base (editor.css:68-122). Set `--hN-size`, `--hN-weight`,
`--hN-line-height`; letter-spacing has no Obsidian variable, so add rules.

| Heading | Size | Weight | Line height | Letter-spacing |
|---|---|---|---|---|
| h1 | `1.867em` (28px) | 700 | 1.2 | `-0.028em` |
| h2 | `1.333em` (20px) | 650 | 1.3 | `-0.02em` |
| h3 | `1.1em` (16.5px) | 650 | 1.35 | `-0.012em` |
| h4 to h6 | `1em` | 650 | body | `-0.012em` |

Colours: h1 to h4 `var(--text-normal)`; h5 and h6 label-secondary (`#d8d8db`
dark, `#55555c` light).

- Heading margins: `1.35em` above h1 and h2, `1.1em` above h3 and h4, drop the
  current `0.5em` side margins. First h1 gets `0.35em` below.
- `.inline-title` (note title): 28px, weight 700, line-height 1.2,
  letter-spacing `-0.028em`, margin-bottom `0.35em`.
- Links: `--link-color` and hover are `var(--text-normal)`; underline 1px,
  `text-decoration-color: var(--background-modifier-border-hover)`,
  `text-underline-offset: .18em`; hover colours the underline to text; 120ms
  ease-out (editor.css:133-145).
- Internal links (`.internal-link`): weight 550, `text-decoration-thickness:
  2px`, underline colour accent at 16%, offset `.2em` (editor.css:365-392).
- Unresolved (`.is-unresolved`): colour muted, weight 450, dashed 1px
  `border-hover` underline, opacity 1. Replaces the current opacity .6 solid.
- `strong` 650. `del` / `s` colour muted.

### 4.4 Blocks

| Element | Variables / rules | Source |
|---|---|---|
| Blockquote | `--blockquote-border-thickness: 3px`; `--blockquote-border-color: #38383a` (light `#d5d5d7`); `--blockquote-color: #d8d8db` (light `#55555c`); padding-inline-start `.95em` | editor.css:282-287 |
| hr | `--hr-thickness: 1px`; `--hr-color: var(--background-modifier-border)`; margin-block `1.4em` | editor.css:353-358 |
| Code block (`pre`) | padding `.95em 1.1em`; radius `10px`; background `--background-primary-alt`; `box-shadow: inset 0 0 0 1px var(--background-modifier-border)`; colour label-secondary; font `.84em` mono; line-height 1.6; tab-size 2; inner `code` weight 400 | editor.css:289-312 |
| Inline code | `--code-size: .85em`; padding `.12em .38em .1em`; radius `.36em`; background `--background-modifier-hover`; same 1px ring; colour `#f5e4e6` dark / `#372629` light; weight 500; `box-decoration-break: clone`. Needs a `:not(pre) > code` rule because Obsidian's `--code-background` covers both | editor.css:147-159 |
| Task check | `--checkbox-radius: 50%`; `--checkbox-size: 1.05em`; border 1.5px `border-hover`, hover muted; checked fills accent with on-accent tick; `--checklist-done-decoration: none`; `--checklist-done-color: var(--text-muted)`; active scale .88; 200ms spring | editor.css:225-280 |
| Lists | marker colour muted; numerals tabular; disc, circle, decimal markers | editor.css:181-214 |
| Tables | horizontal rules only (1px bottom border `--background-modifier-border`, no vertical lines); width 100%; `.93em`; cell padding `.45em .7em`; header weight 600, colour label-secondary; tabular numerals. Replaces full grid and `4px 8px` padding | editor.css:421-440 |

Code token colours (`--code-*`): comment `#aeaeb1`, function `#99c2f6`,
keyword `#f2a6aa`, string `#9dd2b4`, value (numbers) `#ba9e5e`, property
`#d8d8db`, tag `#9b8bbf` (my mapping: Zephium's type colour), important
`#c8977b` (my mapping: attribute), operator and punctuation `#aeaeb1` (my
mapping). Light values in 3.5.

### 4.5 Callouts, tags, properties (not in Zephium: derived)

Zephium has none of these (section 7). These reuse its soft-hue recipe.

- Callout: `--callout-radius: 10px`; `--callout-border-width: 1px`; padding
  `12px 14px`; background hue at 15% dark, 13% light; border hue at 26% dark,
  30% light; title weight 600, colour = the ink column in 3.4; content colour
  `--text-normal`. Obsidian hard-codes the 10% wash, so reading.css must set
  `background-color: rgba(var(--callout-color), .15)` itself.
- Callout type to hue: note, info, todo = sky; abstract, summary, tldr, tip,
  hint, success, check, done = mint; question, help, faq = peach; warning,
  caution, attention = lemon; failure, fail, missing, danger, error, bug = rose;
  example = lilac; quote, cite = neutral (text rgb 244,244,246 at the same
  alphas). RGB triplets, dark: sky `141,185,242`, mint `136,208,179`, lemon
  `230,203,128`, peach `239,170,137`, rose `238,154,174`, lilac `183,166,240`.
  Light: sky `79,143,224`, mint `47,156,119`, lemon `184,140,20`, peach
  `210,112,63`, rose `211,93,122`, lilac `126,103,214`.
- Tags (`.tag`): Zephium's "term" chip is sky ink on a 12% sky wash
  (`tokens.css:270-273`). Dark: background `#282d37`, colour `#b4d0f5`. Light:
  `#dfe6f1`, `#4474b1`. Capsule radius, weight 500, padding `.1em .55em`,
  `font-size: .9em`.
- Properties panel: rows 32px, padding `5px 8px`, hover
  `--background-modifier-hover`; key muted 12px, value text 13px (ListRow,
  `shared/ui/ListRow/ListRow.svelte:28`; derived).

### 4.6 Scrollbar and focus

- Use standard properties, not `::-webkit-scrollbar` (Servo support unverified):
  `scrollbar-width: thin; scrollbar-color: rgba(255,255,255,.16) transparent`
  (light `rgba(24,24,27,.2)`), `scrollbar-gutter: stable`; hover thumb 28% dark,
  34% light (global.css:157-187).
- Focus: keyboard only, `outline: 2px solid <accent>; outline-offset: 2px`
  (global.css:132-135). Reduce motion: no transitions under
  `prefers-reduced-motion: reduce` (`styles/axes/motion.css`).

## 5. GPUI editor tab and notes chrome

GPUI colours: `hsla(h/360, s, l, a)` takes hue in 0 to 1. Hue below is in
degrees. Hex are composites unless marked.

### 5.1 Editor text column (re-express in the zorite editor host)

| Property | Zephium | Platinum now | Action |
|---|---|---|---|
| outer stage max width | 760px, centred (NotesPage.svelte:529) | n/a | wrapper `max_w(px(760))`, `mx_auto` |
| side padding | 48px; 24px when narrow | n/a | `px(48)` / `px(24)` |
| top padding | 20px under the stage bar | n/a | `px(20)` |
| bottom padding | 40vh (30vh in the panel) | n/a | spacer of 40% window height |
| font size | 15px | 16px default (lib.rs:1117) | pass `px(15)` |
| line height | 1.62 (editor.css:8) | ratio 1.45 hard-coded (lib.rs:215) | patch constant to 1.62 (tracked patch, THEOREM-PATCH.md) |
| heading scale | 28 / 20 / 16.5 / 15 / 15 / 15 = 1.867, 1.333, 1.1, 1, 1, 1 | 1.8, 1.5, 1.3, 1.15, 1.05, 1.0 (markdown_syntax.rs:1667-1672) | H1 already close (27px). H2 to H4 are larger than Zephium; patch is optional |

Weights: Zephium uses 550, 650, 700. GPUI `FontWeight` may accept floats
(unverified); otherwise MEDIUM for 550, SEMIBOLD for 650, BOLD for 700. GPUI
likely has no letter-spacing in `TextStyle` (unverified): skip negative tracking
in the editor and keep it in CSS.

### 5.2 `SyntaxStyle` colours (dark / light)

| Field | Dark | Light | Basis |
|---|---|---|---|
| marker (`#`, `**`) | `#7a7a7c` `hsla(240,1%,48%,1)` | `#98989a` | faint |
| code (inline ink) | `#f5e4e6` `hsla(353,46%,93%,1)` | `#372629` `hsla(349,18%,18%,1)` | 3.5 |
| code_bg | `#2c2c2f` `hsla(240,3%,18%,1)` | `#e0e0e2` | fill-hover |
| link | `#f4f4f6` `hsla(240,10%,96%,1)` (underlined) | `#1c1c1f` | text |
| tag | `#b4d0f5` `hsla(214,76%,83%,1)` | `#4474b1` `hsla(214,44%,48%,1)` | sky ink |
| quote | `#d8d8db` `hsla(240,4%,85%,1)` | `#55555c` | label-secondary |
| rule | `#38383a` `hsla(240,2%,22%,1)` | `#d5d5d7` | border-strong |
| mark_bg (`==x==`) | `#34312b` `hsla(40,9%,19%,1)` | `#eae4da` | lemon wash |
| alert_note / tip / important / warning / caution | sky, mint, lilac, lemon, rose inks and washes from 3.4 | same | soft hues |
| popover_bg | `#3d3d44` `hsla(240,5%,25%,1)` | `#ffffff` | float |

Backgrounds: do not retheme the app. Platinum's GPUI background (`#121314`) is
its own theme; set the reading page to follow it (4.1) rather than moving the
editor to `#1a1a1d`. Zephium's values apply as relationships: chrome sits one
step above page, fill is page + 5% white, hover +4%, selected +7.5%.

### 5.3 Title treatment

Note title above the body: 28px, weight 700, line height 1.2 (33.6px), tracking
`-0.028em` (CSS only), 0.35em (about 10px) below, body text colour, faint
"Title" placeholder. I did not confirm whether Zephium's title is a separate
input or the first h1 block (editor.css:68-122 gives a first h1 that margin).

### 5.4 Stage bar (above the editor)

48px high, three-column grid, 12px side padding (NotesPage.svelte:440-512). A
`0 1px 0` hairline appears only once the body scrolls past 56px. Left status
("Edited ...", 12px faint) crossfades to the title (13px, weight 600, tracking
`-0.01em`) over 200ms with 6px travel. Head buttons 30x30, radius 12, muted;
hover fill-hover.

### 5.5 Tab strip

Zephium has no horizontal tab strip; its tabs are a vertical sidebar
(`features/tabs/components/TabRow.svelte`, `TabList.svelte`). The state recipe
transfers, the geometry does not.

| Property | Zephium | Platinum now | Proposal |
|---|---|---|---|
| row height | 36px (`--row-sidebar`) | TAB_ROW 36 (browser.rs:102) | keep 36 |
| tab width | n/a | 160, min 96 (browser.rs:105-106) | keep |
| radius | 12 | capsule | 12 (not capsule) for notes tabs |
| default text | label-secondary `#d8d8db` | foreground | `#d8d8db` |
| hover | white 4% (`#232326`) | foreground 5% | white 4% |
| selected | white 7.5% (`#2b2b2e`) plus 1px inset rim white 6% | tab.active | white 7.5% + rim |
| font | 13px, 500; selected 550; tracking -0.005em | text_sm | 13px, MEDIUM; selected SEMIBOLD |
| icon / gap | 16px / 10px | 18px tray | 16px / 10px |
| close button | 21px, radius 8, right 5px, hidden until hover, hover bg white 10% | n/a | same |

Light selected: ink 7.5% (`#e1e1e3`), rim ink 6%. Zephium also fades the label
with a 26px mask, presses selected to scale .985 and transitions 120ms; in GPUI
use ellipsis and a 120ms opacity hover (no mask without a gradient quad).

### 5.6 Sidebar rows and note list

| Property | Zephium | Platinum now |
|---|---|---|
| tree row | 36px, gap 4px, radius 12, 13px/500, current 550 | ROW 30, INDENT 14, radius 6 |
| note list row | 58px (62 at page density); padding `1px 0`; button padding `0 36px 0 12px`; radius 12 | none |
| row title | 13.5px/19px, weight 600, tracking -0.008em | n/a |
| row meta | 12px/16px muted; time label-secondary tabular; preview faint | n/a |
| section header | 32px; padding `0 12px 6px`; 12px/600 faint; tracking .01em | none |
| "more" button | 26x26, radius 8, right 8px, hidden until row hover | none |
| column width | `minmax(260px, 320px)`, 8px gap between panes | sidebar 240 (180 to 480) |

Recommendation: keep Platinum's 30px for a dense vault tree, but take
Zephium's row radius (12, or 8 at 30px), state alphas, 10px icon gap and 120ms
hover. Use the 58px two-line row for the recency-sectioned note list (pinned,
today, yesterday, this week, this month, then months; `features/notes/lib/sections.ts`).

### 5.7 Menus, overlays, empty state

- Float menu (slash menu, wiki-link picker): background `#3d3d44`
  `hsla(240,5%,25%,1)`, radius 22, padding 6, shadow-menu (3.6), width 236.
  Item min-height 32, padding `5px 8px`, radius 16, 13px/18px, gap 10, icon 16px
  muted; highlighted row white 7.5%; detail 12px faint; separator 1px white 10%;
  entry 120ms from scale .97 (editor.css:456-582).
- Format bar: 38px high, padding 4, radius 14, float background; buttons 30x30,
  radius 8, pressed white 12%; divider 1x18 border-strong (`FormatBar.svelte:236`).
- Banner: padding `12px 12px 12px 14px`, radius 14, fill (`NoteView.svelte:148`).
  Toast: float background, radius 14, shadow-float, 16px from bottom
  (`NoteNotice.svelte:33`). Backlinks: top border, items 32px radius 8 13px
  (`NoteView.svelte:280`).
- Empty state: icon plate 60x60, radius 18, fill plus 1px inset ring; heading
  17px/650 tracking -0.018em; body 13px/19px muted, max 340; gap 18; 300ms
  entrance rising 6px; primary button 34px, radius 14, accent fill
  (`NotesPage.svelte:574-708`). Reference strings, write your own: "Title",
  "Start writing, or type / for styles", "No notes yet".

## 6. Conflicts and risks

1. Two backgrounds (reading `#1e1e1e`, GPUI `#121314`): inject the theme
   background into `--background-primary` (4.1). Two accents (Obsidian purple,
   chrome cyan): decide once (4.1).
2. `LINE_HEIGHT_RATIO` and the heading scale live in `vendor/zorite-editor`;
   changing them needs a tracked patch (THEOREM-PATCH.md), not an edit to an
   ignored `deps/` copy.
3. Zephium's light faint (2.55:1) fails for text; keep Platinum's light muted
   `#586069`.
4. Weights 550 and 650 may snap in Servo with system fonts; fall back to 500
   and 600 and check in the mounted app.

## 7. Not found or unverified

- Zephium notes have no callouts, properties/frontmatter UI or tag UI (4.5 is
  derived), and no horizontal tab strip (vertical sidebar only).
- `docs/design/system.md` names Geist, but no Geist font is loaded in code
  (`tokens.css:433-436` is system fonts only). Do not add Geist.
- Not recorded: Zephium `--color-ring` focus colour.
- Unverified in Servo: `::-webkit-scrollbar`, weights 550 and 650, `color-mix()`.
  Unverified in the pinned GPUI: arbitrary `FontWeight` floats, letter-spacing.
- Platinum has no GPUI editor tab or `SyntaxStyle` wiring yet, so 5.2 has no
  current values to compare against.
- Nothing was built, run or screenshotted. All values are read from source.
