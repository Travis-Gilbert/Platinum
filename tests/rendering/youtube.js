// Public watch-page rendering diagnostic; no account data or image URLs.
(async () => {
    const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
    await wait(15000);
    const images = Array.from(document.querySelectorAll('img.ytCoreImageHost'))
        .map(image => {
            const rect = image.getBoundingClientRect();
            return {
                x: rect.x, y: rect.y, width: rect.width, height: rect.height,
                decoded: image.complete && image.naturalWidth > 0,
            };
        })
        .filter(rect => rect.width > 80 && rect.height > 60 && rect.x < innerWidth &&
            rect.y < innerHeight && rect.x + rect.width > 0 && rect.y + rect.height > 0);
    const report = {
        passed: images.length > 0 && images.every(image => image.decoded),
        viewport: [innerWidth, innerHeight, devicePixelRatio],
        dark: document.documentElement.hasAttribute('dark'),
        images,
        video: !!document.querySelector('video'),
    };
    console.log('FRAME_MEDIA_FRAME youtube');
    await wait(1000);
    console.log('FRAME_MEDIA_RESULT ' + JSON.stringify(report));
})().catch(error => console.log('FRAME_MEDIA_RESULT ' + JSON.stringify({passed: false, error: String(error)})));
