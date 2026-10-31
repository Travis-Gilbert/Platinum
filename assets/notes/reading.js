/* The reading view's page script. The host (src/notes) renders a note to HTML
 * and sends it here as a patch; this script morphs it into the page, runs the
 * post-processors Obsidian runs on a rendered note (math, code, diagrams), and
 * tells the host what the person did (a link, a task, a fold) through
 * navigation requests, which the host's delegate answers and refuses.
 *
 * Nothing here is fetched from outside the frame-notes: scheme. */
(() => {
  "use strict";

  const config = JSON.parse(document.getElementById("frame-notes-config").textContent);
  const root = new URL("/app/", location.href).href.replace(/\/$/, "");
  const vendor = `${root}/vendor`;
  const state = {
    // The authority revision of the text the page shows; a patch rendered from
    // an older one is dropped.
    revision: config.revision,
    // Identifies the html and classes the page shows; the host sends it back
    // in `ready` so it can tell whether the page already has a rendering.
    digest: config.digest,
    // Callout folds, as the host last told us, and the ones we toggled that
    // the host has not yet counted (`foldSeq` numbers them).
    folds: new Map(Object.entries(config.folds)),
    unacknowledged: new Map(),
    foldSeq: 0,
    mermaid: config.mermaid,
  };

  // ---- Page structure -------------------------------------------------

  const view = document.querySelector(".markdown-preview-view");
  const sizer = document.querySelector(".markdown-preview-sizer");
  const PUSHER = '<div class="markdown-preview-pusher" style="width: 1px; height: 0.1px; margin-bottom: 0px;"></div>';
  const BASE_CLASSES = [...view.classList];

  // The vault's own scheme wins; otherwise the app's theme (`chrome`, which the
  // host updates through `frameNotes.chrome`), and only without either the
  // system's.
  const dark = matchMedia("(prefers-color-scheme: dark)");
  let chromeScheme = config.chrome;
  const applyTheme = () => {
    const scheme = config.theme !== "system" ? config.theme : (chromeScheme ?? (dark.matches ? "dark" : "light"));
    document.body.classList.toggle("theme-dark", scheme === "dark");
    document.body.classList.toggle("theme-light", scheme !== "dark");
  };
  applyTheme();
  dark.addEventListener("change", applyTheme);

  // ---- Talking to the host --------------------------------------------

  // A navigation set while another is starting replaces it, so messages go one
  // at a time.
  const outbox = [];
  let sending = false;
  const post = (verb, params = {}) => {
    const url = new URL(`/act/${verb}`, location.href);
    url.searchParams.set("t", config.token);
    for (const [key, value] of Object.entries(params)) url.searchParams.set(key, String(value));
    outbox.push(url.href);
    if (!sending) flush();
  };
  const flush = () => {
    const next = outbox.shift();
    sending = next !== undefined;
    if (!sending) return;
    location.href = next;
    setTimeout(flush, 30);
  };

  // ---- Libraries, loaded when a note first needs them ------------------

  const loaded = new Map();
  const load = (src) => {
    if (!loaded.has(src)) {
      loaded.set(src, new Promise((resolve, reject) => {
        const script = document.createElement("script");
        script.src = src;
        script.onload = resolve;
        script.onerror = () => reject(new Error(`could not load ${src}`));
        document.head.append(script);
      }));
    }
    return loaded.get(src);
  };

  const mathjax = () => {
    window.MathJax = {
      startup: { typeset: false },
      options: { enableMenu: false },
      loader: { paths: { mathjax: `${vendor}/mathjax`, fonts: `${vendor}/mathjax/font` }, load: [] },
      chtml: {
        fontURL: `${vendor}/mathjax/font/woff2`,
        dynamicPrefix: `${vendor}/mathjax/font/dynamic`,
      },
    };
    return load(`${vendor}/mathjax/tex-chtml.js`).then(() => MathJax.startup.promise);
  };

  const prism = () => {
    window.Prism = { manual: true };
    return load(`${vendor}/prismjs/components/prism-core.min.js`)
      .then(() => load(`${vendor}/prismjs/plugins/prism-autoloader.min.js`))
      .then(() => {
        Prism.plugins.autoloader.languages_path = `${vendor}/prismjs/components/`;
        Prism.hooks.add("complete", ({ element }) => {
          element.classList.add("is-loaded");
          addCopyButton(element);
        });
      });
  };
  let prismReady, mathReady, mermaidReady;

  // ---- Ids from source -------------------------------------------------

  // FNV-1a: the same source gives the same id, so a patch can tell whether a
  // formula, a code block or a diagram changed.
  const hash = (text) => {
    let h = 0x811c9dc5;
    for (let i = 0; i < text.length; i++) {
      h ^= text.charCodeAt(i);
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h.toString(16).padStart(8, "0");
  };

  const MATH = ".math:not(.is-loaded)";
  const CODE = "pre:not(.frontmatter) > code[class*='language-']";

  // What a formula was typeset from. A rendered formula's text is MathJax's,
  // so its source is kept here, not in the page's markup.
  const sources = new WeakMap();

  // The elements whose output is expensive or stateful, each with an id made
  // of what it was rendered from and how many identical ones came before it.
  const sourceElements = (container) => {
    const seen = new Map();
    const found = [];
    const add = (element, kind, source) => {
      const base = `${kind}-${hash(source)}`;
      const n = seen.get(base) ?? 0;
      seen.set(base, n + 1);
      found.push([element, `${base}-${n}`]);
    };
    for (const math of container.querySelectorAll(".math")) {
      const source = sources.get(math) ?? (math.classList.contains("is-loaded") ? null : math.textContent);
      if (source !== null) add(math, math.classList.contains("math-block") ? "math-block" : "math-inline", source);
    }
    for (const code of container.querySelectorAll("pre > code[class*='language-']")) {
      const language = /language-([\w-]+)/.exec(code.className)?.[1] ?? "";
      const mermaid = language === "mermaid";
      add(mermaid ? code.closest(".mermaid-wrapper") ?? code.parentElement : code.parentElement, mermaid ? "mermaid" : `code-${language}`, code.textContent);
    }
    return found;
  };

  // ---- Post-processors -------------------------------------------------

  const COPY_ICON =
    '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="svg-icon lucide-copy"><rect x="8" y="8" width="14" height="14" rx="2" ry="2"></rect><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"></path></svg>';

  const addCopyButton = (code) => {
    const next = code.nextElementSibling;
    if (next?.classList.contains("copy-code-button")) return;
    if (code.closest(".frontmatter")) return;
    const button = document.createElement("button");
    button.className = "copy-code-button";
    button.innerHTML = COPY_ICON;
    code.after(button);
  };

  const typesetMath = async (container) => {
    const pending = [...container.querySelectorAll(MATH)];
    if (!pending.length) return;
    mathReady ??= mathjax();
    try {
      await mathReady;
    } catch (error) {
      console.error(error.message);
      return;
    }
    for (const element of pending) {
      if (!element.isConnected) continue;
      const tex = element.textContent;
      sources.set(element, tex);
      const output = MathJax.tex2chtml(tex, { display: element.classList.contains("math-block") });
      element.replaceChildren(output);
      element.classList.add("is-loaded");
    }
    MathJax.startup.document.clear();
    MathJax.startup.document.updateDocument();
  };

  const highlightCode = async (container) => {
    const pending = [...container.querySelectorAll(CODE)].filter(
      (code) => !code.classList.contains("is-loaded") && !code.classList.contains("language-mermaid"),
    );
    if (!pending.length) return;
    prismReady ??= prism();
    try {
      await prismReady;
    } catch (error) {
      console.error(error.message);
      return;
    }
    for (const code of pending) {
      if (!code.isConnected) continue;
      Prism.highlightElement(code);
      // A language the autoloader does not know leaves the text as it was.
      if (!code.classList.contains("is-loaded")) {
        code.classList.add("is-loaded");
        addCopyButton(code);
      }
    }
  };

  // A diagram's source is untrusted vault content, so a vault gets its
  // diagrams only after the person allows them (the host remembers it).
  const wrapMermaid = (container) => {
    for (const code of container.querySelectorAll("pre > code.language-mermaid")) {
      const pre = code.parentElement;
      if (pre.closest(".mermaid-wrapper")) continue;
      const wrapper = document.createElement("div");
      wrapper.className = "mermaid-wrapper";
      if (!state.mermaid) {
        wrapper.classList.add("is-guarded");
        wrapper.innerHTML =
          '<div class="mermaid-guard-header"><div class="mermaid-guard-text"><div class="mermaid-guard-title">Display Mermaid diagrams in this vault?</div><div class="mermaid-guard-description">Only allow if you trust this vault\'s contents.</div></div><div class="mermaid-guard-actions"><button>Allow</button></div></div><div class="mermaid-guard-source"></div>';
        wrapper.querySelector(".mermaid-guard-source").append(pre.cloneNode(true));
      } else {
        wrapper.append(pre.cloneNode(true));
      }
      const outer = document.createElement("div");
      outer.append(wrapper);
      pre.replaceWith(outer);
    }
  };

  const renderMermaid = async (container) => {
    if (!state.mermaid) return;
    const pending = [...container.querySelectorAll(".mermaid-wrapper:not(.is-guarded):not(.is-loaded)")];
    if (!pending.length) return;
    mermaidReady ??= load(`${vendor}/mermaid/mermaid.min.js`).then(() => {
      mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark.matches ? "dark" : "default" });
    });
    try {
      await mermaidReady;
    } catch (error) {
      console.error(error.message);
      return;
    }
    for (const wrapper of pending) {
      const code = wrapper.querySelector("code.language-mermaid");
      if (!code || !wrapper.isConnected) continue;
      const source = code.textContent;
      try {
        const { svg } = await mermaid.render(`mermaid-${hash(source)}-${Math.random().toString(36).slice(2, 8)}`, source);
        const diagram = document.createElement("div");
        diagram.className = "mermaid-diagram";
        diagram.innerHTML = svg;
        // Keep the source: the next patch compares against it.
        code.parentElement.hidden = true;
        wrapper.append(diagram);
      } catch (error) {
        wrapper.classList.add("is-error");
        const message = document.createElement("div");
        message.className = "mermaid-error";
        message.textContent = String(error?.message ?? error);
        wrapper.append(message);
      }
      wrapper.classList.add("is-loaded");
    }
  };

  // ---- Callout folds ---------------------------------------------------

  const foldKeys = (container) => {
    const seen = new Map();
    const keys = new Map();
    for (const callout of container.querySelectorAll(".callout.is-collapsible")) {
      const title = callout.querySelector(".callout-title-inner")?.textContent ?? "";
      const base = `${callout.dataset.callout}|${title}`;
      const n = seen.get(base) ?? 0;
      seen.set(base, n + 1);
      keys.set(callout, `${base}#${n}`);
    }
    return keys;
  };

  const setFold = (callout, collapsed) => {
    callout.classList.toggle("is-collapsed", collapsed);
    callout.querySelector(":scope > .callout-title > .callout-fold")?.classList.toggle("is-collapsed", collapsed);
    const content = callout.querySelector(":scope > .callout-content");
    if (content) content.style.display = collapsed ? "none" : "";
    if (content && !content.getAttribute("style")) content.removeAttribute("style");
  };

  const applyFolds = (container) => {
    for (const [callout, key] of foldKeys(container)) {
      const folded = state.unacknowledged.get(key)?.collapsed ?? state.folds.get(key) ?? callout.dataset.calloutFold === "-";
      setFold(callout, folded);
    }
  };

  // ---- Patches ---------------------------------------------------------

  const parse = (html) => {
    const template = document.createElement("template");
    template.innerHTML = html;
    // Scripts and event handlers never come from a note.
    for (const script of template.content.querySelectorAll("script")) script.remove();
    for (const element of template.content.querySelectorAll("*")) {
      for (const name of element.getAttributeNames()) if (name.startsWith("on")) element.removeAttribute(name);
    }
    return template.content;
  };

  // Output that is already on the page for the same source is carried over, so
  // the morph finds nothing to change in it and it keeps its rendering.
  const reuse = (incoming, live) => {
    const rendered = new Map(sourceElements(live).map(([element, id]) => [id, element]));
    for (const [element, id] of sourceElements(incoming)) {
      const existing = rendered.get(id);
      if (!existing || existing === element) continue;
      const copy = existing.cloneNode(true);
      if (sources.has(existing)) sources.set(copy, sources.get(existing));
      element.replaceWith(copy);
    }
  };

  const applyClasses = (cssclasses) => {
    view.className = [...BASE_CLASSES, ...cssclasses].join(" ");
  };

  // The host's count of the folds, and the highest fold of ours it has counted.
  const adoptFolds = (folds, foldAck) => {
    state.folds = new Map(Object.entries(folds));
    for (const [key, { seq }] of state.unacknowledged) if (seq <= foldAck) state.unacknowledged.delete(key);
  };

  // Returns whether the patch was applied: one rendered from an older revision
  // than the page shows is dropped.
  const patch = ({ revision, digest, html, cssclasses, folds, foldAck }) => {
    if (revision < state.revision) return false;
    state.revision = revision;
    state.digest = digest;
    adoptFolds(folds, foldAck);
    const incoming = parse(PUSHER + html);
    wrapMermaid(incoming);
    reuse(incoming, sizer);
    Idiomorph.morph(sizer, incoming, { morphStyle: "innerHTML", ignoreActiveValue: true });
    applyClasses(cssclasses);
    afterRender(sizer);
    return true;
  };

  // How many post-processors are still working, so the host's checks can tell
  // a page that is done from one that has yet to typeset its formulas.
  let busy = 0;
  const track = (work) => {
    busy++;
    work.catch((error) => console.error(error?.message ?? error)).finally(() => busy--);
  };

  const afterRender = (container) => {
    applyFolds(container);
    track(typesetMath(container));
    track(highlightCode(container));
    track(renderMermaid(container));
  };

  // ---- Clicks ----------------------------------------------------------

  const headingTarget = (hash) => {
    if (!hash) return null;
    const text = decodeURIComponent(hash.replace(/^#/, ""));
    if (text.startsWith("^")) return document.getElementById(text) ?? document.querySelector(`[id="${CSS.escape(text)}"]`);
    for (const heading of document.querySelectorAll("[data-heading]")) {
      if (heading.dataset.heading.toLowerCase() === text.toLowerCase()) return heading;
    }
    return document.getElementById(text);
  };

  const scrollToHash = (hash) => {
    const target = headingTarget(hash);
    if (target) target.scrollIntoView({ block: "start" });
    return Boolean(target);
  };

  // What a click follows: a link in the text (`a`) or a link in the
  // properties, which the renderer draws as a `div` carrying `data-href`.
  const LINK = "a, .metadata-link-inner[data-href]";
  const INTERNAL_LINK = "a.internal-link, .metadata-link-inner.internal-link";

  const followLink = (link) => {
    const anchor = link.localName === "a";
    const href = (anchor ? link.getAttribute("href") : link.dataset.href) ?? "";
    const absolute = anchor ? link.href : href;
    if (link.classList.contains("internal-link")) {
      const target = link.dataset.href ?? href;
      if (target.startsWith("#")) scrollToHash(target);
      else post("open", { link: target, from: config.path, unresolved: link.classList.contains("is-unresolved") ? 1 : 0 });
    } else if (href.startsWith("#")) {
      scrollToHash(href);
    } else if (/^(https?|mailto):/i.test(absolute)) {
      post("web", { url: absolute });
    }
  };

  document.addEventListener(
    "click",
    (event) => {
      const link = event.target.closest(LINK);
      if (link) {
        event.preventDefault();
        followLink(link);
        return;
      }
      const copy = event.target.closest(".copy-code-button");
      if (copy) {
        const code = copy.parentElement.querySelector("code");
        post("copy", { text: code?.textContent ?? "" });
        copy.classList.add("is-copied");
        setTimeout(() => copy.classList.remove("is-copied"), 1200);
        return;
      }
      if (event.target.closest(".mermaid-guard-actions button")) {
        post("mermaid-allow");
        return;
      }
      const title = event.target.closest(".callout.is-collapsible > .callout-title");
      if (title) {
        const callout = title.parentElement;
        const key = foldKeys(sizer).get(callout);
        const collapsed = !callout.classList.contains("is-collapsed");
        setFold(callout, collapsed);
        const seq = ++state.foldSeq;
        state.unacknowledged.set(key, { collapsed, seq });
        post("fold", { key, collapsed: collapsed ? 1 : 0, seq });
      }
    },
    true,
  );

  document.addEventListener("change", (event) => {
    const box = event.target;
    if (!box.matches?.("input.task-list-item-checkbox")) return;
    const section = box.closest("[data-sourcepos]");
    const start = /^(\d+):/.exec(section?.dataset.sourcepos ?? "");
    const line = box.dataset.line;
    if (!start || line === undefined) return;
    post("task", { line: Number(start[1]) - 1 + Number(line), checked: box.checked ? 1 : 0, revision: state.revision });
  });

  // ---- Page previews ---------------------------------------------------

  let popover = null;
  let hoverTimer = 0;
  let hideTimer = 0;
  const hidePopover = () => {
    clearTimeout(hoverTimer);
    popover?.remove();
    popover = null;
  };

  const showPreview = async (link) => {
    const target = link.dataset.href ?? "";
    if (!target || target.startsWith("#") || link.classList.contains("is-unresolved")) return;
    const url = new URL("/preview", location.href);
    url.searchParams.set("from", config.path);
    url.searchParams.set("link", target);
    let html;
    try {
      const response = await fetch(url);
      if (!response.ok) return;
      html = await response.text();
    } catch {
      return;
    }
    if (!link.matches(":hover") && !popover?.matches(":hover")) return;
    hidePopover();
    popover = document.createElement("div");
    popover.className = "popover hover-popover";
    popover.innerHTML =
      '<div class="markdown-embed is-loaded"><div class="markdown-embed-content"><div class="markdown-preview-view markdown-rendered"><div class="markdown-preview-sizer markdown-preview-section"></div></div></div></div>';
    const body = popover.querySelector(".markdown-preview-sizer");
    body.append(parse(html));
    document.body.append(popover);
    popover.addEventListener("mouseleave", () => (hideTimer = setTimeout(hidePopover, 150)));
    popover.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    const box = link.getBoundingClientRect();
    const width = popover.offsetWidth;
    const height = popover.offsetHeight;
    const left = Math.max(8, Math.min(box.left, innerWidth - width - 8));
    const below = box.bottom + 6;
    const top = below + height > innerHeight - 8 ? Math.max(8, box.top - height - 6) : below;
    popover.style.left = `${left}px`;
    popover.style.top = `${top}px`;
    afterRender(body);
  };

  document.addEventListener("mouseover", (event) => {
    const link = event.target.closest?.(INTERNAL_LINK);
    if (!link) return;
    clearTimeout(hoverTimer);
    clearTimeout(hideTimer);
    hoverTimer = setTimeout(() => showPreview(link), 350);
  });
  document.addEventListener("mouseout", (event) => {
    const link = event.target.closest?.(INTERNAL_LINK);
    if (!link) return;
    clearTimeout(hoverTimer);
    hideTimer = setTimeout(() => {
      if (!popover?.matches(":hover")) hidePopover();
    }, 250);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") hidePopover();
  });

  // ---- The host's handle on the page ----------------------------------

  window.frameNotes = {
    patch,
    // The person allowed diagrams: turn the guards into diagrams.
    allowMermaid() {
      state.mermaid = true;
      for (const wrapper of sizer.querySelectorAll(".mermaid-wrapper.is-guarded")) {
        wrapper.classList.remove("is-guarded");
        wrapper.querySelector(".mermaid-guard-header")?.remove();
        const source = wrapper.querySelector(".mermaid-guard-source");
        if (source) {
          wrapper.append(...source.childNodes);
          source.remove();
        }
      }
      renderMermaid(sizer);
    },
    // The remembered folds of a note, put on a page that has just loaded.
    restore({ folds, foldAck }) {
      adoptFolds(folds, foldAck);
      applyFolds(sizer);
    },
    // The app's theme changed: its colors (the host's `:root` rule, as text)
    // and whether it is dark.
    chrome({ css, dark: isDark }) {
      let style = document.getElementById("frame-notes-chrome");
      if (!style) {
        style = document.createElement("style");
        style.id = "frame-notes-chrome";
        document.head.append(style);
      }
      style.textContent = css;
      chromeScheme = isDark ? "dark" : "light";
      applyTheme();
    },
    // What the page shows now, for the host's checks, and whether it is done
    // post-processing it.
    shown: () => state.revision,
    idle: () => busy === 0,
    // The ids of the formulas, code blocks and diagrams on the page.
    ids: () => sourceElements(sizer).map(([, id]) => id),
  };

  // ---- First render ----------------------------------------------------

  // The shell carries the note's rendered HTML in its config, not in the
  // markup; it goes onto the page here, and the post-processors take it from
  // there, as they do after a patch.
  sizer.append(parse(PUSHER + config.html));
  applyClasses(config.cssclasses);
  wrapMermaid(sizer);
  afterRender(sizer);
  if (location.hash) scrollToHash(location.hash);
  // The page script is running and shows `state.revision`. Posting during the
  // load could be taken for a navigation away from it, so wait for the load.
  const ready = () => post("ready", { revision: state.revision, digest: state.digest });
  if (document.readyState === "complete") ready();
  else addEventListener("load", ready, { once: true });
})();
