# gpui-component 0.8.0, carried by Frame

This is the published `gpui-component` 0.8.0 (GPUI Kit's styled layer) with a
few additions, applied through `[patch.crates-io]` in `Cargo.toml`. Every
change is marked `THEOREM PATCH`.

Why: Frame's selected tab is a folder tab joined to the toolbar below it, and
the toolbar takes the color of the page it sits over, as Dia's does. The theme
is one set of colors for the whole window, so a tab and an address bar could
not take a page's color, nor text that reads on it, and in a split each side
has its own page.

The changes:

- `src/tab/tab.rs`: `Tab::selected_bg(bg)` and `Tab::selected_fg(fg)` set the
  fill of a selected `TabVariant::Folder` tab and the color of its text and
  icons, falling back to the theme's as before. A folder tab's content starts
  at its leading edge, as a browser's does, instead of centering.
- `src/tab/tab_bar.rs`: the wrapper each tab is laid out in takes the tab's
  `max_width` too. Without it, tabs that grow (`flex_1()`) stop at the cap
  while their wrappers keep growing, and gaps open between them.
- `src/button/button.rs`: a `Toolbar` keeps a button's `custom` variant
  instead of making every button its ghost; the custom colors are the ones
  that suit the surface under the toolbar (test in `src/toolbar.rs`).
- `src/input/input.rs`: `Input::foreground(fg)` draws the placeholder and the
  caret in `fg`, for an input without appearance on a surface whose color is
  not the theme's. The text itself takes the color set around the input.

Re-applying them on a GPUI Kit upgrade: copy the new `gpui-component` from the
registry, carry the marked changes, and build Frame. Drop any the new release
makes unnecessary (`tabs_sit_side_by_side_with_new_tab_at_the_end` in Frame's
tests covers the tab wrapper).
