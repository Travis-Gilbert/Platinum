---
title: TabBar's per-tab wrapper grows past the tab's max_width
kind: gotcha
trigger_case: >
  Frame's tabs, given flex_1() and max_width(240), sat 25.5 px apart:
  each tab stopped at 240 while the wrapper TabBar lays it out in kept
  growing. Patching tab_bar.rs to give the wrapper the same max_w closed
  the gap; tabs_sit_side_by_side_with_new_tab_at_the_end measures it.
rule_short: >
  Growing tabs in a GPUI Kit TabBar need the wrapper capped too; measure
  tab bounds in a UI test rather than trusting the tab's own max_width.
---
