(async () => {
  const result = {mode: "mse", steps: [], passed: false};
  const report = () => console.log("FRAME_MEDIA_RESULT " + JSON.stringify(result));
  const wait = (target, event, act) => new Promise((resolve, reject) => {
    const timer = setTimeout(() => finish(new Error("Timeout: " + event)), 12000);
    const success = () => finish();
    const failure = () => finish(new Error("Error waiting for " + event));
    function finish(error) {
      clearTimeout(timer);
      target.removeEventListener(event, success);
      target.removeEventListener("error", failure);
      error ? reject(error) : resolve();
    }
    target.addEventListener(event, success, {once: true});
    target.addEventListener("error", failure, {once: true});
    try { if (act) act(); } catch (error) { finish(error); }
  });
  const until = async (test, label) => {
    const end = Date.now() + 12000;
    while (!test()) {
      if (Date.now() > end) throw new Error("Timeout: " + label);
      await new Promise(resolve => setTimeout(resolve, 50));
    }
  };
  let video;
  try {
    document.body.textContent = "";
    video = document.createElement("video");
    video.muted = true;
    video.controls = true;
    document.body.appendChild(video);
    const source = new MediaSource();
    await wait(source, "sourceopen", () => { video.src = URL.createObjectURL(source); });
    result.steps.push("sourceopen");
    const tracks = [
      ['video/webm; codecs="vp8"', "webm/test-v-128k-320x240-30fps-10kfr.webm"],
      ['audio/webm; codecs="vorbis"', "webm/test-a-192k-44100Hz-1ch.webm"]
    ];
    const buffers = tracks.map(([type]) => {
      if (!MediaSource.isTypeSupported(type)) throw new Error("Unsupported: " + type);
      return source.addSourceBuffer(type);
    });
    for (const [index, [type, file]] of tracks.entries()) {
      const buffer = buffers[index];
      const response = await fetch(new URL(file, fixtureBase));
      if (!response.ok) throw new Error("Fixture HTTP " + response.status);
      const data = await response.arrayBuffer();
      for (let offset = 0; offset < data.byteLength; offset += 32768) {
        await wait(buffer, "updateend", () => buffer.appendBuffer(data.slice(offset, offset + 32768)));
      }
      if (!buffer.buffered.length) throw new Error("Empty buffered range: " + type);
      result.steps.push({type, bytes: data.byteLength, buffered: [buffer.buffered.start(0), buffer.buffered.end(0)]});
    }
    source.endOfStream();
    await video.play();
    await until(() => video.currentTime > 0.5 && video.videoWidth > 0, "playing");
    result.steps.push({playing: video.currentTime, width: video.videoWidth, height: video.videoHeight});
    const beforeRepeatPlay = video.currentTime;
    await video.play();
    await until(() => video.currentTime > beforeRepeatPlay + 0.2, "repeated play stays playing");
    result.steps.push({repeatedPlay: video.currentTime});
    video.pause();
    await wait(video, "seeked", () => { video.currentTime = 0.75; });
    await video.play();
    await until(() => video.currentTime > 1.2, "resumed after seek");
    video.pause();
    result.steps.push({seekAndResume: video.currentTime});
    console.log("FRAME_MEDIA_FRAME mse");
    await new Promise(resolve => setTimeout(resolve, 300));
    result.passed = true;
  } catch (error) {
    result.error = String(error);
    if (video) result.video = {currentTime: video.currentTime, readyState: video.readyState, networkState: video.networkState, error: video.error && video.error.code};
  }
  report();
})();
