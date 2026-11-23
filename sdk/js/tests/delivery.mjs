// Native Rust fixture owns the live HTTP server, retained tree, curl pin and reports.
// This child invokes the real addon, never a CLI bridge or substitute HTTP implementation.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash } from 'node:crypto';
import { isAbsolute } from 'node:path';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 3, 'supply one exact native addon path');
assert.ok(isAbsolute(process.argv[2]));
const chunks = [];
let length = 0;
for await (const chunk of process.stdin) {
  length += chunk.length;
  assert.ok(length <= 4 * 1024 * 1024, 'bounded native delivery fixture packet');
  chunks.push(chunk);
}
const { request, expected, success_expected } = JSON.parse(Buffer.concat(chunks).toString('utf8'));
const api = loadGazette(process.argv[2]);
const bytes = Buffer.from(JSON.stringify(request));
const encoded = value => Buffer.from(JSON.stringify(value));
function failure(call, code) {
  let caught;
  try { call(); } catch (error) { caught = error; }
  assert.ok(caught instanceof Error, 'nonmatching routes must not become successful partial observations');
  const report = JSON.parse(caught.message);
  if (code) assert.equal(report.code, code);
  return report;
}

await test('real addon delivery preserves complete native observations and refusal boundaries', async t => {
  await t.test('mixed outcomes retain all four exact routes and match the Rust report', () => {
    const report = failure(() => api.observeDelivery(bytes));
    assert.deepEqual(report, expected);
    assert.deepEqual(report.observations.map(row => row.route), request.selection.routes);
    assert.deepEqual(report.observations.map(row => row.outcome.status),
      ['matches', 'differs', 'http-failure', 'unavailable']);
    assert.deepEqual(report.output, request.tree_identity);
    assert.equal(report.base_url, request.selection.base_url);
    assert.ok(!JSON.stringify(report).includes('PRIVATE_'), 'remote response bodies stay private');
    assert.ok(!Object.hasOwn(report, 'settlement'));
  });

  await t.test('a matching explicit subset returns its native report, not whole-site approval', () => {
    const subset = structuredClone(request);
    subset.selection.routes = [request.selection.routes[0]];
    const report = JSON.parse(api.observeDelivery(encoded(subset), {resultBytes:1 << 20}));
    assert.deepEqual(report, success_expected);
    assert.equal(report.observations.length, 1);
    assert.equal(report.observations[0].outcome.status, 'matches');
    assert.deepEqual(report.output, request.tree_identity);
  });

  await t.test('permissions, malformed input, stale identity and impossible budgets cause no HTTP requests', () => {
    for (const permission of ['allow_read', 'allow_execute', 'allow_network']) {
      const denied = structuredClone(request);
      denied.options.http[permission] = false;
      failure(() => api.observeDelivery(encoded(denied)), 'delivery_request');
    }
    for (const input of [
      Buffer.from('[]'), Buffer.from('null'), Buffer.from('{broken'), Buffer.from([0xff]),
      encoded({...request, allow_network:true}),
      Buffer.from(bytes.toString().replace('"tree":', '"tree":"old","tr\\u0065e":')),
    ]) failure(() => api.observeDelivery(input), 'delivery_input');
    const stale = structuredClone(request);
    stale.tree_identity.sha256 = '0'.repeat(64);
    failure(() => api.observeDelivery(encoded(stale)), 'delivery_capture');
    failure(() => api.observeDelivery(bytes, {inputBytes:1}), 'delivery_input_limit');
    failure(() => api.observeDelivery(bytes, {resultBytes:1}), 'node_result_limit');
    for (const value of [-1, 0.5, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 1]) {
      failure(() => api.observeDelivery(bytes, {resultBytes:value}), 'node_limits');
    }
    // The parent owns the request log and verifies no calls from any refusal.
  });

  await t.test('overflow after observations delivers an exact request-bound warning without retry', () => {
    assert.ok(Buffer.byteLength(JSON.stringify(expected)) > 1024, 'budget excludes complete native report');
    const report = failure(() => api.observeDelivery(bytes, {resultBytes:1024}), 'delivery_response_unavailable');
    assert.equal(report.request_sha256, createHash('sha256').update(bytes).digest('hex'));
    assert.match(report.message, /HTTP requests may have occurred/);
    assert.match(report.message, /do not infer rollback or retry/);
    assert.ok(!Object.hasOwn(report, 'observations'), 'no silently shrunken route report');
    assert.ok(Buffer.byteLength(JSON.stringify(report)) <= 1024);
  });
});
// Parent verifies exactly 4 mixed + 1 matching + 4 overflow GETs and unchanged retained output.
console.log('Gazette JavaScript delivery proof passed');
