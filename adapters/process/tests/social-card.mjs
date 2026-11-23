import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { mkdtemp, mkdir, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { validate } from '../social_card/contract.mjs';
import { retainedAsset } from '../social_card/assets.mjs';

export const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export function request() {
  return { version: 1, request_sha256: 'a'.repeat(64), html: '<html></html>', initial_title: 'Fallback', document_url: 'https://card.test/article/', width: 200, height: 100,
    media_selector: '#hero', media_kind: 'image', browser: { program: '/explicit/chrome', bytes: 1, sha256: 'b'.repeat(64) },
    limits: { timeout_ms: 10000, max_output_bytes: 1 << 20 }, assets: [] };
}

test('card wire enforces independent bounds and retained roles', () => {
  assert.equal(validate(request()).width, 200);
  for (const change of [
    { width: 0 }, { height: 4097 }, { media_selector: ' #hero' }, { media_selector: 'x\n' },
    { media_selector: 'é'.repeat(513) }, { media_kind: 'guess' }, { video_time_ms: 0 },
    { initial_title: '' }, { initial_title: 'é'.repeat(513) }, { html: 'x'.repeat((8 << 20) + 1) },
    { document_url: 'file:///tmp/card' }, { document_url: 'https://name:secret@card.test/' },
    { unknown: true }, { browser: { program: 'chrome', bytes: 1, sha256: 'a'.repeat(64) } },
  ]) assert.throws(() => validate({ ...request(), ...change }));
  assert.doesNotThrow(() => validate({ ...request(), media_kind: 'video', video_time_ms: 600000 }));
  const asset = { url: 'https://card.test/image.png', entry: 'payload/image', media_type: 'image/png', bytes: 4, sha256: 'a'.repeat(64), role: 'media' };
  assert.doesNotThrow(() => validate({ ...request(), assets: [asset] }));
  for (const change of [{ role: 'script' }, { entry: 'payload/../outside' }, { entry: '/payload/image' }, { url: 'https://other.test/image.png' }, { media_type: 'text/plain\r\nHeader:value' }]) {
    assert.throws(() => validate({ ...request(), assets: [{ ...asset, ...change }] }));
  }
  assert.throws(() => validate({ ...request(), assets: [asset, asset] }));
});

test('declared image alternatives are bounded and preserve their exact native address', () => {
  const image = { address: 'content:home#image-1', destination: 'https://card.test/image.png',
    variants: { dark: 'https://card.test/image-dark.png' } };
  assert.doesNotThrow(() => validate({ ...request(), image_variants: [image] }));
  for (const image_variants of [null, {}, [image, image],
    [{ ...image, address: '' }], [{ ...image, variants: {} }],
    [{ ...image, variants: { guessed: image.destination } }],
    [{ ...image, variants: { dark: 'file:///private/image.png' } }],
    Array.from({length: 10001}, (_, i) => ({ ...image, address: `image-${i}` })),
  ]) assert.throws(() => validate({ ...request(), image_variants }));
});

test('retained reads check regular files, byte identity and parent directories', async () => {
  const root = await mkdtemp(join(tmpdir(), 'gazette-card-assets-'));
  try {
    await mkdir(join(root, 'payload'));
    const bytes = Buffer.from('retained bytes');
    const asset = { entry: 'payload/file', bytes: bytes.length, sha256: hash(bytes) };
    await writeFile(join(root, asset.entry), bytes);
    assert.deepEqual(await retainedAsset(root, asset), bytes);
    await assert.rejects(retainedAsset(root, { ...asset, sha256: 'a'.repeat(64) }), /asset-binding/);
    await writeFile(join(root, asset.entry), 'changed bytes!');
    await assert.rejects(retainedAsset(root, asset), /asset-binding/);
    await symlink(join(root, asset.entry), join(root, 'payload/link'));
    await assert.rejects(retainedAsset(root, { ...asset, entry: 'payload/link' }), /asset-binding/);
  } finally { await rm(root, { recursive: true, force: true }); }
});


test('CLI entrypoint executes through a symlinked path instead of silently succeeding', async () => {
  const root = await mkdtemp(join(tmpdir(), 'gazette-card-entry-'));
  try {
    const entry = join(root, 'entry.mjs');
    await symlink(fileURLToPath(new URL('../social-card.mjs', import.meta.url)), entry);
    const result = spawnSync(process.execPath, [entry], {
      input: '{}', encoding: 'utf8', env: {}, timeout: 5000, maxBuffer: 65536,
    });
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.equal(result.stdout, '');
    assert.match(result.stderr, /^social-card: [a-z-]+\n$/);
  } finally { await rm(root, { recursive: true, force: true }); }
});


test('closed capture transport still terminates its child, verifies and removes its profile', async () => {
  const { cleanupCapture } = await import('../social-card.mjs');
  const { CDP } = await import('../social_card/cdp.mjs');
  const calls = [];
  const cdp = new CDP({ readyState: WebSocket.CLOSED, addEventListener() {}, close() { calls.push('socket-close'); } });
  let finish;
  const exited = new Promise(resolve => { finish = resolve; });
  const child = { exitCode: null, signalCode: null, kill(signal) { calls.push(signal); this.signalCode = signal; finish(); } };
  const original = new Error('capture-timeout');
  await cleanupCapture({ cdp, child, exited,
    verify() { calls.push('verify'); throw new Error('verification-failed'); },
    remove() { calls.push('remove'); },
  }, original);
  assert.deepEqual(calls, ['socket-close', 'SIGKILL', 'verify', 'remove']);
  assert.equal(original.message, 'capture-timeout');
  await assert.rejects(cleanupCapture({
    verify() { throw new Error('verification-failed'); },
    remove() { calls.push('remove-after-success'); },
  }), /verification-failed/);
  assert.equal(calls.at(-1), 'remove-after-success');
});
