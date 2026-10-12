// Opt-in loopback measurements, never timing assertions. Admission/data/finish
// are timed for uploads; publication and byte-oracle reads run afterwards.
const transferSize = 50 * 1024 * 1024;
const transferPiece = Buffer.from(Array.from({length:64*1024}, (_,i)=>i%251));
function measuredBody() {
  let remaining = transferSize;
  return new ReadableStream({pull(controller) {
    if (!remaining) {controller.close();return;}
    controller.enqueue(transferPiece); remaining -= transferPiece.length;
  }});
}
function reportThroughput(direction, path, count, started) {
  const seconds = (performance.now()-started)/1000;
  console.log(JSON.stringify({test_progress:'transfer_complete',throughput:{
    direction,path,count,mib:50*count,seconds,mib_per_second:50*count/seconds}}));
}
async function byteOracle(response) {
  assert.equal(response.status,200);
  let received=0;
  for await (const bytes of response.body) {
    assert.deepEqual(Buffer.from(bytes),transferPiece);
    received+=bytes.length;
  }
  assert.equal(received,transferSize);
}
async function measureUploadTransport({member,memberHeaders,memberSocket,memberReceive}) {
  async function command(action,payload) {
    const request_id=crypto.randomUUID();
    memberSocket.send(JSON.stringify({op:'command',request_id,action,payload}));
    for (;;) {
      const frame=await memberReceive();
      if (frame.request_id===request_id) {assert.ok(frame.accepted);return frame.result;}
    }
  }
  for (const path of ['legacy_ack','binary_ack']) for (const count of [1,10]) {
    const fetch = path==='legacy_ack' ? (url,init)=>legacyBodyFetch(member,url,init) : (url,init)=>member.fetch(url,init);
    console.log(JSON.stringify({test_progress:'transfer_started'}));
    const started=performance.now(), completed=[];
    let next=0;
    await Promise.all(Array.from({length:Math.min(count,3)},async ()=>{
      for (;;) {
        const index=next++; if(index>=count)return;
        const response=await fetch('/api/message-attachment-transfers',{method:'POST',headers:memberHeaders,
          body:JSON.stringify({room_id:'secure-room',operation_id:crypto.randomUUID(),filename:`measured-${path}-${index}`,
            content_type:'application/octet-stream',declared_size:transferSize})});
        assert.equal(response.status,200);
        const root=`/api/message-attachment-transfers/${(await response.json()).transfer.handle}`;
        const data=await fetch(`${root}/data?offset=0`,{method:'POST',headers:{...memberHeaders,'content-type':'application/octet-stream'},
          body:measuredBody(),duplex:'half'});
        assert.equal(data.status,200); assert.equal((await data.json()).transfer.accepted_offset,transferSize);
        const finish=await fetch(`${root}/finish`,{method:'POST',headers:memberHeaders});
        assert.equal(finish.status,200); completed.push((await finish.json()).transfer.attachment);
        console.log(JSON.stringify({test_progress:'transfer_complete'}));
      }
    }));
    reportThroughput('upload',path,count,started);
    for (const attachment of completed) {
      const bound=await command('message.send',{content:'',attachment_ids:[attachment.id]});
      await byteOracle(await fetch(attachment.download_url,{headers:memberHeaders}));
      await command('message.delete',{event_id:bound.event.id});
      console.log(JSON.stringify({test_progress:'transfer_complete'}));
    }
  }
}
async function measureDownloadTransport({member,memberHeaders,attachment}) {
  // The measured fixture uses the same repeated varied piece in both directions.
  for (const path of ['legacy_ack','binary_ack']) for (const count of [1,10]) {
    console.log(JSON.stringify({test_progress:'download_batch_started'}));
    const started=performance.now();
    await Promise.all(Array.from({length:count},async ()=>{
      const response=path==='legacy_ack'
        ? await legacyBodyFetch(member,attachment.download_url,{headers:memberHeaders})
        : await member.fetch(attachment.download_url,{headers:memberHeaders});
      await byteOracle(response);
      console.log(JSON.stringify({test_progress:'download_complete'}));
    }));
    reportThroughput('download',path,count,started);
  }
}
