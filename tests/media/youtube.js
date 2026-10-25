// Live smoke diagnostic only: advancing playback may be an ad, and does not
// prove the requested video's identity, audible output, or Chrome parity.
(async () => {
    const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
    const report = {
        title: document.title,
        mediaSource: typeof MediaSource !== 'undefined',
        sourceBuffer: typeof SourceBuffer !== 'undefined',
        passed: false,
    };
    let video;
    try {
        for (let i = 0; i < 150; i++) {
            video = document.querySelector('video');
            if (video) break;
            await delay(100);
        }
        if (!video) throw new Error('No video element appeared on the watch page');
        video.muted = true;
        video.play().catch(error => { report.playError = String(error); });
        const initialTime = video.currentTime;
        await delay(10000);
        Object.assign(report, {
            initialTime,
            currentTime: video.currentTime,
            readyState: video.readyState,
            networkState: video.networkState,
            paused: video.paused,
            width: video.videoWidth,
            height: video.videoHeight,
            errorCode: video.error ? video.error.code : null,
            sourceScheme: video.currentSrc ? new URL(video.currentSrc).protocol : null,
        });
        if (!(video.currentTime > initialTime + 1 && video.videoWidth > 0 && !video.error))
            throw new Error('Playback did not advance');
        const player = document.getElementById('movie_player');
        report.videoId = player && player.getVideoData ? player.getVideoData().video_id : null;
        report.adShowing = !!(player && player.classList.contains('ad-showing'));
        report.duration = video.duration;
        if (report.adShowing || report.videoId !== new URL(location.href).searchParams.get('v'))
            throw new Error('Requested video identity was not confirmed');
        video.pause();
        await delay(300);
        const pausedAt = video.currentTime;
        await delay(400);
        if (Math.abs(video.currentTime - pausedAt) > 0.1) throw new Error('Pause did not hold');
        report.pause = {at: pausedAt, held: true};
        const seekTo = Math.min(60, video.duration / 2);
        video.currentTime = seekTo;
        video.play().catch(error => { report.playError = String(error); });
        const deadline = Date.now() + 15000;
        while (video.currentTime < seekTo + 1 || video.readyState < 3) {
            if (Date.now() > deadline || video.error) throw new Error('Seek and resume failed');
            await delay(100);
        }
        report.seek = {target: seekTo, resumedAt: video.currentTime};
        video.playbackRate = 1.5;
        const rateChangeStart = video.currentTime;
        const rateDeadline = Date.now() + 10000;
        while (video.currentTime < rateChangeStart + 0.5) {
            if (Date.now() > rateDeadline) throw new Error('Playback did not resume after rate change');
            await delay(100);
        }
        const speedStart = video.currentTime;
        const wallStart = performance.now();
        await delay(4000);
        const elapsed = (performance.now() - wallStart) / 1000;
        const advanced = video.currentTime - speedStart;
        report.speed = {rate: video.playbackRate, advanced, elapsed, observedRate: advanced / elapsed};
        if (report.speed.observedRate < 1.2 || report.speed.observedRate > 1.8)
            throw new Error('Playback speed did not take effect');
        video.playbackRate = 1;
        report.passed = true;
    } catch (error) {
        report.error = String(error);
        if (video) report.failureState = {time: video.currentTime, readyState: video.readyState, seeking: video.seeking, paused: video.paused, error: video.error && video.error.code};
    }
    report.trace = window.__frameMediaTrace || [];
    report.sources = (window.__frameMediaSources || []).map(source => ({state: source.readyState, duration: source.duration, buffers: source.sourceBuffers.length, active: source.activeSourceBuffers.length}));
    console.log('FRAME_MEDIA_FRAME youtube');
    await delay(500);
    console.log('FRAME_MEDIA_RESULT ' + JSON.stringify(report));
})();
