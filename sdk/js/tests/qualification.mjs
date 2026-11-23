// Invoked by the native qualification fixture while its exact captured sources exist.
// Expected certificates and policy programs come from that owner, never JS replicas.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { isAbsolute } from 'node:path';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 3, 'supply one exact native addon path');
assert.ok(isAbsolute(process.argv[2]));
const chunks = [];
let length = 0;
for await (const chunk of process.stdin) {
  length += chunk.length;
  assert.ok(length <= 32 * 1024 * 1024, 'bounded native fixture packet');
  chunks.push(chunk);
}
const { request, expected, log } = JSON.parse(Buffer.concat(chunks).toString('utf8'));
assert.ok(isAbsolute(log));
const api = loadGazette(process.argv[2]);
assert.ok(Object.isFrozen(api));
const bytes = Buffer.from(JSON.stringify(request));
const allowed = { allowRead: true, allowPolicyExecution: true, resultBytes: 8 * 1024 * 1024 };
const events = () => readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
function failure(call, code) {
  let caught;
  try { call(); } catch (error) { caught = error; }
  assert.ok(caught instanceof Error, 'failed qualification cannot return partial success');
  const report = JSON.parse(caught.message);
  if (code) assert.equal(report.code, code);
  return report;
}
function fullSet(report) {
  assert.deepEqual(report.requested, request.sites.map(site => site.render.site));
  assert.deepEqual(report.sites.map(site => site.site), report.requested);
}

await test('real addon qualification shares complete native reports and effect boundaries', async t => {
  const originalCalls = events();
  assert.equal(originalCalls.length, 4, 'native reference ran two candidate and two output checks');

  await t.test('the complete two-site report and checker inputs equal native SDK evidence', () => {
    const report = JSON.parse(api.qualifySites(bytes, allowed));
    assert.deepEqual(report, expected, 'same native certificates and complete evidence, not a JS reconstruction');
    fullSet(report);
    assert.equal(report.failure, null);
    assert.ok(report.sites.every(site => site.status === 'supported-profile-technically-qualified'));
    assert.deepEqual(events().slice(4), originalCalls, 'same exact native checker requests');
  });

  await t.test('independent permissions, malformed recipes and impossible budgets cause no effects', () => {
    const before = events();
    for (const options of [
      { ...allowed, allowRead: false },
      { ...allowed, allowPolicyExecution: false },
      { allowRead: true },
    ]) {
      const report = failure(() => api.qualifySites(bytes, options));
      fullSet(report);
      assert.equal(report.failure.phase, 'before');
      assert.equal(report.failure.code, 'qualification_permission');
      assert.ok(report.sites.every(site => site.status === 'not-run'));
      assert.deepEqual(report.candidate_checks, {});
    }
    for (const input of [
      Buffer.from('[]'), Buffer.from('null'), Buffer.from('{broken'), Buffer.from([0xff]),
      Buffer.from(JSON.stringify({ ...request, stage_parent: '/private/not-a-destination' })),
      Buffer.from(bytes.toString().replace('"workspace":', '"workspace":"old","worksp\\u0061ce":')),
    ]) failure(() => api.qualifySites(input, allowed), 'qualification_input');
    failure(() => api.qualifySites(bytes, { ...allowed, inputBytes: 1 }), 'node_input_limit');
    failure(() => api.qualifySites(bytes, { ...allowed, resultBytes: 1 }), 'node_result_limit');
    for (const value of [-1, 0.5, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 1]) {
      failure(() => api.qualifySites(bytes, { ...allowed, resultBytes: value }), 'node_limits');
    }
    assert.deepEqual(events(), before, 'no configured checker was started by any refusal');
  });

  await t.test('a later rendering mismatch preserves every requested site and native candidate evidence', () => {
    const changed = structuredClone(request);
    changed.sites[1].qualification.rendering_sha256 = '0'.repeat(64);
    const before = events().length;
    const report = failure(() => api.qualifySites(Buffer.from(JSON.stringify(changed)), allowed));
    fullSet(report);
    assert.equal(report.failure.code, 'qualification_render');
    assert.equal(report.sites[0].status, 'not-run');
    assert.equal(report.sites[1].status, 'failed');
    assert.equal(Object.keys(report.candidate_checks).length, 2);
    const calls = events().slice(before);
    assert.equal(calls.length, 2);
    assert.ok(calls.every(call => call.format === 'gazette-candidate-policy/1'),
      'complete identity preflight prevents every output checker, not just the failed site');
    assert.ok(report.sites.every(site => !Object.hasOwn(site, 'certificate')));
  });

  await t.test('post-effect report overflow is explicit, request-bound and never retried', () => {
    const before = events().length;
    const report = failure(() => api.qualifySites(bytes, { ...allowed, resultBytes: 1024 }),
      'qualification_response_unavailable');
    assert.equal(report.request_sha256, createHash('sha256').update(bytes).digest('hex'));
    assert.match(report.message, /may have run/);
    assert.match(report.message, /do not infer rollback or retry/);
    assert.ok(Buffer.byteLength(JSON.stringify(report)) <= 1024);
    assert.ok(!Object.hasOwn(report, 'sites'), 'unavailable is not a truncated successful set');
    assert.deepEqual(events().slice(before), originalCalls, 'the effectful operation ran exactly once');
  });
});
console.log('Gazette JavaScript qualification proof passed');
