// Actual Node addon history reads compared with the independent Rust SDK owner.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 4, 'supply exact addon and history_reference paths');
const [, , addon, referenceExecutable] = process.argv;
assert.ok(isAbsolute(referenceExecutable));
const api = loadGazette(addon);
const kinds = ['authorization', 'leases', 'consumption', 'attempts', 'reconciliations'];
const native = request => JSON.parse(execFileSync(referenceExecutable, [], {
  input: JSON.stringify(request), encoding: 'utf8', maxBuffer: 8 * 1024 * 1024,
}));
const bytes = request => Buffer.from(JSON.stringify(request));
function failure(call, code) {
  assert.throws(call, error => {
    assert.ok(error instanceof Error, 'no partial success');
    assert.equal(JSON.parse(error.message).code, code);
    return true;
  });
}
function directory(t) {
  const root = mkdtempSync(join(tmpdir(), 'gazette-node-history-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test('all five history projections equal native SDK reports without advancing the journal', t => {
  const root = directory(t), journal = join(root, 'journal');
  mkdirSync(journal, { mode: 0o700 });
  const initialized = native({ operation: 'initialize', journal });
  const before = readFileSync(initialized.database);
  const request = {
    database: initialized.database, record_id: 'selected-record',
    expected_header: initialized.header, read: { allow_coordination_files: true },
  };
  const expected = native({ operation: 'inspect', ...request });
  for (const kind of kinds) {
    for (const header of [initialized.header, null, undefined]) {
      const report = api.inspectReleaseHistory(bytes({ ...request, kind, expected_header: header }), { allowRead: true });
      assert.deepEqual(JSON.parse(report), expected[kind], kind);
    }
    const wrong = { ...initialized.header, writer_fence: initialized.header.writer_fence + 1 };
    failure(() => api.inspectReleaseHistory(bytes({ ...request, kind, expected_header: wrong }), { allowRead: true }), 'store_journal_mismatch');
    const exact = JSON.stringify({ ...request, kind });
    for (const coordinate of ['9007199254740993', '18446744073709551615']) {
      const retained = exact.replace(/"writer_fence":\d+/, `"writer_fence":${coordinate}`);
      assert.notEqual(retained, exact);
      failure(() => api.inspectReleaseHistory(Buffer.from(retained), { allowRead: true }), 'store_journal_mismatch');
    }
    const outsideNativeRange = exact.replace(/"writer_fence":\d+/, '"writer_fence":18446744073709551616');
    failure(() => api.inspectReleaseHistory(Buffer.from(outsideNativeRange), { allowRead: true }), 'node_query');
    failure(() => api.inspectReleaseHistory(bytes({ ...request, kind, read: { allow_coordination_files: true, storage_bytes: 1 } }), { allowRead: true }), 'store_storage_limit');
    const encoded = bytes({ ...request, kind });
    const maximum = Buffer.byteLength(JSON.stringify(expected[kind])) - 1;
    assert.throws(() => api.inspectReleaseHistory(encoded, { allowRead: true, resultBytes: maximum }), error => {
      const unavailable = JSON.parse(error.message);
      assert.equal(unavailable.code, 'history_response_unavailable');
      assert.equal(unavailable.request_sha256, createHash('sha256').update(encoded).digest('hex'));
      assert.equal(Object.hasOwn(unavailable, 'record'), false, 'never return partial native history');
      assert.ok(Buffer.byteLength(error.message) <= maximum, 'reserved failure respects result ceiling');
      return true;
    });
  }
  assert.deepEqual(readFileSync(initialized.database), before, 'read-only SQL never advances or migrates');
  assert.deepEqual(native({ operation: 'inspect', ...request }), expected, 'independent final read-back');
});

test('independent read and coordination permissions precede missing database access', t => {
  const root = directory(t), database = join(root, 'missing.db');
  for (const kind of kinds) {
    const request = { kind, database, record_id: 'selected-record' };
    for (const read of [undefined, { allow_coordination_files: true }]) {
      failure(() => api.inspectReleaseHistory(bytes({ ...request, read }), { allowRead: false }), 'storage_permission');
    }
    failure(() => api.inspectReleaseHistory(bytes(request), { allowRead: true }), 'store_coordination_permission');
    failure(() => api.inspectReleaseHistory(bytes({ ...request, read: { allow_coordination_files: true } }), { allowRead: true }), 'store_io');
  }
  assert.deepEqual(readdirSync(root), [], 'failed reads never initialize missing storage');
});

test('closed JSON shapes, duplicate keys and bounded transport fail before storage', t => {
  const root = directory(t), database = join(root, 'missing.db');
  const request = { kind: 'authorization', database, record_id: 'selected-record', read: { allow_coordination_files: true } };
  const valid = JSON.stringify(request);
  const invalid = [
    '', '{', '[]', 'null',
    JSON.stringify({ ...request, kind: 'latest' }),
    JSON.stringify({ ...request, extra: true }),
    JSON.stringify({ ...request, read: [true, 268435456, 250, 1048576, 1048576, 100000] }),
    JSON.stringify({ ...request, read: { allow_coordination_files: true, extra: true } }),
    JSON.stringify({ ...request, expected_header: [1, Array(32).fill(1), 1, 1] }),
    valid.replace('"kind":"authorization"', '"kind":"authorization","kind":"authorization"'),
    valid.replace('"allow_coordination_files":true', '"allow_coordination_files":true,"allow_coordination_files":true'),
  ];
  for (const text of invalid) failure(() => api.inspectReleaseHistory(Buffer.from(text), { allowRead: true }), 'node_query');
  failure(() => api.inspectReleaseHistory(bytes({ ...request, database: 'relative.db' }), { allowRead: true }), 'store_unsafe_path');
  failure(() => api.inspectReleaseHistory(Buffer.from([0xff]), { allowRead: true }), 'node_query');
  failure(() => api.inspectReleaseHistory(bytes(request), { allowRead: true, inputBytes: 1 }), 'node_query_limit');
  failure(() => api.inspectReleaseHistory(bytes(request), { allowRead: true, resultBytes: 1 }), 'node_result_limit');
  for (const inputBytes of [-1, 1.5, NaN, Infinity]) {
    failure(() => api.inspectReleaseHistory(bytes(request), { allowRead: true, inputBytes }), 'node_limits');
  }
  assert.deepEqual(readdirSync(root), [], 'invalid transport never reaches storage');
});
