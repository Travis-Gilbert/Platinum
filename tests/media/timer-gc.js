// Exercise coalesced timer callbacks while allocations trigger garbage collection.
// Pending callbacks and captured arguments must remain rooted across earlier callbacks.
(() => {
  let completed = 0;
  const count = 256;
  let checksum = 0;
  for (let i = 0; i < count; i++) {
    const capture = {index: i};
    setTimeout(() => {
      const garbage = Array.from({length: 20000}, (_, j) => ({value: j + capture.index}));
      checksum += garbage[0].value;
      completed++;
      if (completed === count) console.log('FRAME_MEDIA_RESULT ' + JSON.stringify({mode: 'timer-gc', completed, checksum, passed: checksum === count * (count - 1) / 2}));
    }, 0);
  }
})();
