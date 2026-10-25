# The native workspace beneath the pages

Status: vision, recorded 2026-10-25. The first slice (a native Home and a
native search results page) is on branch `feat/native-home`; see
"Order of work" at the end.

## The idea

Frame draws every web page as a card that sits on the browser's chrome. The
chrome is GPUI Kit; the card's content is Servo. That arrangement already says
something: the page is on top, and the browser is what it rests on.

The native workspace makes that literal. Frame's own capabilities are native
GPUI Kit places, drawn on the chrome itself rather than inside a card:

- Home, with search and routes to everything else.
- Search results (the SERP).
- Mail.
- The library (media, saved pages, files).
- The agent (Theorem by default).
- A terminal.
- Installed apps (App Host).

Close every tab and the cards go away, leaving the workspace underneath: Home,
from which you can search the web, open a native place, or open a page again.
Nothing about the workspace depends on a web page being open, and nothing
native is drawn as if it were a website. A user can always tell which is which:
web content is in a card; Frame's own surfaces are the chrome.

The persistent part is the workspace and its state, not a stack of live views.
Native places can be created, hidden and released as needed, the way tabs are.

## Principles

1. **Native means GPUI Kit.** Frame's own surfaces use GPUI Kit components and
   follow its Design and Coding Guides. They are not HTML rendered by Servo.
2. **Web content stays in cards; native content is the chrome.** The visual
   difference is the security and trust boundary made visible.
3. **Theorem is native and optional.** It takes part everywhere when it is on.
   With it off, Frame is still a complete browser, search engine, mail client,
   library and terminal.
4. **Objects, not pixels.** Native places know exactly what they show. They
   hand the real object (a result record, a draft, an asset) to whatever needs
   it. Visual interpretation is for surfaces that cannot say what they contain.
5. **One operation, several paths.** A menu command, a keyboard shortcut and a
   drag all invoke the same underlying operation.
6. **Honest states.** A place that is not built yet says so. A result that came
   from the local index is labelled local. An action that ran is reversible or
   clearly final.
7. **Never by color alone.** Every surface stays usable and understandable
   with its colors removed: in grayscale, under a strong page tint, with
   Increase Contrast or Reduce Transparency on. This is WCAG 1.4.1, "Use of
   Color", often called color independence or redundant coding. Differences
   and contrast come from component design, spacing and texture: shape, fill
   versus outline, weight, borders and separators, grouping, position, icons
   and words. Color can reinforce a meaning, never carry it alone. See
   "Visual design" below.

## Places

### Home

Home is a native destination owned by the shell. It holds:

- A search and address field: an address opens a page; anything else searches.
- Routes to the native places (Mail, Library, Agent, Terminal, Apps).
- Things to resume: recent pages and recent searches.
- Later: installed App Host apps, chosen collections, ongoing agent tasks, and
  a user-arranged layout built from the same components.

A new tab shows the same content under the tab's address bar, on a pane of the
chrome rather than a page's opaque card. When the last tab closes, Home
appears on the chrome itself, full window.

### Search

RustyWeb returns structured results (title, URL, snippet, providers, lane,
date, and an optional cited answer), so the SERP is a GPUI view over those
records rather than an HTML document.

What a native SERP makes possible:

- Keyboard selection of results: arrows move, Return opens, Command-Return
  opens in a new tab.
- Return from a page to the exact search state: query, scope, selection and
  scroll position.
- Open a result beside the results, keeping the query visible.
- Select several results and save them to a collection or a draft.
- Give Theorem the exact result record, with no need to interpret pixels.

One search field can reach several kinds of information. Each scope is a
separate, visible operation, because searching your own material and sending a
query to a web provider are different things:

| Scope   | Searches                                   |
| ------- | ------------------------------------------ |
| Web     | RustyWeb's providers and its local index   |
| Library | Saved material                             |
| Mail    | The connected mailbox                      |
| History | Pages and searches visited in Frame        |
| Apps    | Installed apps                             |

A combined view can come later; the scope stays readable and easy to change.

A result row gives the title, the destination and the excerpt the most weight.
Provenance (which providers returned it, whether it came from the local index
or was retrieved live, when it was published, when it was retrieved) is real
information, but it belongs in a details area for the selected result rather
than in every row. Two times are never conflated: *published* is the page's own
date; *retrieved* is when Frame or RustyWeb fetched it. A live web lane is
"retrieved now", not "fresh", which would read as a claim about the content.
Snippets from providers are untrusted markup: Frame keeps their emphasis as
text styling and shows nothing else of the markup.

### Mail

A communication app connected to the person's mailbox, starting with Gmail
through the Gmail API (messages, threads, drafts, labels), with Frame's own
interface. Customization goes where people feel it: layout, density, which
fields a list shows, views by person, thread, label or saved search, toolbars,
context actions, and where a draft opens (beside a page, in a panel, or full
view). All views share one account and draft service.

Incoming HTML mail may need a restricted Servo view inside the native app.

**"Add to email draft…"** is a browser-wide action. It creates or edits a
draft and sends nothing. With a composer open it offers that draft; otherwise a
new or existing one. Insertion keeps the existing text and can be undone. What
it inserts depends on the selection:

| Selection                   | Contribution to the draft                     |
| --------------------------- | --------------------------------------------- |
| Text                        | The exact quotation with a source link        |
| Link                        | Its label and resolved destination            |
| Image                       | The original asset, with its source           |
| Video                       | The watch link, title and current timestamp   |
| A selected video frame      | A captured still, identified as a frame       |
| Local file or attachment    | An attachment with the selected file's bytes  |
| Several objects             | An ordered insertion of the above             |

An original image and a screenshot crop are different things; the crop is
offered only as an alternative. A file attachment keeps the selected version,
not a mutable path. The action works without any model.

### Library

A personal library of references, captured material and actual files, kept
distinct from one another. An item can have a source URL and title, an
original file, a captured version or excerpt, a preview, notes, tags,
collections and annotations, a playback position or interval for media, and a
record of where and when it was captured.

Views: a gallery, a dense list for searching and organizing, mixed
collections, a reading or viewing surface with notes beside it, a comparison
view, and a persistent player that keeps playing elsewhere in the browser.

Save the thing that was interesting: a timestamp or interval of a video, a
passage of a page, a region of an image together with the original.

Storage: file bytes live in an asset store; a catalog holds metadata and
references. Identical content is deduplicated while each capture keeps its own
history. Mail, the library and Theorem refer to the same asset. Removing an
agent integration never removes saved material. The library is the dependable
native collection; the Theorem Index can add organization over the same
objects rather than becoming a second collection to keep in sync.

Specialized content keeps its renderer (a restricted Servo view for HTML, a
playback backend for video, a PDF renderer), with GPUI owning the surrounding
app, commands, selection and layout.

### Agent

Theorem has a dedicated native place for conversation, ongoing tasks, memory,
activity, model choice and configuration, and it also takes part inside other
places: a draft, a search, a collection, a page, a terminal.

- **Theorem by default.** Bring-your-own-key works through Theorem's provider
  integrations. Choosing a model for Theorem is a different choice from
  choosing a different agent.
- **Frame is an ACP host.** In Agent Client Protocol terms Frame is the Client;
  Theorem and other compatible runtimes are Agents. Start with ACP's current
  stable version (v1) and its capability negotiation.
- **Capabilities through MCP.** Frame defines typed operations over its own
  objects and offers them to agents as MCP servers at session setup: inspect
  the shared browser pane, read the selected objects, search a collection, add
  an asset to a draft, propose a composer edit, open a result beside the
  current view. The browser owns these operations and their scope.
- **Results return to the work surface.** A reply about a draft becomes a
  proposed edit to that draft. A comparison becomes a comparison view of the
  real items. A browser action has a visible target and can be interrupted.
  The transcript accompanies the work; the work itself appears where it
  belongs.

The harness policy keeps its separation of learned policy, verbalizer and
deterministic action boundary. Moving Theorem's interface into GPUI does not
tie its backend's lifetime to an open panel.

### Terminal

A full terminal: a pseudoterminal running the shell, `alacritty_terminal` for
terminal state (grid, VT parser, selection, TTY event loop), and a GPUI view
that draws that state and handles input, selection, scrolling and resizing.

Ratatui builds apps that run inside a terminal; it is not a terminal emulator.
Ratatui apps, including any Frame tools built with it, run inside this
terminal.

Selected output can become context; a generated file can go to the library or
a draft. ACP's terminal methods for agent commands can share the
infrastructure, while the user's shell and an agent's command keep separate
identities and lifetimes.

### Apps

Apps installed through App Host have a collection on Home. Supervising an
arbitrary app does not make its window embeddable in a split: native Frame
apps, hosted web apps, streamed surfaces and separately launched programs have
different presentation mechanisms. Home still gives all of them one place.

## The workspace: panes, splits and drag and drop

Splitting belongs to the shell and works for every kind of view: a page, the
results, a mailbox, a draft, a media viewer, an agent session, a terminal.

| Arrangement               | What it enables                                    |
| ------------------------- | -------------------------------------------------- |
| Search beside a page      | Explore results while keeping the search           |
| Page beside a draft       | Quote, attach and compose while reading            |
| Collection beside a draft | Pick saved material and assemble a message         |
| Page beside the agent     | Work together on the visible task                  |
| Terminal beside a page    | Documentation next to a running tool               |

A view's identity is independent of its pane: moving a draft does not create a
new draft; moving an agent session does not cut it from its task.

An agent beside a page shows what it is using, for example
"Using: Browser pane · Article title", and can follow that pane or hold a
particular page or selection. Keyboard focus is separate: typing in a composer
does not retarget an agent that is comparing two pages.

Drag and drop carries typed objects (GPUI has typed drag and drop; the bridge
from Servo and the payload conventions are Frame's work):

- A quotation carries its text and source.
- An image carries its source and, when available, the original asset.
- A video selection carries its URL and time or interval.
- A library item carries its existing reference.
- A mail attachment carries its identity and the bytes the destination needs.

The drop target decides the operation: a collection saves, a composer inserts
or attaches, the agent adds to context. "Add to email draft…" is the same
operation as dropping onto a draft. In the other direction, dragging a library
item into a site's upload field turns it into a file the page accepts.

## Visual design

The chrome takes its color from the theme and from tinted or liquid glass, and
a page can lend the toolbar its colors. Because that color changes with every
site, nothing may depend on it (principle 7).

**Structure carries meaning.** Selection is a filled item plus a change of
structure (a leading mark, a weight change), not a hue. A toggle that is on is
filled; off is outlined. A local result says "Local" with an icon; a web result
is named by its site, as on a search engine's page. Regions are told apart by spacing, separators and elevation, not by
two shades of the same color. A test for every surface: take a grayscale
screenshot, and everything a person needs is still there.

**Glass reinforces structure.** It marks the chrome as the layer under the
pages and floating controls as temporary layers above them. It is never a
decoration on content.

- Documents, email and other dense content sit on stable, opaque, readable
  surfaces. Glass does not go behind text people have to read.
- The address and site-identity area stays clear even when the toolbar takes a
  page's colors: its own surface, its own contrast, the host always legible.
- Ordinary browsing stays compact. Larger tools (the agent, downloads, details)
  open in a drawer or beside the page, not as more toolbar.
- Reduce Transparency and Increase Contrast get solid, higher-contrast
  surfaces in the same structure.
- Light and dark appearances are both first-class. Glass in light mode is
  lighter than the theme beneath it, not a darkened copy of it.

**What the platform allows.** GPUI's `Blurred` window background is
`NSVisualEffectView`, not `NSGlassEffectView`. Real Liquid Glass needs a
transparent window (`WindowBackgroundAppearance::Transparent`) and a native
adapter that places `NSGlassEffectView` under GPUI's view (objc2-app-kit 0.3.2
has the bindings; Rhein-Industries/gpui-macos 0.1.1 is precedent for underlays
below the GPUI renderer and for contained glass popups). One Metal surface
cannot put system glass between page pixels and toolbar pixels; that needs
separate composition surfaces, native `NSToolbar` controls, or a shader of
Frame's own.

## The shared subject

Clicking or selecting something establishes the current subject of the
interaction. That does not call a model on every click: the browser updates a
lightweight reference, and the next turn or agent operation receives it. An
agent that is actively working can also receive a context-change event.

### Lifetimes

| Context        | Behavior                                           |
| -------------- | -------------------------------------------------- |
| Current focus  | What you are pointing at or have selected now      |
| Pinned         | Objects deliberately kept available for a while    |
| Saved          | Objects kept in the library or another destination |

Selecting something does not turn a conversation into a task. You can select a
graph and ask why it behaves that way.

### What a reference carries

A picture inside a link is at once an image, a link, an element and a visible
region; the reference keeps all of these instead of choosing one. It answers:

- Where it came from: tab, page, frame, mail account, app, document.
- What exactly was selected: element, text range, message, attachment, media
  interval, visual region.
- Which representations exist: text, URL, original asset, screenshot crop,
  structured record.
- When it was observed: document revision, navigation, frame, timestamp.
- What can operate on it: save, attach, quote, compare, annotate, transform.
- Which properties are known and which are inferred.

Consumers ask for the representation they need; nothing large moves because the
pointer moved.

### Where object context comes from

| Surface                 | Primary source                              | Visual interpretation adds                  |
| ----------------------- | ------------------------------------------- | ------------------------------------------- |
| Frame's native places   | The app's exact object and state            | Appearance and spatial context              |
| Ordinary web page       | Hit test, DOM, accessibility, resources     | Visual grouping, icon meaning               |
| Canvas-heavy page       | The canvas element, any app integration     | Candidate controls and text in the canvas   |
| Opaque hosted app       | Whatever its adapter exposes                | Regions the host cannot otherwise identify  |

Native places register their objects directly; there is no reason to OCR
Frame's own interface.

**OmniParser** is one contributor, for interface regions on surfaces without
reliable semantics. Its numbered regions belong to one parse and are not
identities, and its interactivity flag is a candidate, not proof. Objects
inside photographs or video (a chair, a person) are a different problem: allow
selecting a region immediately and add a vision or segmentation model where
needed. Selection is immediate, from the native hit test; interpretation is
asynchronous. A screenshot and its coordinates belong to one observation; an
action against the live page needs a fresh target check. Run the parser in a
managed local inference process with models kept loaded; the stock wrapper
picks CUDA or CPU, so Apple GPU performance needs measuring before promising
continuous parsing. Pin the model combination deliberately: the original V2
detector is AGPL, and a newer MIT-licensed detector exists.

### Engine work in the Servo fork

Frame pins Theorem's Servo fork at `ebc89e68` (branch
`Travis-Gilbert/servo07-browser-20261013`), which already has:

- `WebView::hit_test`: the topmost element at a point, as a handle that
  matches the document snapshot.
- `WebView::document_layout_snapshot`: DOM properties, parents, geometry,
  paint order and selected computed styles.
- Context-menu element information: link, image, editable field or selection,
  with URLs where available. Frame already uses this for its context menu.

Work to do on top of it:

- **Scope and redact before an agent sees anything.** The snapshot reads the
  live value of every input, password inputs included, and copies every
  attribute. The agent-facing projection needs selection scoping and redaction.
- **Durable identities.** The current handles are writable `data-theorem-id`
  attributes, fine for locating an element in a document. A saved object needs
  its own identity, and an action must verify its target still exists in the
  intended document. Shadow content and nested frames need deliberate handling.
- **A selection fallback the page cannot cancel.** A page can cancel the normal
  context menu, so Frame needs a browser-owned shortcut or gesture for
  selecting an object.
- **Capture what was visible.** `take_screenshot` waits for loading and
  rendering to settle; selection wants the frame that was on screen at the
  click.
- The fork's devtools transport is Firefox's actor-based remote protocol, not
  the Chrome DevTools Protocol.

## Theorem: native and optional

With Theorem off, browsing, native search, mail, the library, App Host,
terminals, splits, drag and drop and every ordinary command keep working. On,
it adds contextual help, reasoning across places, delegated work and
continuity. Three actions stay distinct:

- **Close the agent view:** changes what is visible.
- **Pause a task:** stops that work.
- **Disable the agent:** ends its participation and revokes its browser
  access, with a clear policy for work in progress.

Closing a conversation leaves an explicitly running research task running.

## Local inference service

Frame owns local inference as an application-level service with existing
engines underneath. Mail, the library, search and Theorem request work from
it, and it keeps running with every tab closed.

| Part                 | Responsibility                                            |
| -------------------- | --------------------------------------------------------- |
| Inference runtime    | Execute a particular model                                |
| Shared model service | Download, cache and load models; schedule; expose operations |
| Theorem harness      | Decide how to use models, context, memory and tools       |

Local semantic search works without the agent: a library can use local
embeddings with Theorem off, and a separate setting can turn local models off
while text search keeps working.

The first model is EmbeddingGemma 2 (official int8 ONNX build). fastembed
cannot load it, because the graph needs empty image, video and audio inputs,
so the embedder uses `ort`, `tokenizers` and `hf-hub` with a thin adapter.
Engines considered for generative models: uzu, WebLLM or similar.

## First demonstration

1. Right-click an image on a page; the exact object is highlighted.
2. Save it to the library with its source.
3. Add the same item to a native email draft.
4. Ask Theorem to help explain why you are sending it.
5. Edit the result in the ordinary composer.
6. Close the agent view; the draft and the library item remain.

That one flow proves the browser can identify objects, native places can
exchange them, Theorem can work on their real state, and the result lives
where the person needs it. Quotations, PDFs, video intervals, groups of
references and App Host apps extend the same foundation.

## Order of work

1. **Native Home and SERP (this branch).** Home on the chrome when no tab is
   open and inside new tabs; routes to the native places; recent pages and
   searches; a keyboard-driven SERP in GPUI Kit. Mail, Library, Agent,
   Terminal and Apps are routes to placeholder places that say they are not
   built yet.
2. The object reference and the shared subject, starting with the image
   right-click, including redaction and durable identities.
3. The library: asset store and catalog, gallery and list views, save from a
   page.
4. Mail on the Gmail API: account, list, thread, composer, "Add to email
   draft…".
5. Native places in the dock's panes, so splits hold any view; typed drag and
   drop between pages and places.
6. The agent place and Frame as an ACP host, with Frame's MCP capabilities.
7. The terminal.
8. The local inference service and library semantic search.
9. SERP scopes beyond Web as each place lands: Library, Mail, History, Apps.
