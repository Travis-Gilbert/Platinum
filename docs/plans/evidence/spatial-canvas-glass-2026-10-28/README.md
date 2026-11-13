# Glass on a real window, 2026-10-28

P0's last item: Frame's Liquid Glass captured on real windows over two backdrops, with the WindowServer's blur in the picture.

## How

- **Home:** `feat/native-home` at `40a50b8`, built in `cargo-targets/frame` (479 s). It ran from `frame-deps/FrameHome.app` (`dev.frame.browser.home`) with a temporary HOME and no sign-in.
- **Canvas:** `feat/spatial-canvas` at `babc913`, the copied binary in `frame-deps/FrameCanvasGlass.app` (`dev.frame.browser.canvasglass`), also with a temporary HOME.
- **Backdrops:** a Preview window filling the built-in display behind Frame:
  - `backdrop-dark.png`: dark stripes with white stair lines;
  - `backdrop-light.png`: rainbow stripes on white.
- **Captures:** full-screen captures through computer use, which shows only granted apps. A capture of the window alone shows the glass as a flat fill, because the desktop behind it is not in the picture.
- **Machine:** macOS 26, where `NSGlassEffectView` exists. Reduce Transparency and Increase Contrast are off.

## What the captures show

Home is `feat/native-home`'s workspace. It shows when the app starts, and again when the last tab closes.

| File | Theme | Backdrop | Result |
|---|---|---|---|
| `home-dark-over-dark.jpg` | Dark | dark | The glass dims and blurs the stripes. All text reads. |
| `home-dark-over-light.jpg` | Dark | light | **Fails.** Over the white parts the glass turns mid-grey, and the muted text disappears. |
| `home-dark-over-light-continue-zoom.jpg` | Dark | light | Close-up: "searches you run" in Continue's hint is lost. |
| `home-dark-over-light-hints-zoom.jpg` | Dark | light | Close-up: "New tab", "Search tabs" and "Right-click The same search" are lost. |
| `mail-dark-over-light.jpg`, `mail-dark-over-light-zoom.jpg` | Dark | light | **Fails.** The Mail place's description is unreadable. Its title and Back button read. |
| `agent-dark-over-light.jpg` | Dark | light | **Fails** the same way. |
| `home-light-over-light.jpg` | Light | light | All text reads. The glass shows only as a pale frost. |
| `home-light-over-dark.jpg` | Light | dark | All text reads. The white stair lines show softly through. |
| `tab-light-over-dark.jpg` | Light | dark | A page's card, its toolbar and the selected tab are opaque. Only the tab row is glass. |
| `last-tab-closed-light-over-dark.jpg` | Light | dark | Closing the last tab brings Home back, and Continue lists the closed page. |
| `canvas-tab-dark-over-dark.jpg`, `canvas-tab-row-dark-over-dark-zoom.jpg` | Dark | dark | The canvas build: only the tab row is glass. The new-tab card is opaque. |
| `canvas-tab-dark-over-light.jpg`, `canvas-tab-row-dark-over-light-zoom.jpg` | Dark | light | The same. The tab row's text sits on the selected tab's own colour, so it reads. |

## Against `glass.rs`'s intent

- **Holds:**
  - The glass is clear enough that what is behind shows through.
  - Pages, their toolbars and the selected tab stay opaque.
  - The address field keeps the theme's colour.
  - The light theme reads over both backdrops.
  - The canvas build's chrome follows the same rules.
- **Does not hold:** the module says the dark tint (`DARK_TINT = 0.65`) is "as much as keeps Home's text readable over a white window behind Frame". That holds for Home's primary text (`foreground`), but not for its secondary text (`muted_foreground`): Continue's hint, the shortcut hints, and the places' descriptions.
  - `home.rs`, `places.rs` and `browser/underlay.rs` set `muted_foreground` directly over the glass.
  - `glass::seen` stands the frame's colour in for the glass. That is true over a dark desktop and false over a light one, so a contrast check through `seen` cannot catch this.
- **Fix options, not taken here:**
  - raise `DARK_TINT`;
  - give the dark theme a frame coat as the light theme has (`LIGHT_FRAME`);
  - or put Home's secondary text on `glass::field`-style surfaces.

  Each changes how much of the desktop shows, so it is a design call.

  The fix taken was none of these: the switch to regular glass, below.

## Variants: regular glass, and GPUI inside the glass

Two questions came after the first captures. Can regular glass be recolored? And does GPUI's view have to stay above the glass?

### How

- **Build:** `feat/native-home` with the chrome fixes of the same day (larger chrome icons, Ask uses the agent icon, no tooltips on the places, no shortcut hints).
- **Temporary switch in `glass::macos::install`, since reverted:**
  - `FRAME_GLASS_STYLE=regular` sets `NSGlassEffectViewStyle::Regular`;
  - `FRAME_GLASS_EMBED=1` moves GPUI's `NSView` out of the window's content view and makes it the glass's `contentView`.
- **Everything else unchanged:** the same tints (`DARK_TINT`, `LIGHT_TINT`, the light theme's `LIGHT_FRAME` coat), the same backdrops, the same capture method.

### Captures, in `variants/`

| File | Glass | Theme | Backdrop | Result |
|---|---|---|---|---|
| `a-clear-dark-over-light.jpg` | clear, as committed | Dark | light | Fails as before: "example.com" under Continue is lost. |
| `b-regular-dark-over-light.jpg`, `b-regular-dark-over-light-continue-zoom.jpg` | regular | Dark | light | Dark charcoal with the stripes' colors softly through, not grey. All text reads, the muted text too. |
| `b-regular-dark-over-dark.jpg` | regular | Dark | dark | Reads. Frostier than clear: the white stair lines barely show. |
| `b-regular-light-over-dark.jpg` | regular | Light | dark | Near-opaque light: the frame coat on top of the frost hides most of the desktop. |
| `c-embed-regular-dark-over-dark.jpg` | regular, GPUI inside | Dark | dark | The same picture as `b-regular-dark-over-dark.jpg`. |
| `c-embed-regular-dark-over-light.jpg`, `c-embed-regular-dark-over-light-notes.jpg` | regular, GPUI inside | Dark | light | The same picture as `b-regular-dark-over-light.jpg`. The Notes placeholder's description reads. |

### What they show

- **Regular glass takes the tint.**
  - `glass.rs`'s module doc says window-sized regular glass frosts to a flat grey. With the frame color as its `tintColor`, it does not: it takes the theme's color, and the desktop's colors show through it softly.
  - Over a light desktop it fixes the muted-text failure, with no change to the text colors.
  - It shows the desktop's colors, but less of its shapes than clear glass does.
  - With regular glass, the light theme's frame coat is likely more than it needs.
- **GPUI's view can sit inside the glass.**
  - It draws, AppKit's accessibility presses reach it, and the keyboard reaches it: ⌘L and typing went to the address field.
  - Not tried: a real mouse click. Another app's window covered the display, so synthetic clicks would have landed on that app.
  - Startup logged `RefCell already borrowed` once. Moving the view while GPUI is still opening the window likely calls back into GPUI's window state.
  - It changes nothing in the picture. GPUI draws its own pixels with Metal, and the glass's adaptive treatment applies to AppKit's own content, such as template symbols and vibrant labels. GPUI's text is not among them.
- **Apple's guidance points the same way** (Meet Liquid Glass, WWDC25):
  - Regular "provides legibility regardless of context", at any size, over any content.
  - Clear has no adaptive behavior and needs a dimming layer. It fits only over media-rich content, where dimming does no harm and the content on top is bold and bright.
  - Frame's window sits over whatever desktop is behind it, and Home has muted text, so clear's conditions do not hold.

## The switch to regular glass

After the variants, Frame moved to regular glass, tinted to the frame's color, with GPUI's view above it.

### How

- **Build:** `feat/native-home` with the chrome fixes and the switch, uncommitted, built in `cargo-targets/frame` (266 s). `cargo test --locked --bin frame browser::tests`: 16 passed (104 s).
- **Tuning:** one build read the two tints and the light theme's coat from environment variables (since removed), so trying a value took a relaunch, not a rebuild.
- **Measuring:** `glass_range.swift`, in this folder. It takes a text-free region of the glass (x 130 to 1190, y 130 to 320 of the 1372x891 capture), splits it into 24-pixel blocks, and takes each block's median luminance. Then it gives the WCAG contrast of the theme's own text colors against the worst block. Sampling the text itself was unreliable, because JPEG blurs it.
- **Floor:** Home's secondary text (`muted_foreground`) at 3:1 or better. On their own solid frame the themes give it 3.41 (Dark, `#6d6d6d` on `#181919`) and 5.00 (Light, `#586069` on `#e1e4e8`).
- **Focused or not:** the close button at (119,104) tells: red while Frame is the key window, grey when not. A capture taken with an app that computer use hides (the Claude window) in front shows Frame unfocused without showing why. For an unfocused capture of Frame still visible, a second Preview window parked mostly offscreen was made key.

### Tuning, regular glass

| Theme | Tint | Light coat | Backdrop | Window | Glass luminance | Muted text |
|---|---|---|---|---|---|---|
| Dark | 0.65 (Clear's value) | n/a | light | focused | 0.025 to 0.028 | 2.60 |
| Dark | 0 | n/a | light | focused | 0.078 to 0.091 | 1.44 |
| Dark | **0.85** | n/a | light | focused | 0.015 to 0.016 | **3.06** |
| Dark | 0.85 | n/a | light | unfocused | 0.105 to 0.126 | **1.15** |
| Light | 0.6 | 0.8 (Clear's value) | dark | focused | | near-opaque, see `b-regular-light-over-dark.jpg` |
| Light | 0 | 0 | dark | focused | 0.525 to 0.714 | about 3.5 |
| Light | 0.7 | 0 | dark | focused | 0.556 to 0.672 | 3.68 |
| Light | 0.85 | 0 | dark | focused | 0.563 to 0.665 | 3.72 |
| Light | 0 | 0 | dark | unfocused | 0.477 to 0.646 | 3.20 |
| Light | 0.7 | 0 | dark | unfocused | 0.380 to 0.517 | 2.61 |

- **Dark needs 0.85.** At 0.65 the muted text falls to 2.60 over the light backdrop.
- **Light needs no coat and no particular tint.** Its text reads on regular glass over the dark backdrop at any tint tried, so the coat went and the tint stayed at 0.6, for the frame's color.
- **An inactive window's glass is a different material.** AppKit draws it far lighter: the dark glass at 0.85 goes from 0.015 to about 0.11 over the light backdrop, and the muted text falls to 1.15 ("example.com" disappears in `variants/d-regular-dark-085-over-light-unfocused.jpg`; compare `variants/d-regular-dark-085-over-light-focused.jpg`). Inactive light glass with a tint is darker than without one. `NSGlassEffectView` has no property for this: its header in the macOS SDK lists `contentView`, `cornerRadius`, `tintColor`, `style` and, from macOS 27, `effectIsInteractive`.

### What changed in `feat/native-home`

- `glass::macos::install` sets `NSGlassEffectViewStyle::Regular`.
- `DARK_TINT` is 0.85, `LIGHT_TINT` stays 0.6, and `LIGHT_FRAME`, the light theme's coat, is gone.
- `glass::frame` takes the window. It is clear over the glass while the window is active, and the theme's title bar while the window is inactive or there is no glass, the way AppKit's own vibrancy turns flat in an inactive window. GPUI redraws a window when its activation changes, so nothing else has to watch for it.
- `glass.rs`'s module doc and the README say regular glass, give the tints, and give the inactive rule.

### Final captures, in `regular/`

| File | Theme | Backdrop | Window | Glass luminance | Muted text | Result |
|---|---|---|---|---|---|---|
| `home-dark-over-light-focused.jpg` | Dark | light | focused | 0.015 to 0.016 | 3.06 | Dark charcoal, the stripes' colors faintly through. All text reads. |
| `home-dark-over-light-unfocused.jpg` | Dark | light | unfocused | 0.010 (solid) | 3.40 | The theme's solid frame. All text reads. Home's tiles nearly vanish into the frame, as they do without glass. |
| `home-dark-over-dark-focused.jpg` | Dark | dark | focused | 0.012 to 0.015 | 3.11 | Near-black frost. The desktop barely shows. |
| `home-light-over-dark-focused.jpg` | Light | dark | focused | 0.551 to 0.683 | 3.65 | Pale grey frost, the stripes' shapes and colors softly through. |
| `home-light-over-dark-unfocused.jpg` | Light | dark | unfocused | 0.773 (solid) | 5.00 | The theme's solid frame. |
| `home-light-over-light-focused.jpg` | Light | light | focused | 0.687 to 0.832 | 4.48 | Near-white, the rainbow faintly through. |

Every capture holds the floor. The tightest are the dark theme over either backdrop while focused (3.06 and 3.11); its own solid frame gives 3.41.

## Also seen

- The places on `feat/native-home` are placeholders. Mail, Agent and the rest open a "<Place> is not built yet" page.
  - No local branch has a Mail or Agent module.
  - `feat/little-arc`, `feat/arc-split-model` and `claude/arc-browser-platinum-planning-be32e2` carry the same placeholders.
- `claude/arc-preview-peek-4685e7` is based on `4f3373f`, the merge of `feat/liquid-glass-chrome`. It does not contain `feat/native-home`, so it has no Home, `places.rs` or `underlay.rs`.
