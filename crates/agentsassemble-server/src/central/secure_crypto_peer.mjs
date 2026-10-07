import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';
const { beginSecureHandshake } = await import(pathToFileURL(process.env.AA_SECURE_CRYPTO_MODULE));
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const read = async () => JSON.parse((await lines.next()).value);
const out = value => process.stdout.write(JSON.stringify(value) + '\n');
const pinned = await read();
const target = { server_id: pinned.server_id, registration_epoch: 'epoch', origin: 'https://secure.example.test', generation: 1,
  host_public_key_jwk: { kty:'OKP', crv:'Ed25519', x:pinned.key }, host_key_fingerprint:pinned.fingerprint };
const pending = await beginSecureHandshake(target, 'owner');
out(pending.hello);
const channel = await pending.finish(await read());
out(Buffer.from(await channel.send.seal(new TextEncoder().encode('client possession'))).toString('base64url'));
const record = Buffer.from(await read(), 'base64url');
const decrypted = await channel.receive.open(record.buffer.slice(record.byteOffset, record.byteOffset + record.byteLength));
out(new TextDecoder().decode(decrypted));
process.exit(0);
