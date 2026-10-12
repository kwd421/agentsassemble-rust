// Opt-in loopback measurements, never timing assertions. Admission/data/finish
// are timed for uploads; publication and byte-oracle reads run afterwards.
const transferSize = 50 * 1024 * 1024;
const transferPiece = Buffer.from(Array.from({length:64*1024}, (_,i)=>i%251));
function measuredBody() {
  let remaining = transferSize;
  return new ReadableStream({pull(controller) {
    if (!remaining) {controller.close();return;}
    controller.enqueue(transferPiece); remaining -= transferPiece.length;
    if (remaining % (1024*1024) === 0) console.log(JSON.stringify({test_progress:"transfer_complete"}));
  }});
}
function reportThroughput(direction, path, count, started) {
  const seconds = (performance.now()-started)/1000;
  console.log(JSON.stringify({test_progress:'transfer_complete',throughput:{
    direction,path,count,rtt_ms:config.rtt_ms,mib:50*count,seconds,mib_per_second:50*count/seconds}}));
}
async function byteOracle(response) {
  assert.equal(response.status,200);
  let received=0;
  for await (const bytes of response.body) {
    assert.deepEqual(Buffer.from(bytes),transferPiece);
    received+=bytes.length;
    if (received % (1024*1024) === 0) console.log(JSON.stringify({test_progress:"transfer_complete"}));
  }
  assert.equal(received,transferSize);
}
async function measureUploadTransport({member,memberHeaders,memberSocket,memberReceive}) {
  linkRtt = config.rtt_ms;
  async function command(action,payload) {
    const request_id=crypto.randomUUID();
    memberSocket.send(JSON.stringify({op:'command',request_id,action,payload}));
    for (;;) {
      const frame=await memberReceive();
      if (frame.request_id===request_id) {assert.ok(frame.accepted);return frame.result;}
    }
  }
  for (const path of ['legacy_ack','binary_window']) for (const count of [1,10]) {
    linkRtt=config.rtt_ms;
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
    await new Promise(resolve=>setTimeout(resolve,config.rtt_ms)); linkRtt=0;
    for (const attachment of completed) {
      const bound=await command('message.send',{content:'',attachment_ids:[attachment.id]});
      await byteOracle(await fetch(attachment.download_url,{headers:memberHeaders}));
      await command('message.delete',{event_id:bound.event.id});
      console.log(JSON.stringify({test_progress:'transfer_complete'}));
    }
  }
  await new Promise(resolve=>setTimeout(resolve,config.rtt_ms)); linkRtt=0;
}
async function measureDownloadTransport({member,memberHeaders,attachment}) {
  linkRtt = config.rtt_ms;
  // The measured fixture uses the same repeated varied piece in both directions.
  for (const path of ['legacy_ack','binary_window']) for (const count of [1,10]) {
    linkRtt=config.rtt_ms;
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
  await new Promise(resolve=>setTimeout(resolve,config.rtt_ms)); linkRtt=0;
}

async function verifyEncryptedCreditRejection(attachment) {
  for (const fault of ['legacy_host',0,1,384*1024+1,'replay','binary_length']) {
    const peer=await RemoteTransport.connect(config.target,'member_connect');
    if(fault==='legacy_host') {
      // Replay pre-binary host capability headers after production WebCrypto opens
      // each record. The real host then executes its unchanged legacy owners.
      const accept=peer.accept.bind(peer), send=peer.sendBytesRecord.bind(peer);
      peer.accept=frame=>{if(frame.op==='response')frame.headers=frame.headers.filter(([name])=>!name.startsWith('x-agentsassemble-body-'));return accept(frame);};
      peer.sendBytesRecord=bytes=>{
        assert.equal(bytes[0],123,'older host accepts JSON plaintext records only');
        const frame=JSON.parse(new TextDecoder().decode(bytes));
        if(frame.op==='request')assert.deepEqual(Object.keys(frame).sort(),['op','id','method','path','headers',...(frame.upload_credit?['upload_credit']:[])].sort());
        return send(bytes);
      };
    }
    const device='aad1_'+Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString('base64url');
    const post=async (path,body)=>{const response=await peer.fetch(path,{method:'POST',headers:{'content-type':'application/json','x-device-token':device},body:JSON.stringify(body)});assert.equal(response.status,200);return response.json();};
    const challenge=await post('/api/member-connect/challenge',{});
    await post('/api/member-connect/rooms',{challenge_id:challenge.challenge_id,grant_token:'aamc1.'+'A'.repeat(43)});
    const selected=await post('/api/member-connect/select',{challenge_id:challenge.challenge_id,room_id:'secure-room',client_id:crypto.randomUUID()});
    let received=0, release;
    const full=new Promise(resolve=>{release=resolve;});
    const accept=peer.acceptBinary.bind(peer);
    peer.acceptBinary=async bytes=>{const result=await accept(bytes);received+=bytes.length-18;if(received>=320*1024)release();return result;};
    const response=await peer.fetch(attachment.download_url,{headers:{'x-device-token':device,authorization:`Bearer ${selected.session_token}`} });
    assert.equal(response.status,200);
    if(fault==='legacy_host') { assert.deepEqual(Buffer.from(await response.arrayBuffer()),Buffer.alloc(attachment.size,50));peer.close();continue; }
    await full;
    assert.equal(received,320*1024,'stalled body leaves one shared piece for another consumer');
    const id=peer.nextId-1, ended=new Promise(resolve=>peer.onClose(resolve));
    if(fault==='binary_length') await peer.sendBytesRecord(new Uint8Array([1,0,0]));
    else if(fault==='replay') {
      const reader=response.body.getReader(); assert.equal((await reader.read()).value.length,64*1024);
      // Public pull queued the first credit; replay its exact cumulative offset.
      await peer.send({op:'credit',id,offset:64*1024});
    } else await peer.send({op:'credit',id,offset:fault});
    await ended; assert.equal(peer.active,false);
  }
}
