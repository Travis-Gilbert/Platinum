// Runs inside Frame's Servo instance. Fixtures come from Servo's existing WPT
// media directory; this probe does not replace the media-source WPT suite.
(async () => {
    const types = [
        'video/mp4; codecs="avc1.42E01E, mp4a.40.2"',
        'video/webm; codecs="vp8, vorbis"',
        'video/webm; codecs="vp9, opus"',
        'video/mp4; codecs="av01.0.04M.08"',
    ];
    const probe = document.createElement('video');
    const report = {
        userAgent: navigator.userAgent,
        mediaSource: typeof MediaSource !== 'undefined',
        sourceBuffer: typeof SourceBuffer !== 'undefined',
        codecs: types.map(type => ({
            type,
            canPlayType: probe.canPlayType(type),
            mse: typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported(type),
        })),
        playback: [],
        passed: false,
    };
    const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
    async function until(test, description) {
        const deadline = performance.now() + 10000;
        while (!test()) {
            if (performance.now() > deadline) throw new Error(`Timed out: ${description}`);
            await delay(50);
        }
    }
    try {
        if (fixtureBase) {
            document.body.replaceChildren();
            for (const file of ['movie_5.mp4', 'movie_5.webm']) {
                const video = document.createElement('video');
                const result = { file, passed: false };
                report.playback.push(result);
                video.muted = true;
                video.controls = true;
                document.body.appendChild(video);
                try {
                    video.src = new URL(file, fixtureBase).href;
                    await until(() => video.readyState >= 2 || video.error, 'decoded frame');
                    if (video.error) throw new Error(`MediaError ${video.error.code}`);
                    await video.play();
                    await until(() => video.currentTime > 0.4 || video.error, 'time advancing');
                    if (video.error) throw new Error(`MediaError ${video.error.code}`);
                    video.pause();
                    const pausedAt = video.currentTime;
                    await delay(250);
                    if (Math.abs(video.currentTime - pausedAt) > 0.1) throw new Error('Pause failed');
                    video.currentTime = 2;
                    await until(() => !video.seeking && Math.abs(video.currentTime - 2) < 0.25, 'seek');
                    await video.play();
                    await until(() => video.currentTime > 2.4 || video.error, 'resume after seek');
                    if (video.error) throw new Error(`MediaError ${video.error.code}`);
                    video.pause();
                    const canvas = document.createElement('canvas');
                    canvas.width = 64;
                    canvas.height = 48;
                    const context = canvas.getContext('2d');
                    context.drawImage(video, 0, 0, 64, 48);
                    const pixels = context.getImageData(0, 0, 64, 48).data;
                    // WPT's movie_5 fixtures are solid red at this timestamp.
                    // Check pixels as well as time/metadata to catch black output.
                    result.framePixel = Array.from(pixels.slice(0, 4));
                    if (pixels[0] < 200 || pixels[1] > 40 || pixels[2] > 40 || pixels[3] < 240) {
                        throw new Error('Expected a red decoded video frame');
                    }
                    console.log('FRAME_MEDIA_FRAME ' + file);
                    await delay(500);
                    result.width = video.videoWidth;
                    result.height = video.videoHeight;
                    result.duration = video.duration;
                    result.audioTracks = video.audioTracks ? video.audioTracks.length : null;
                    result.passed = video.videoWidth > 0 && video.videoHeight > 0;
                } catch (error) {
                    result.error = String(error);
                } finally {
                    video.pause();
                    video.removeAttribute('src');
                    video.load();
                    video.remove();
                }
            }
        }
        report.passed = fixtureBase
            ? report.playback.length === 2 && report.playback.every(result => result.passed)
            : report.codecs.slice(0, 2).every(codec => codec.canPlayType !== '');
    } catch (error) {
        report.error = String(error);
    }
    console.log('FRAME_MEDIA_RESULT ' + JSON.stringify(report));
})();
