// Frozen pre-binary request/data shapes, replayed through the production AEAD
// cipher against the real host. The current HTTP response consumer rejects any
// binary response for these legacy children and emits the original sequence ACK.
async function legacyBodyFetch(channel, path, init = {}) {
  const id = channel.allocate();
  let wait;
  const response = new Promise((resolve, reject) => {
    wait = {resolve, reject, chunks:[], end:false, pulling:false, responded:false,
      uploadCredit:true, binary:false, offeredBinary:false, sendOffset:0, receiveOffset:0,
      removeAbort() {}};
    channel.http.set(id, wait);
  });
  void (async () => {
    await channel.send({op:'request',id,method:init.method || 'GET',path,
      headers:[...new Headers(init.headers)],upload_credit:true});
    const reader = new Response(init.body).body?.getReader();
    try {
      if (reader) for (;;) {
        const piece = await reader.read(); if (piece.done) break;
        for (let start=0;start<piece.value.length;start+=64*1024) {
          const consumed = new Promise(resolve => {wait.consumed = resolve;});
          const data = Buffer.from(piece.value.subarray(start,start+64*1024)).toString('base64url');
          await channel.send({op:'data',id,data,end:false});
          await consumed;
          if (wait.responded) {await reader.cancel();return;}
        }
      }
      await channel.send({op:'data',id,data:'',end:true});
    } finally {reader?.releaseLock();}
  })().catch(error => channel.cancelHttp(id,error));
  return response;
}
