---
title: A page-coloured chrome must use the exact colour, then fix the ink
kind: anti_pattern
trigger_case: >
  Frame first clamped the sampled page colour toward the theme, so
  Wikipedia's white page gave a #333 toolbar in dark mode; the user saw
  the chrome being "influenced by" the page rather than taking its colour.
  Using the exact top-row colour and choosing readable ink (theme
  background when the fill's light/dark class differs from the theme's)
  matched Dia. A second miss: the editing field fell back to the theme's
  dark input box on a white toolbar.
rule_short: >
  Fill tinted chrome with the page's exact colour and adapt the
  foreground, including focus and editing states, never the fill.
---
