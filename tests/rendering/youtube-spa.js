(async()=>{
 const wait=ms=>new Promise(r=>setTimeout(r,ms));
 const visible=()=>Array.from(document.querySelectorAll('img.ytCoreImageHost')).filter(i=>{const r=i.getBoundingClientRect();return r.width>80&&r.height>60&&r.x>innerWidth*.6&&r.x<innerWidth&&r.y<innerHeight&&r.y+r.height>0;});
 await wait(12000);
 const first=visible(), link=first.map(i=>i.closest('a')).find(a=>a&&a.href.includes('/watch?'));
 const before={count:first.length,loaded:first.filter(i=>i.complete&&i.naturalWidth>0).length};
 if(!link)throw new Error('No visible recommendation link');
 const target=new URL(link.href).searchParams.get('v');link.click();
 await wait(15000);
 const second=visible(), after={count:second.length,loaded:second.filter(i=>i.complete&&i.naturalWidth>0).length};
 const navigated=new URL(location.href).searchParams.get('v')===target;
 console.log('FRAME_MEDIA_FRAME youtube');await wait(1000);
 console.log('FRAME_MEDIA_RESULT '+JSON.stringify({passed:navigated&&after.count>0&&after.count===after.loaded,before,after,navigated}));
})().catch(e=>console.log('FRAME_MEDIA_RESULT '+JSON.stringify({passed:false,error:String(e)})));
