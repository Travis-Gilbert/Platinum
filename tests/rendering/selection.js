(async () => {
 // Three lines whose selections start and end at different columns, so every
 // kind of corner appears: rounded, square, and filleted.
 document.documentElement.innerHTML = `<head><style>
 body { margin:0; background:#fff; }
 p { margin:20px 40px; font:20px/30px monospace; color:transparent; }
 ::selection { background:rgb(0,0,255); }
 </style></head><body><p>abcdefghij<br>abcdefghijklmnop<br>abcdefgh</p></body>`;
 const lines = [...document.querySelector('p').childNodes].filter(node => node.nodeType === Node.TEXT_NODE);
 document.getSelection().setBaseAndExtent(lines[0], 4, lines[2], 6);
 await new Promise(r=>setTimeout(r,1000));
 console.log('FRAME_MEDIA_FRAME selection');
 await new Promise(r=>setTimeout(r,1000));

 // The same shape over prose, for looking at.
 document.documentElement.innerHTML = `<head><style>
 body { margin:0; background:#1e1f24; }
 p { margin:24px 32px; width:560px; font:16px/26px -apple-system, sans-serif; color:#e6e6e6; }
 code { font:14px monospace; }
 ::selection { background:rgb(46,72,112); }
 </style></head><body><p>The profile suite still passes. The application test command stopped before compiling because this worktree lacks the patched MSE runtime; I’ll link the existing matching runtime from the primary dependency directory. I’ll also revert the unrelated <code>src/downloads.rs</code> formatting churn caused by package-wide rustfmt.</p></body>`;
 const prose = document.querySelector('p');
 document.getSelection().setBaseAndExtent(prose.firstChild, 4, prose.lastChild, prose.lastChild.length - 10);
 await new Promise(r=>setTimeout(r,1000));
 console.log('FRAME_MEDIA_FRAME selection-prose');
 await new Promise(r=>setTimeout(r,1000));
 console.log('FRAME_MEDIA_RESULT '+JSON.stringify({passed:true}));
})().catch(e=>console.log('FRAME_MEDIA_RESULT '+JSON.stringify({passed:false,error:String(e)})));
