const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const handlers = {}, frames = [];
const context = {innerWidth:1280, innerHeight:800, devicePixelRatio:2, scrollX:0, scrollY:0,
 document:{querySelector:()=>null, activeElement:{id:''}, cookie:''},
 performance:{now:()=>1}, addEventListener:(kind, fn)=>handlers[kind]=fn,
 requestAnimationFrame:fn=>frames.push(fn)};
context.window=context;
vm.createContext(context);
vm.runInContext(fs.readFileSync('tests/canvas/probe.js','utf8'),context);
const probe=context.canvasProbe;
probe.startTrace();
frames.shift()();
context.innerWidth=637; context.innerHeight=398;
handlers.resize();
context.innerWidth=1280; context.innerHeight=800;
handlers.resize();
frames.shift()();
let trace=probe.finishTrace();
assert.equal(trace.filter(x=>x.kind==='resize').length,2);
assert.equal(trace.find(x=>x.kind==='resize').innerWidth,637);
assert.equal(trace.find(x=>x.kind==='resize').innerHeight,398);
assert.equal(trace.at(-1).innerWidth,1280);
assert.equal(probe.snapshot().resizeEvents[0].innerWidth,637);
frames.shift()();
probe.startTrace();
context.devicePixelRatio=0.7;
frames.shift()();
trace=probe.finishTrace();
assert.equal(trace.filter(x=>x.kind==='resize').length,0);
assert(trace.every(x=>x.innerWidth===1280 && x.innerHeight===800));
assert.equal(trace.at(-1).devicePixelRatio,0.7);
console.log('PASS: transient resize dimensions retained; stable scale-only trace; trace restarts independently of document history');
