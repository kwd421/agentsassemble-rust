async function verifyHumanTransfers({member, memberHeaders, memberSocket, memberReceive}) {
  console.log(JSON.stringify({test_progress:'transfer_started'}));
  for (const [index, size, image] of [[0, 50 * 1024 * 1024, null],
    [1, Buffer.from(config.persona_png, 'base64').length, Buffer.from(config.persona_png, 'base64')]]) {
    const declaration = {room_id:'secure-room', operation_id:crypto.randomUUID(),
      filename:`transfer-${index}`, content_type:image?'image/png':'application/octet-stream', declared_size:size};
    const root = '/api/message-attachment-transfers';
    async function jsonRequest(path, method, body) {
      const response = await member.fetch(path, {method, headers:memberHeaders, body});
      const value = await response.json();
      assert.equal(response.status,200, `${path}: ${JSON.stringify(value)}`);
      assert.equal(response.headers.get('x-agentsassemble-upload-transfer'),'1');
      return value;
    }
    const begun = await jsonRequest(root,'POST',JSON.stringify(declaration));
    assert.deepEqual(await jsonRequest(root,'POST',JSON.stringify(declaration)),begun);
    const path = `${root}/${begun.transfer.handle}`;
    for (const [offset,end] of [[0,19],[19,size]]) {
      let produced = offset;
      const body = new ReadableStream({pull(controller) {
        if (produced === end) {controller.close();return;}
        const count = Math.min(64*1024,end-produced);
        const bytes=image?image.subarray(produced,produced+count):new Uint8Array(count);
        if (!image) for(let i=0;i<count;i++) bytes[i]=(produced+i)%251;
        controller.enqueue(bytes);
        produced += count;
      }});
      const response = await member.fetch(`${path}/data?offset=${offset}`, {method:'POST',
        headers:{...memberHeaders,'content-type':'application/octet-stream'},body,duplex:'half'});
      assert.equal(response.status,200);
      assert.equal((await response.json()).transfer.accepted_offset,end);
      assert.equal((await jsonRequest(path,'GET')).transfer.accepted_offset,end);
    }
    const finished = await jsonRequest(`${path}/finish`,'POST');
    assert.deepEqual(await jsonRequest(path,'GET'),finished,'status recovers a lost finish response');
    assert.deepEqual(await jsonRequest(`${path}/finish`,'POST'),finished);
    const attachment = finished.transfer.attachment;
    assert.equal(attachment.size,size); assert.equal(attachment.is_image,Boolean(image));
    const request_id=crypto.randomUUID();
    memberSocket.send(JSON.stringify({op:'command',request_id,action:'message.send',payload:{content:'',attachment_ids:[attachment.id]}}));
    for (;;) {const frame=await memberReceive();if(frame.request_id===request_id){assert.ok(frame.accepted);break;}}
    const response=await member.fetch(attachment.download_url,{headers:memberHeaders});
    assert.equal(response.status,200);
    let received=0;
    for await (const piece of response.body) {
      const expected=image?image.subarray(received,received+piece.length):Buffer.alloc(piece.length);
      if (!image) for(let i=0;i<piece.length;i++) expected[i]=(received+i)%251;
      assert.deepEqual(Buffer.from(piece),expected);
      received+=piece.length;
    }
    assert.equal(received,size);
    console.log(JSON.stringify({test_progress:'transfer_complete'}));
  }
}
