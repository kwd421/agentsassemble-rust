// Shared fixture is emitted by the real Rust prepare/bind/draft flow, never a
// copied implementation or a user backup. Run from the Rust repository:
// IDENTITY_DIRECTORY=/path/to/infra/identity-directory \
// AGENTSASSEMBLE_CENTRAL_URL=https://central.example \
// cargo test -p agentsassemble-server --lib native_restore_worker_shared_fixture -- --ignored
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

assert.ok(process.env.IDENTITY_DIRECTORY, 'IDENTITY_DIRECTORY must name the Worker checkout');
const worker = file => pathToFileURL(resolve(process.env.IDENTITY_DIRECTORY, 'src', file)).href;
const {restoreIdentity, restoreCanonical} = await import(worker('server_restore.js'));
const {verifyHostSignature} = await import(worker('crypto.js'));
const {bound, draft, issuer} = JSON.parse(await readFile(process.argv[2], 'utf8'));
const serverId = bound.journal.manifest.server_id;
const identity = await restoreIdentity(serverId, bound.body);
assert.equal(identity.fingerprint, bound.journal.new_key_fingerprint, 'validated JWK fingerprint');
assert.equal(identity.epoch, bound.journal.expected_epoch, 'string epoch');
assert.equal(identity.operation, bound.journal.operation_id, 'durable operation');
assert.equal(identity.requestId, bound.request_id, 'Google handoff and guest proof request_id');
assert.equal(bound.path, `/v1/servers/${serverId}/restore-proof`);
assert.equal(draft.path, `/v1/servers/${serverId}/restore`);
const body = JSON.parse(draft.body);
const switched = await restoreIdentity(serverId, body);
assert.deepEqual(switched, identity, 'draft/switch retain the bound tuple');
const proof = body.host_proof;
assert.equal(await verifyHostSignature(switched.jwk, proof.signature,
  restoreCanonical(issuer, proof.owner_person_id, switched, proof.issued_at, proof.nonce)),
true, 'Rust draft signature verifies in Worker switch');
