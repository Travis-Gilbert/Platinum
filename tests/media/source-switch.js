// Run through media_check with FRAME_MEDIA_SCRIPT pointing at this file and
// the HTTP fixture base set to Servo's existing WPT /media/ directory.
(async () => {
    const video = document.createElement('video');
    video.muted = true;
    video.controls = true;
    document.body.replaceChildren(video);
    const report = { mode: 'source-switch', cases: [], passed: false };
    const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
    let active;
    const snapshot = () => ({ current: video.currentTime, ready: video.readyState,
        paused: video.paused, ended: video.ended, seeking: video.seeking,
        duration: video.duration, width: video.videoWidth, height: video.videoHeight,
        error: video.error?.code });
    for (const event of ['loadedmetadata', 'loadeddata', 'playing', 'waiting',
        'ended', 'seeking', 'seeked', 'resize', 'error']) {
        video.addEventListener(event, () => active?.events.push({ event, ...snapshot() }));
    }
    async function until(test, description) {
        const deadline = performance.now() + 10000;
        while (!test()) {
            if (video.error) throw new Error(`MediaError ${video.error.code}`);
            if (performance.now() > deadline) throw new Error(`Timed out: ${description}`);
            await delay(25);
        }
    }
    async function playPast(position = 0.4) {
        let resolved = false, rejected;
        video.play().then(() => { resolved = true; }, error => { rejected = error; });
        await until(() => rejected || (resolved && video.currentTime > position),
            `play promise and clock past ${position}`);
        if (rejected) throw rejected;
        active.playback = snapshot();
    }
    async function check(name, run) {
        active = { name, events: [], passed: false };
        report.cases.push(active);
        try { await run(); active.passed = true; }
        catch (error) { active.error = String(error); }
        active.final = snapshot();
    }
    const source = file => { video.src = new URL(file, location.href).href; };
    await check('initial MP4', async () => { source('movie_5.mp4'); await playPast(); });
    await check('replace playing MP4 with WebM', async () => {
        source('movie_5.webm'); await playPast();
    });
    await check('replace paused WebM with MP4', async () => {
        video.pause(); source('movie_5.mp4');
        await until(() => video.readyState >= 2, 'paused replacement preroll');
        await delay(300);
        if (!video.paused || video.currentTime !== 0) throw new Error('Paused replacement started');
        await playPast();
    });
    await check('rapid replacement cancels stale sources', async () => {
        source('movie_5.webm'); source('movie_5.mp4'); source('movie_5.webm');
        await playPast();
    });
    await check('explicit load resets and plays', async () => {
        video.load(); await playPast();
    });
    await check('end position is retained and replay resolves', async () => {
        source('movie_5.mp4'); await playPast();
        await until(() => video.ended, 'ended');
        await delay(750);
        active.afterEnd = snapshot();
        if (!video.ended || Math.abs(video.currentTime - video.duration) > 0.1)
            throw new Error('Backend rewound the ended element without an explicit seek');
        await playPast();
        if (video.ended) throw new Error('Replay still ended');
    });
    await check('seek from ended and resume', async () => {
        source('movie_5.webm'); await playPast();
        video.currentTime = video.duration - 0.3;
        await until(() => video.ended, 'ended after seek');
        video.currentTime = 1;
        await until(() => !video.seeking, 'seek away from end');
        const soughtAt = video.currentTime;
        await delay(750);
        if (!video.paused || Math.abs(video.currentTime - soughtAt) > 0.1)
            throw new Error('Seeking a paused ended element started playback');
        await playPast(1.4);
    });
    for (const extension of ['mp4', 'webm']) {
        await check(`intrinsic resolution change ${extension}`, async () => {
            source(`400x300-red-resize-200x150-green.${extension}`);
            await playPast(0.1);
            await until(() => video.videoWidth === 200 && video.videoHeight === 150,
                'intrinsic resize to 200x150');
            const canvas = document.createElement('canvas');
            canvas.width = canvas.height = 1;
            const context = canvas.getContext('2d');
            // Resize metadata and the asynchronously uploaded image can land
            // in different tasks. Require the new decoded pixels to arrive.
            await until(() => {
                context.drawImage(video, 0, 0, 1, 1);
                active.pixel = Array.from(context.getImageData(0, 0, 1, 1).data);
                return active.pixel[1] >= 90 && active.pixel[0] <= 50 && active.pixel[3] === 255;
            }, 'green resized frame');
            await until(() => video.currentTime > 1.4, 'clock beyond the resize');
            await until(() => active.events.some(event => event.event === 'ended'), 'EOS after resize');
            active.afterEnd = snapshot();
            if (extension === 'mp4' && Math.abs(video.currentTime - video.duration) > 0.1)
                throw new Error('MP4 ended before its final timestamp');
            video.currentTime = 0.2;
            await until(() => !video.seeking && video.videoWidth === 400,
                'seek back across the resolution change');
            await playPast(1.4);
            await until(() => video.videoWidth === 200 && video.videoHeight === 150,
                'resize again after seek');
        });
    }
    video.pause();
    report.passed = report.cases.every(result => result.passed);
    console.log('FRAME_MEDIA_RESULT ' + JSON.stringify(report));
})();
