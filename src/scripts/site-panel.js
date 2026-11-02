// What the site panel shows about the page: head metadata and the names of
// the fonts in use. Never page text: the fonts are probed with a fixed
// string, and nothing here reaches the search index or model context.
(() => {
    const safe = (read, fallback) => {
        try {
            return read();
        } catch (_) {
            return fallback;
        }
    };
    const text = (value) => (value || '').replace(/\s+/g, ' ').trim();
    const meta = (selector) => {
        const element = document.querySelector(selector);
        return element ? text(element.getAttribute('content')) : '';
    };

    // Any CSS color Servo parses, as the bytes it paints. A value it rejects
    // leaves the fill style at the sentinel, so two sentinels tell it apart.
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    const rgb = (value) => {
        context.fillStyle = '#010203';
        context.fillStyle = value;
        const first = context.fillStyle;
        context.fillStyle = '#040506';
        context.fillStyle = value;
        if (first !== context.fillStyle) return null;
        context.clearRect(0, 0, 1, 1);
        context.fillRect(0, 0, 1, 1);
        const [r, g, b, a] = context.getImageData(0, 0, 1, 1).data;
        return a === 0 ? null : [r, g, b];
    };

    // The first meta theme-color whose media query matches, as authored for
    // light or dark schemes.
    const themeColor = () => {
        for (const element of document.querySelectorAll('meta[name="theme-color" i]')) {
            const media = element.getAttribute('media');
            if (media && !matchMedia(media).matches) continue;
            const color = rgb(text(element.getAttribute('content')));
            if (color) return color;
        }
        return null;
    };

    // A family is in use when it is a web font the page has loaded, or, for
    // any other name, when text set in it measures differently from every
    // generic fallback alone. Servo's FontFaceSet has no check() or status,
    // but it lists the faces of the page's @font-face rules, each with a
    // family and a load status.
    const KEYWORDS = new Set(['serif', 'sans-serif', 'monospace', 'cursive', 'fantasy',
        'system-ui', 'ui-serif', 'ui-sans-serif', 'ui-monospace', 'ui-rounded', 'math',
        'emoji', 'fangsong']);
    const ALIASES = new Set(['-apple-system', 'blinkmacsystemfont']);
    const SAMPLE = 'mmmmmmmmmmlliWWQ@#0123456789';
    const unquote = (family) => family.trim().replace(/^["']|["']$/g, '');
    const measure = (font) => {
        context.font = `72px ${font}`;
        return context.measureText(SAMPLE).width;
    };
    const fallbacks = ['serif', 'sans-serif', 'monospace'].map((base) => [base, measure(base)]);
    const installed = (name) => {
        const quoted = `"${name.replace(/["\\]/g, '\\$&')}"`;
        return fallbacks.some(([base, width]) => measure(`${quoted}, ${base}`) !== width);
    };
    const webFaces = [];
    safe(() => document.fonts.forEach((face) => webFaces.push([unquote(face.family).toLowerCase(), face.status])), null);
    const inUse = (name) => {
        const key = name.toLowerCase();
        if (KEYWORDS.has(key) || ALIASES.has(key)) return false;
        const declared = webFaces.filter(([family]) => family === key);
        return declared.length ? declared.some(([, status]) => status === 'loaded') : installed(name);
    };

    // The family list of a computed font-family, quotes removed.
    const families = (list) => (list.match(/"[^"]*"|'[^']*'|[^,]+/g) || [])
        .map(unquote)
        .filter(Boolean);
    // The first family in use; failing that, the generic the page falls back
    // to, or the first family it names.
    const used = (element) => {
        const list = families(getComputedStyle(element).fontFamily);
        return list.find(inUse)
            || list.find((family) => KEYWORDS.has(family.toLowerCase()))
            || list[0] || '';
    };

    const rendered = (element) => element.getClientRects().length > 0;
    const withText = (element) => rendered(element) && text(element.textContent).length > 0;
    const heading = () => ['h1', 'h2', 'h3']
        .map((tag) => [...document.querySelectorAll(tag)].find(withText))
        .find(Boolean);
    // The paragraph with the most text among the first few, so a caption or a
    // cookie notice does not stand for the page's body.
    const body = () => {
        const best = [...document.querySelectorAll('p')].filter(rendered).slice(0, 30)
            .reduce((most, paragraph) => (!most || paragraph.textContent.length > most.textContent.length
                ? paragraph : most), null);
        return best || document.body;
    };
    const code = () => [...document.querySelectorAll('pre code, pre, code')].find(withText);

    const fonts = () => {
        const found = [];
        for (const [role, element] of [['headings', heading], ['body', body], ['code', code]]) {
            const family = safe(() => {
                const target = element();
                return target ? used(target) : '';
            }, '');
            if (!family) continue;
            const same = found.find((font) => font.family.toLowerCase() === family.toLowerCase());
            if (same) same.roles.push(role);
            else found.push({ family, roles: [role] });
        }
        return found;
    };

    return JSON.stringify({
        url: document.URL,
        title: text(document.title) || meta('meta[property="og:title" i]'),
        description: meta('meta[name="description" i]') || meta('meta[property="og:description" i]'),
        themeColor: safe(themeColor, null),
        fonts: safe(fonts, []),
    });
})()
