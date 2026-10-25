(async () => {
 document.documentElement.innerHTML = `<head><style>
 body { margin:0; background:#111; color:white; }
 svg { width:100px; height:100px; display:inline-block; }
 #inherited { fill: currentColor; }
 #styled rect { fill:rgb(0,255,0); }
 </style></head><body><svg id="inherited" viewBox="0 0 100 100"><rect width="100" height="100"/></svg><svg id="styled" viewBox="0 0 100 100"><rect width="100" height="100"/></svg><svg viewBox="0 0 100 100"><rect width="100" height="100" fill="blue"/></svg><svg viewBox="0 0 100 100"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient></defs><rect width="100" height="100" fill="url(#g)"/></svg></body>`;
 await new Promise(r=>setTimeout(r,1000));
 console.log('FRAME_MEDIA_FRAME svg-initial');
 await new Promise(r=>setTimeout(r,1000));
 document.body.style.color = 'rgb(255,0,255)';
 document.querySelector('style').textContent += '#styled rect { fill:rgb(255,255,0) }';
 await new Promise(r=>setTimeout(r,1000));
 console.log('FRAME_MEDIA_FRAME svg-restyled');
 await new Promise(r=>setTimeout(r,1000));
 console.log('FRAME_MEDIA_RESULT '+JSON.stringify({passed:true,fill:getComputedStyle(document.getElementById('inherited')).fill}));
})().catch(e=>console.log('FRAME_MEDIA_RESULT '+JSON.stringify({passed:false,error:String(e)})));
