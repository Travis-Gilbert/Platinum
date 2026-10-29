// Login forms never enter the local search index or model context.
(() => {
    if (document.querySelector('input[type="password"], input[autocomplete="one-time-code"]')) return null;
    return [document.URL, document.contentType,
        document.documentElement ? document.documentElement.outerHTML : ''];
})()
