---
title: GPUI Kit Input text colour comes from the element around it
kind: gotcha
trigger_case: >
  Frame set the omnibox Input's foreground for a dark page, yet the typed
  address stayed dark. Input draws its text with the inherited
  window.text_style(); editor_style only supplies placeholder, caret and
  decorations. Setting text_color on the wrapping div fixed it.
rule_short: >
  To colour an Input's text, set text_color on its parent; Input::foreground
  (Frame patch) covers only placeholder and caret.
---
