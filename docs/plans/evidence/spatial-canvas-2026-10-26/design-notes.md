# Design notes for the spatial canvas, as pasted on 2026-10-26

These are the notes the plan in `docs/plans/spatial-canvas.md` was written from, kept verbatim. They were written before the code was checked; where the code disagrees, the plan's section "What the code says about the notes" says so.

I remember the browsing canvas idea, and after watching your recording and inspecting Platinum's actual source, I think there's a very promising way to implement it.

My recommendation is to build a spatial browsing mode on top of Platinum's existing GPUI/Servo integration, using GPUI Whiteboard for its spatial interaction machinery—not as the entire rendering system.

The recording demonstrates something important: webpages aren't merely arranged into columns like split tabs. You can zoom out until entire groups of pages become visible, pan around that larger space, and zoom back into an individual page to continue browsing.

That creates a different relationship with browsing history. Rather than replacing one webpage with another, you can leave webpages where you encountered them and navigate among them spatially.
1. The biggest discovery: Platinum already has the difficult rendering foundation

I inspected the current main branch of Platinum, which is presently packaged as Frame.

Its implementation has three particularly relevant pieces:

    src/scene.rs takes Servo's rendered frames through IOSurface and CVPixelBuffer into GPUI.

    src/page.rs places those frames inside GPUI's scene and routes mouse, wheel, keyboard, focus, and text-composition input into Servo.

    src/engine.rs runs one shared Servo engine with event-loop integration.

That's significant. Your webpages already render as GPUI scene content rather than just separate native browser windows layered over GPUI.

The existing implementation even includes rounded surface rendering and retains multiple frames in flight.

So the primary challenge isn't getting Servo to draw inside GPUI. It's extending an existing, working rectangular webpage surface to participate in a larger spatial coordinate system, while preserving normal browser behavior.

The design I would pursue has three independent responsibilities:

GPUI owns the canvas and spatial navigation. Servo owns live webpage behavior. Platinum owns the browsing session, history, and relationship between them.
2. How I would divide the actual architecture

Platinum Browser Shell

Tabs mode  Canvas mode — same browsing sessions

GPUI Spatial Canvas

Camera · gestures · page placement · connections · minimap · navigation trail

Servo Surface Host
Live webpage frames, focus and input

Canvas Renderer
Static previews, annotations and native panels

Browser Session & History Coordinator

Stable identities · visit sequence · resource lifecycle · restoration

RustyWeb / Browser Substrate

Search sessions · retained pages · captures · optional relationship graph

The important architectural rule is that switching between a normal browser tab and its canvas representation must not reload the webpage. Both modes should refer to the same underlying live session.
Where GPUI Whiteboard fits

gpui-whiteboard is a good starting point, especially because it already provides an infinite camera, panning, zooming, selections, dragging, resizing, and ordinary whiteboard objects.

But there's a limitation worth catching before implementation: its page-card element is an embedded page reference, not a container for an arbitrary live GPUI component. Its existing element vocabulary is largely shapes, text, images and those page cards. 
Docs.rs+1

So I would extract or adapt its camera, gestures, selection and geometry machinery into a new SpatialCanvas host. I'd make that host responsible for positioning both conventional whiteboard elements and arbitrary live surfaces.

This is better than mounting WhiteboardView as a background with a completely unrelated overlay of webpages. Two separately managed cameras would eventually disagree about positions, zoom levels, selection bounds and pointer interactions.

It also means you can continue using GPUI Kit's existing docking system for ordinary browsing without trying to turn the dock itself into an infinite canvas.
3. The critical engineering problem: two different kinds of zoom

This is where I would spend the most care.

Suppose a real webpage has an internal viewport of 1,200 × 800 pixels. At 35% canvas zoom, its node appears 420 × 280 pixels on the screen.

The website should not suddenly believe its viewport is 420 × 280 pixels. Its responsive layout shouldn't rearrange itself every time you zoom the canvas.

One webpage, two coordinate systems

Servo viewport
1,200 × 800

GPUI canvas projection
420 × 280 at 35%
The webpage retains its internal layout. GPUI scales its visual presentation.

In the current Page::place() implementation, the GPUI rectangle determines the Servo viewport size. I'd separate those concepts.

Servo maintains a stable document viewport. GPUI applies the canvas camera to the resulting frame. Ordinary canvas zoom becomes a compositor operation, not a webpage resize. A deliberate resize of the webpage node can change Servo's viewport, but dragging the whole canvas around should not.

Servo itself distinguishes webpage zoom from pinch zoom, but neither should become the canvas camera's primary zoom mechanism. 
Rust
Input routing needs the inverse transformation

The rendering side is only half of the work. If you click a link inside a shrunken webpage, Servo has to receive the corresponding point in its own viewport.

For a given page node, convert the pointer from GPUI screen coordinates into that node's local coordinates using the inverse canvas transform, then apply the appropriate device-pixel scale.

That same mapping must extend beyond clicking links. It needs to cover text selection, mouse hovering, scrolling, drag/drop, IME composition, context menus, HTML selects, and positioned browser dialogs.

Your existing Page::device_point() and the coordinate conversion in controls.rs are the natural integration points. But they currently assume an unscaled rectangular page, so they will need to understand the canvas projection.

I would also give gestures a clear ownership policy: dragging empty canvas space pans the workspace; scrolling over an interactive webpage scrolls that page; holding Space while dragging pans the canvas even over a webpage; dragging a page's GPUI frame moves that node rather than selecting text inside it.

This separation is necessary for the mode to feel natural.
4. Navigation should work across webpages, not just within them

I particularly like your idea of pressing Back and having the previous webpage automatically come into focus.

I'd go further: Canvas mode should have a unified navigation history that understands both webpage navigation and movement between webpage nodes.

Right now, Platinum's Browser::back() mostly operates on the active tab's Servo history, with a special fallback to its search results.

I'd introduce a browser-owned NavigationLedger above those individual web histories.

Imagine this sequence:

Your browsing trail
Example

Search A

Page B

Page C

Page D

Press ⌘ [ to return to Page C

When you press Back, the browser checks what the preceding navigation event actually was.

If it was navigation inside the same webpage surface, it delegates to Servo's existing history operations. If it was a transition between surfaces, it activates the previous SurfaceId, animates the camera toward that page, and restores focus. Forward reverses the operation.

The history entries should also remember the relevant camera position, so returning to a previous page feels like returning to a place, rather than having a tab suddenly appear somewhere offscreen.

Crucially, moving a card or casually panning the canvas shouldn't generate new history entries. Otherwise Back would become a replay of every little movement.

I would preserve the conventional Back behavior in ordinary Tabs mode, making this enhanced navigation policy specific to Canvas mode.
5. The minimap should be a real navigation instrument

Your bottom-left overview is an important part of the experience, not just decoration.

It should show the arrangement of webpage nodes, the camera's current viewport, and a fading path between recently visited nodes or searches.

I'd build it as a small, screen-fixed GPUI component that reads the same spatial scene and navigation ledger as the main canvas. It should not be a second independent canvas with a second source of truth.

Clicking a location in the minimap pans the main camera there. Clicking a visible trail point focuses the corresponding browser node. Double-clicking a cluster could fit that cluster into the viewport.

There are actually two fundamentally different kinds of connection worth preserving:

Navigation connections represent where you came from: search A led to page B, which led to page C.

Knowledge connections represent why things are related: two pages discuss the same concept, cite the same source, or contain conflicting claims.

The first is mechanically observable without AI. The second is where RustyWeb, embeddings, RustyRed and the graph-based research interface we've discussed become particularly useful.

I wouldn't mix them indiscriminately. The chronological trail should remain understandable even when the semantic relationship graph is hidden.
6. Performance: the entire canvas cannot consist of fully active webpages

This is the other major engineering challenge.

Platinum already has the right primitive in Page::set_visible(), which calls Servo's show() or hide(). Servo also exposes resource-throttling behavior in its webview API. 
Rust

But for a spatial browser, I would extend that into a lifecycle policy based on focus, visibility, distance, zoom and resource pressure.

Canvas node state
	

Rendering behavior

Focused webpage
	

Full live Servo surface, responsive to input

Nearby visible webpage
	

Retain the live session; update its presentation as resources permit

Small or distant webpage
	

Show a cached frame or thumbnail rather than continuously repainting

Outside the viewport
	

Retain lightweight metadata and restore the rendering on demand

Unopened search result
	

Native card containing title, snippet and preview; no live Servo webview

An important qualification: hiding a surface doesn't necessarily release all its script memory, network activity or other resources. Reclaiming memory may eventually require disposing of the actual webview and restoring it later.

Those are different lifecycle operations and should not be hidden behind a single is_visible flag.

I'd also preserve intentional background activity such as music, downloads or uploads rather than suspending it just because its card has moved offscreen.

The smoothness in your recording should come primarily from GPU compositing and cached representations, not from repeatedly asking dozens of websites to render at different sizes.

With the existing IOSurface bridge, that's achievable as an engineering direction. The exact memory and frame-rate behavior still needs to be measured with real pages.
7. A durable data model for the canvas

I would keep webpage identity separate from placement. You might have two cards showing the same URL, multiple visits to that page, and only one currently active Servo surface.

The useful identities are:

Identity
	

Meaning

ProfileId
	

Cookies, storage and browser isolation

SpaceId
	

A particular browsing canvas or investigation

SurfaceId
	

One live browser surface/session

VisitId
	

A particular visit or navigation state

PlacementId
	

Where a resource appears on the canvas

SearchSessionId
	

The query and its associated results

TrailEntryId
	

One step in the user's focus/navigation history

A canvas placement would store its resource reference, position, dimensions, layering and grouping. It would not own the website's session storage, DOM or browsing history.

This follows the same design principle we've used for Theorem documents: one durable resource may have several presentations.

Platinum's existing RustyWeb session store already gives you an important starting point for retaining searches and captured pages. I would extend that authority to represent the canvas rather than create a wholly unrelated search-history database.

For responsive dragging, update the GPUI scene immediately and persist accepted placement changes asynchronously or at bounded checkpoints. Never put a database commit on every pointer-move event.

And keep this capability in Platinum itself. A user should be able to browse spatially without enabling Theorem or making model calls. Theorem can later participate by inspecting selected pages, drawing evidence connections or organizing an investigation.
8. How I'd implement it incrementally

I wouldn't start by constructing the complete whiteboard UI. The riskiest integration is a real, interactive webpage under a spatial transform.

01

One live webpage on a zoomable canvas

Extract enough of GPUI Whiteboard's camera and gesture mechanics to place an existing Page surface within a spatial viewport. Prove panning, zooming, clipping, mouse targeting, scrolling, text input and native popover positioning without reloading or constantly resizing the webpage.

02

Multiple pages and unified history

Add the stable surface registry and spatial placements. Open another webpage as an adjacent node, preserve the first page, then prove Back and Forward move correctly across the surfaces. Add a Tabs/Canvas switch without recreating the Servo sessions.

03

Search layout, minimap and restoration

Reuse the existing RustyWeb search result model to create search-root and result nodes. Implement the minimap, trail and layout persistence. Close and reopen the browser, recover the arrangement, and lazily restore live pages while clearly distinguishing restored URLs from retained in-memory page state.

04

Virtualization and richer objects

Add thumbnail caching, visibility-driven rendering, resource limits, native notes, annotations and relationships. Test with a canvas containing dozens of webpages and a mix of static and dynamic websites, including accessibility, media and authentication cases.

The first milestone I'd make non-negotiable is simple: open a real website, type into it, zoom out so it becomes a small node, pan across the canvas, then zoom back in and continue interacting with the same live webpage without having lost its session.

That one demonstration would validate most of the rendering architecture.
My overall assessment

I would absolutely pursue the GPUI + Servo + Whiteboard combination, but with a slightly different division than your original suggestion:

Whiteboard contributes the spatial interaction model. Platinum supplies real browser surfaces. A new spatial host composes them together.

I would not use an HTML/React canvas inside Servo for this, and I wouldn't try to make the entire web engine aware of world coordinates. Servo should largely continue to think it's rendering conventional rectangular webpages. GPUI handles where those rectangles exist in the larger world.

The most interesting longer-term possibility is that the canvas stops being limited to webpages. A search result, a live site, a note, a PDF, a native GPUI app, and a Cosmograph research panel could coexist as different kinds of spatial objects. The selected objects could then become context for Theorem without forcing the entire canvas into a model prompt.

But the first product is already compelling without any of that: a real browser in which navigating the web leaves behind an explorable spatial history, instead of making everything you previously visited disappear behind the current tab.

That's the experience in your recording I'd preserve above all else. 
So the progression of this feature that I want is for a canvas to be something that can be shared or made public. So someone can see the things that you put next to each other, I suppose, in a way. And the other part of this that's important to explain is that the canvas should be able to hold interactions. So say someone wanted to write annotations on the canvas as it pertains to a particular web page. They should be able to do that. Or they want to link a bunch of web pages together, like nodes and edges in a graph. They should be able to do that Someone wants to run a web search over just their canvas. They should be able to do that I think that these links should be meaningful things, and I think that Rusty red becomes useful here for maintaining the links between these different sites. And maybe the canvas, if the canvas is made public, I'm just thinking out loud now, if the canvas is made public, maybe it could serve some of the ideas that I was describing where the web itself is meant to be a powerful platform So the whiteboard can hold connections. Other people have assigned. It can be shared. It can be made public and findable. But someone could also build an entire app and put it in the canvas and then say to cite their sources. It could be the visual links to those previous pages I think right-click affordances and a good toolbar are also going to be important here but haven't quite figured out precisely what to put there
…

I think you've just identified something more consequential than a browsing mode: the canvas could become a new kind of web document.

Not just a whiteboard containing websites, but a publishable, interactive composition of web resources, annotations, relationships, and executable applications.

And the distinction that makes this exciting is that the relationships themselves become part of what someone publishes.

A conventional web page presents content and links. Your canvas could present the thinking that connects those links: which sources support something, which contradict it, what someone noticed, what they built from those sources, and how another person can extend that work.

I'd think about the product in three layers:

    Spatial: Arrange, resize, group, annotate, and navigate webpages and other objects.

    Semantic: Define meaningful relationships between resources, statements, evidence, and applications, with RustyRed preserving their identity and history.

    Social and programmable: Publish canvases, discover other people's connections, contribute annotations, fork investigations, and interact with applications embedded in them.

The third layer changes the purpose of the first two. A canvas stops being solely your way of browsing and becomes something other people can visit and explore.
1. What a published canvas might look like

Imagine someone researching a topic. They've collected several web pages, compared conflicting information, written observations, and built a little interactive visualization using data from the sources.

An example public research canvas
Spatial relationships with attached evidence
Illustration
Conceptual arrangement: the nodes can be live web resources, native content, or applications. The colored edges represent authored relationships, not automatically inferred facts.

The visitor could explore the arrangement, zoom into the original resources, inspect why a connection was made, and interact with the visualization.

They might leave a response attached to a specific paragraph, or fork the canvas and extend the investigation in another direction.

Crucially, they wouldn't have to recreate the original author's browser tabs, search sequence, or local environment to understand the published work.

That separation between the author's live browsing session and the published canvas is going to be essential.
2. RustyRed should make connections meaningful

I think this is where RustyRed becomes much more than storage for the canvas.

There are two fundamentally different kinds of edges.

Visual edges express composition: these two things are grouped, compared, arranged in a sequence, or just visually connected. They might not have any deeper meaning.

Semantic edges express assertions: this article supports that conclusion, this study contradicts another, this chart uses that dataset, or this application was derived from those sources.

I'd preserve both. Requiring every line someone draws to become a formal knowledge assertion would make the canvas unpleasant to use. But when someone does specify the relationship, RustyRed should preserve its meaning beyond the current arrangement.

For example, a semantic connection might have:

Field
	

Example

Subject
	

Analysis result A

Relationship
	

supported_by

Object
	

Paragraph 14 of article B

Author
	

The person who created the relationship

Evidence
	

Quoted passage and source revision

Visibility
	

Private, shared, or public

Status
	

Authored, disputed, retracted

That gives you something far more useful than two node IDs and a line between them.

It also means that a canvas is just one representation of the graph. Someone could view the same connections as a table, a source bibliography, a graph visualization, a document, or an agent-accessible knowledge structure without losing the relationships.

One distinction I'd be very strict about: an authored relationship is not automatically a fact. If someone draws supports, RustyRed records that this person asserts that relationship. It doesn't declare universal agreement. A second person can dispute it without destroying the first person's contribution.

This becomes extremely important when public canvases start accumulating contributions.
3. Annotations should survive beyond the whiteboard

I'd support both free-floating notes on a canvas and annotations anchored to actual webpage content.

For example, someone selects a paragraph in Servo, right-clicks, and chooses Annotate selection. A note appears beside the webpage, with a line pointing to the selected passage. They can move the note anywhere, but it stays associated with that source.

This is where an existing standard is especially valuable.

The W3C Web Annotation Data Model describes annotations with bodies, targets, and selectors. Its TextQuoteSelector can identify a passage using the selected text and neighboring text, making it more resilient than simply storing its pixel coordinates. 
w3.org+1

I would use that as the foundation for portable web annotations, while keeping GPUI-specific positioning separate.

If the webpage changes, Platinum attempts to locate the passage again. If it cannot, it retains the annotation and marks the original anchor as unresolved instead of silently attaching it to the wrong text.

And because you're interested in public annotations, I'd extend this into lenses: somebody could browse an ordinary website with their own annotations, a research group's annotations, or selected public canvases' annotations overlaid on the same source. Those would be opt-in external layers, not changes to the website itself.
4. Search within a canvas is a natural graph operation

This should be a real scoped search, not a prompt telling an LLM to remember what was on the canvas.

When someone selects Search this canvas, the query should be restricted to authorized material belonging to that canvas: page references and indexed content, notes, annotations, edge labels, evidence, and relevant application artifacts.

You could support ordinary text search, semantic retrieval through the existing embedding system, and relationship-aware queries.

For instance, searching for housing affordability could show the relevant pages and annotations. Asking for sources that contradict this analysis could traverse authored contradicts relationships, then optionally investigate additional candidate disagreements.

Search results should also have a spatial presentation: matching nodes illuminate, irrelevant areas recede, and the camera can fit all matching objects into view.

There should eventually be a separate Search public canvases experience. That would search published compositions and their public relationship metadata, not accidentally extend a private canvas search into other people's material.
5. Applications inside canvases could become the really distinctive feature

This is my favorite part of what you're describing.

An application could be a first-class canvas node, alongside a website, a note, or a document.

The node has an application identity, an implementation revision, its own state and permissions, and relationships to the resources it uses. The runtime might be a native GPUI component, a Servo-hosted web application, or another explicitly supported surface.

Suppose somebody builds an interactive chart from three published reports. They could connect the chart to those reports with derived_from relationships. More importantly, selecting an individual chart element could reveal the exact source passage or dataset entry behind that value.

The visual links would be inspectable citations, not merely decorative lines.

For provenance, the W3C PROV-O ontology already describes derivation, attribution, and activities that produce resources. That is useful external vocabulary for publishing the lineage RustyRed maintains internally. 
w3.org

I would not require every application to use a graph API directly. It should be able to declare source and output references through normal Theorem/Platinum capabilities. The canvas then displays those relationships.

And an important boundary: a public canvas containing an application should not automatically execute arbitrary author-supplied code with a visitor's permissions. Public applications need sandboxing, explicit capabilities, and useful static or read-only fallbacks.
6. Making the canvas genuinely part of the open web

I would give every published canvas a permanent, ordinary HTTPS URL, a human-readable title and description, public node and relationship references, and versioned publication history.

But I'd also ensure that a published canvas is understandable without Platinum.

A regular web browser should be able to open an HTML representation with the spatial interface where supported, along with accessible source lists, annotations, citations, and application fallbacks. Search engines should be able to discover the meaningful text and links without needing to execute an entire native browser environment.

The web already gives you useful building blocks:

    Web Annotation for interoperable annotations.

    Schema.org CreativeWork for describing published work and citations.

    Webmention for notifying other websites that support receiving link notifications.

    ActivityPub as a possible later mechanism for federated publication and social activity.

I would use the first two early, investigate Webmention for external discovery, and defer full federation until people are actually publishing and interacting. None of these individually provides the complete canvas protocol. 
w3.org+2

There's also a web compatibility limitation worth preserving: native Platinum can display an independently navigated Servo webpage as a top-level webview, whereas a publicly hosted canvas viewed inside an ordinary browser cannot freely embed every site. Websites can restrict iframe embedding through CSP or X-Frame-Options. A public canvas therefore needs reliable link previews and open-in-browser behavior instead of promising live embedding everywhere. 
MDN+1

I'd make publication a deliberate operation that previews exactly what becomes public, especially when a personal canvas contains authenticated webpages, private documents, excerpts, or annotations.
7. The interaction design I'd start with

I would keep the permanent toolbar surprisingly small.

Canvas toolbar concept
Suggested tools

Select

Pan

Link

Note

Insert

Find

Example: right-click a webpage

Annotate page

Connect to another node

Find related material

Capture a version

Copy reference
Presentation concept only. The actual menu should change based on whether the target is empty space, selected text, a webpage, an edge, or an application.

I think the richest gesture could be dragging a connection from one node to another, then choosing what the relationship means. You wouldn't need to stop and open a graph editor.

When an edge is selected, a small inspector could show its relationship type, author, evidence and visibility. That same inspector could expose alternative visualizations of the relationship.

The minimap remains anchored in the bottom-left, while the current canvas's sharing and publication controls live in the upper-right. I'd keep the annotation and property inspectors collapsible so they don't compete with the sense of an expansive canvas.

For collaborative editing, Loro is worth reusing: its Rust CRDT implementation supports offline edits, synchronization, undo/redo, and document history. It could coordinate shared layout and annotation editing while RustyRed retains the durable identities, accepted semantic relationships, and publication revisions. 
Rust
8. One product decision I'd make now

I'd distinguish a working canvas from a published canvas, without making them two different document types.

A working canvas is the space you manipulate freely. It can contain temporary browsing sessions, private material, half-written annotations, experimental apps, and incomplete connections.

A published canvas is an explicitly selected, permission-checked revision of that work. Its sources and relationships have stable public identities, and other people can inspect or build on them.

Sharing could subsequently allow live collaboration; forking could create a separately owned continuation; subscribing could let someone follow changes to the original. None of those should silently mutate the author's published version.

That distinction means you can preserve the delightful messiness of browsing without turning every action into a publishing decision.

I also wouldn't make this exclusively a research tool. Someone could make a visual essay, a collaborative reference collection, an interactive tutorial, an artwork, a simulation, or an application whose inputs happen to be distributed across the web.

The longer-term vision I see is spatial hypertext that people can author, publish, and program.

The conventional web treats a link mostly as navigation. Platinum could make links into authored, inspectable relationships that exist independently of any one application or layout. RustyRed is well suited to retaining those relationships, while Servo and GPUI make them something people can actually interact with.

And that leads to the question I think is most worth exploring next: whether a public canvas is primarily a published object that others can visit, or a living space that others can continuously contribute to. I'd support both eventually, but build the first as the foundation. It gives you stable URLs, source attribution, versioned history, meaningful links, and a clear permission boundary before introducing the much harder problems of public collaboration and moderation.
One thing though is that publishing needs to create a searchable, addressable canvas, something that another unrelated person could Google, or type the exact domain of in, which means that a regular browser needs to be able to display it. And I, well, I guess that just means improving GPUI whiteboard WASM, but also the person who searches it— well, as I'm talking out loud, I'm thinking about this more. We should probably create settings for different kinds of publishing. Like, I was going to say that there should be permissionless editing, but maybe someone wants to publish something and have it be a view-only, and maybe someone else wants to publish something and have it be anyone who is interacting with this surface can edit and add to it, etc. I realize there's spaces in between those two things, but to keep it simple for now, I would like to just do those two settings.

Yes. This is the point where publishing stops meaning “export the canvas” and starts meaning “deploy a public web surface.”

The published object should have a stable HTTPS address, be crawlable by search engines, open in Chrome/Firefox/Safari without Platinum installed, and retain the interactive spatial experience. GPUI Whiteboard WASM is likely the main interactive renderer, but it should not be the only representation of the canvas.

A pure WebAssembly-painted canvas would make the experience visible to humans while leaving search engines, accessibility tools, link previews, and non-WASM clients with very little to understand. I would make publication a hybrid:

    The server emits ordinary semantic HTML containing the canvas title, description, authorship, notes, annotations, source links, meaningful edge labels, application descriptions, and a readable linear fallback.
    GPUI Whiteboard WASM hydrates that page into the full pan-and-zoom spatial interface.
    The same durable canvas model drives both representations.
    A browser without WASM can still read the work and follow its sources, even though it loses the spatial presentation.

That follows the architectural principle we’ve been converging on elsewhere: one durable object, multiple specialized projections.
A public canvas should be addressable below the canvas level

The canvas itself needs a stable URL, such as:

https://platinum.rs/c/housing-in-flint

But its important contents should also be independently addressable:

https://platinum.rs/c/housing-in-flint/node/water-study

https://platinum.rs/c/housing-in-flint/annotation/lead-exposure-note

https://platinum.rs/c/housing-in-flint/edge/supports-claim-14

Those routes can open the same canvas with the referenced object selected and centered. They also give search engines something more precise to index than one enormous graphical surface.

I would avoid relying only on URL fragments for this. A fragment can help the client focus an object, but path-addressable nodes and annotations are much more useful for search results, citations, sharing, and external links.

A canvas could initially live under Platinum’s domain and later support a custom domain:

https://flintwaterresearch.org

That custom domain would resolve to the same published canvas and its subordinate routes. The domain choice is independent of the editing policy.
Keep the publishing UI to the two settings you named

I think the first publishing sheet should offer exactly:
Setting	Shared behavior
Public · View only	Anyone can discover, browse, zoom, search, follow links, and interact with read-only applications. They cannot change shared canvas state.
Public · Anyone can edit	Anyone can add, move, connect, annotate, and otherwise modify the public canvas. Changes synchronize to other visitors.

I would use “Anyone can edit” in the interface rather than “permissionless.” It communicates the consequence immediately.

Internally, however, I would not represent this as one editable: true/false flag. Represent it as capability bundles, even though V1 exposes only two presets.

For example, the view-only preset grants public reading, searching, navigation, and safe application interaction. The open-edit preset additionally grants creating nodes, changing placement, adding annotations, creating semantic edges, and modifying canvas-contained content.

That lets you later add intermediate policies—invited editors, comment-only, approval-required, account-required—without replacing the storage model. The product remains simple now, while the underlying authority model remains honest.
“Anyone can edit” must still preserve ownership and history

The open setting should mean:

    Anyone may contribute to the shared canvas.

It should not mean:

    Anyone may irreversibly destroy its history, take ownership, or gain the permissions of embedded applications.

Every public operation should retain:

    The actor or anonymous session that performed it.
    The operation and affected objects.
    The prior and resulting revision.
    A timestamp.
    Any relationship or annotation authorship.
    Enough history to restore an earlier state.

Anonymous visitors do not need accounts for the mode to be genuinely open. They can receive ephemeral public actor identities. But their operations should still be distinguishable from one another.

Deletes should initially be soft deletions or tombstones. The publisher should always be able to restore a previous revision, pause editing, or switch the canvas back to view-only.

The principle I would use is:

Permissionless contribution, not permissionless destruction.

Rate limits, size limits, malicious-link handling, and rollback are not additional publishing modes. They are the minimum operating protections required for the open mode to exist.
Live editing and search indexing need different rhythms

A freely editable canvas may change every few seconds. Search engines should not index every transient drag, half-written note, or moment of vandalism.

I would therefore distinguish:

    The live public head, which people currently interacting with the canvas see.
    Durable public checkpoints, which are saved revisions.
    The indexed publication revision, from which the server generates searchable HTML.

When someone opens the URL, the server immediately returns semantic HTML for the latest indexed checkpoint. Once the WebAssembly client loads, it can synchronize to the newer live head.

After a quiet period, on a schedule, or after an explicit checkpoint condition, the live state becomes a new durable revision and the public HTML/search index updates.

That gives you both:

    Real-time collaborative editing.
    Stable, reproducible, indexable public pages.

It also means a search result doesn’t point to an ephemeral state that disappeared five seconds later. Revision-specific URLs could remain available for citations:

https://platinum.rs/c/housing-in-flint/revision/7f82…

The root URL would continue to point at the latest public state.
Loro and RustyRed have different jobs here

I would use the technologies in complementary ways.

Loro—or a similar CRDT layer—owns live collaborative editing mechanics: concurrent movement, text editing, annotations, edge creation, offline changes, and synchronization between browser clients.

RustyRed owns durable identity and meaning: canvases, webpage resources, semantic relationships, annotations, authorship, publication revisions, provenance, search scope, and retained history.

A CRDT operation such as “move this node” is useful for synchronizing interface state. A RustyRed relationship such as “this chart is derived from these three reports” is a durable semantic claim that should remain useful outside the whiteboard renderer.

Periodic checkpoints lower the current CRDT state into the durable RustyRed-backed canvas revision. The web renderer and native renderer both consume that durable canvas model.
View-only still allows rich interaction

I would be careful not to make “view-only” feel like a screenshot.

A visitor should still be able to:

    Pan and zoom.
    Search within the canvas.
    Expand annotations.
    Follow sources.
    Traverse meaningful relationships.
    Change their local layout or filtering without saving it.
    Operate safe read-only controls in embedded applications.
    Select an object and copy a direct link to it.

What they cannot do is mutate the shared public state.

Applications require one additional boundary. A visitor being allowed to edit a canvas does not automatically give them authority to edit the application’s source, send messages, mutate an external database, or act through the publisher’s logged-in accounts.

An application node should separately declare whether its state is:

    Read-only.
    Local to each visitor.
    Shared within the canvas.
    Connected to an external service with an explicit authority flow.

Canvas edit permission should never silently grant the fourth category.
What “publish” should mean in the product

I would make the terminology precise:

Share can eventually mean give specific people or anyone with a link access, possibly without search indexing.

Publish should mean create a public, indexable, addressable web presence.

For the simple first version, publishing always produces something discoverable. The publishing sheet could contain:

Web address
platinum.rs/c/housing-in-flint

Access

    Public · View only
    Public · Anyone can edit

Preview public version

Publish

The preview is important because the private working canvas may contain logged-in webpages, unpublished notes, private documents, local application state, or captured information that cannot be exposed publicly. Publication should generate a sanitized public projection and show exactly what will become visible.
The first complete acceptance test

I would consider the public architecture proven when this works:

    A person creates a canvas in native Platinum.
    They add webpages, annotations, a meaningful relationship, and a small interactive application.
    They publish it as Public · View only.
    An unrelated person finds it through a normal search engine and opens it in Firefox.
    The initial HTML is readable before the WASM experience finishes loading.
    The visitor zooms around, searches the canvas, opens an annotation, follows a source, and directly links to one node.
    The owner switches the canvas to Public · Anyone can edit.
    Two anonymous browser visitors concurrently add a note and connect two sources.
    Both see the changes live.
    The owner restores an earlier revision.
    The public URL remains stable throughout.
    The next indexed checkpoint exposes the updated searchable HTML.

That would establish that the canvas is simultaneously a native browsing environment, a collaborative object, and a genuine part of the ordinary web.

The core product decision is sound: expose only View only and Anyone can edit initially. Just make those two settings presets over a richer capability and revision system. That gives you radical simplicity in the publishing interface without turning the implementation into a dead end.
