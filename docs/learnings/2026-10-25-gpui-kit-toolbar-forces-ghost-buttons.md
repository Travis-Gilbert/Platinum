---
title: GPUI Kit's Toolbar turns every button ghost
kind: gotcha
trigger_case: >
  On gpui-kit.com in Frame's light theme, back, forward and reload drew
  #292c34 on a #1a1b26 toolbar although each had a custom light variant.
  ToolbarItem::sized calls Button::prepare_for_toolbar, which set
  .ghost().compact() and dropped the custom colours. Share and downloads,
  outside the Toolbar, read correctly.
rule_short: >
  A Button placed in a GPUI Kit Toolbar loses its variant unless
  prepare_for_toolbar keeps ButtonVariant::Custom (Frame's vendor patch).
---

Check `vendor/gpui-component/src/button/button.rs` (THEOREM PATCH) is
still present after a GPUI Kit upgrade before colouring toolbar buttons.
