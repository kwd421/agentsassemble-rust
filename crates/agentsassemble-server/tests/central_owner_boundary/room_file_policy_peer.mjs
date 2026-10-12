// Runs through the production browser transport and revisioned manager socket.
async function verifyRoomFilePolicy({member, memberHeaders, memberSocket, memberReceive,
  managerSocket, managerReceive, revision}) {
  async function command(socket, receive, action, payload) {
    const request_id = crypto.randomUUID();
    socket.send(JSON.stringify({op:'command', request_id, action, payload}));
    for (;;) {
      const frame = await receive();
      if (frame.request_id !== request_id) continue;
      assert.equal(frame.op, 'ack'); assert.ok(frame.accepted);
      return frame.result;
    }
  }
  async function limit(bytes) {
    const result = await command(managerSocket, managerReceive, 'room.settings.update', {
      expected_revision:revision, attachment_file_limit_bytes:bytes,
    });
    revision = result.room_settings.settings_revision;
  }
  const encoded25 = 4 * Math.ceil(25 * 1024 * 1024 / 3);
  { // Virtual HTTP permits no Content-Length header; exercise its streamed bound.
    let produced = 0;
    const body = new ReadableStream({ pull(controller) {
      controller.enqueue(new Uint8Array(64 * 1024)); produced += 64 * 1024;
      if (produced >= encoded25) controller.close();
    }});
    const response = await member.fetch('/api/message-attachments', {method:'POST',
      headers:memberHeaders, body, duplex:'half'});
    assert.equal(response.status,413);
    assert.equal((await response.json()).code,'payload_too_large');
    assert.ok(produced < encoded25, '10 MiB room rejects before the entire 25 MiB file envelope');
    assert.ok(member.active, 'body refusal preserves the encrypted channel');
  }
  await limit(50 * 1024 * 1024);
  const uploads = [];
  for (const mib of [25, 50]) {
    const bytes = Buffer.alloc(mib * 1024 * 1024 + (mib === 25 ? 19 : 0), mib);
    const response = await member.fetch('/api/message-attachments', {method:'POST',headers:memberHeaders,
      body:JSON.stringify({filename:`room-${mib}.bin`,content_type:'application/octet-stream',data_base64:bytes.toString('base64')})});
    assert.equal(response.status,200);
    const {attachment} = await response.json();
    assert.equal(attachment.size, bytes.length);
    uploads.push({attachment, bytes});
  }
  await limit(10 * 1024 * 1024);
  await command(memberSocket, memberReceive, 'message.send', {content:'files retained after lowering',
    attachment_ids:uploads.map(item => item.attachment.id)});
  for (const {attachment, bytes} of uploads) {
    const response = await member.fetch(attachment.download_url, {headers:memberHeaders});
    assert.equal(response.status,200);
    assert.equal(Number(response.headers.get('content-length')), bytes.length);
    assert.deepEqual(Buffer.from(await response.arrayBuffer()), bytes);
  }
  const large = uploads.find(item => item.attachment.size === 50 * 1024 * 1024);
  console.log(JSON.stringify({test_progress:'download_batch_started'}));
  await Promise.all(Array.from({length:10}, async () => {
    const response = await member.fetch(large.attachment.download_url, {headers:memberHeaders});
    assert.equal(response.status,200);
    assert.equal(Number(response.headers.get('content-length')), large.bytes.length);
    let received = 0;
    for await (const piece of response.body) {
      assert.deepEqual(Buffer.from(piece), large.bytes.subarray(received, received + piece.length));
      received += piece.length;
    }
    assert.equal(received, large.bytes.length);
    console.log(JSON.stringify({test_progress:'download_complete'}));
  }));
  const abort = new AbortController();
  const cancelled = await member.fetch(large.attachment.download_url, {headers:memberHeaders, signal:abort.signal});
  const reader = cancelled.body.getReader();
  assert.equal((await reader.read()).done, false);
  abort.abort();
  await assert.rejects(reader.read());
  assert.ok(member.active, 'request cancellation preserves the channel');
  return large.attachment;
}
