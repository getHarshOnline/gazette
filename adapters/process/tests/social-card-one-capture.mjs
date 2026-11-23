// An explicit one-capture checkpoint, separate from the multi-case browser suite.
import assert from 'node:assert/strict';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, writeFile, readFile, stat, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, isAbsolute } from 'node:path';
import { capture } from '../social-card.mjs';

const program = process.env.GAZETTE_TEST_CHROMIUM;
const sha256 = process.env.GAZETTE_TEST_CHROMIUM_SHA256;
assert.ok(program && isAbsolute(program) && /^[a-f0-9]{64}$/.test(sha256 ?? ''), 'An explicit browser path and pin are required');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const executable = await readFile(program);
assert.equal(hash(executable), sha256);
const root = await mkdtemp(join(tmpdir(), 'gazette-one-card-fixture-'));
const children = [];
const originalSpawn = childProcess.spawn;
// Observe the real launch and exit without changing its arguments, environment or process.
childProcess.spawn = (...args) => {
  const child = originalSpawn(...args);
  const observed = { pid: child.pid, profile: args[1].find(arg => arg.startsWith('--user-data-dir=')).slice('--user-data-dir='.length), exited: false };
  children.push(observed);
  child.once('exit', (code, signal) => Object.assign(observed, { exited: true, code, signal }));
  return child;
};
syncBuiltinESMExports();
try {
  await mkdir(join(root, 'payload'));
  const assets = [];
  for (const [name, source, media_type, role] of [
    ['image.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>', 'image/svg+xml', 'media'],
    ['image-dark.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="blue"/></svg>', 'image/svg+xml', 'media'],
    ['card.mjs', `const slot=document.querySelector('[data-social-card-media]');slot.addEventListener('gazette:social-card-media-ready',()=>{if(slot.firstElementChild.src!=='https://card.test/image-dark.svg')throw Error('wrong declared variant');slot.dispatchEvent(new CustomEvent('gazette:social-card-media-complete',{detail:{status:'complete'}}));},{once:true});`, 'text/javascript', 'module'],
  ]) {
    const bytes = Buffer.from(source), entry = `payload/${name}`;
    await writeFile(join(root, entry), bytes);
    assets.push({ url: `https://card.test/${name}`, entry, media_type, role, bytes: bytes.length, sha256: hash(bytes) });
  }
  const result = JSON.parse(await capture({
    version: 1, request_sha256: 'a'.repeat(64), initial_title: 'Fallback', document_url: 'https://card.test/article/',
    html: '<!doctype html><html><head><style>html,body{margin:0;background:white;color-scheme:dark}h1{font-size:12px}</style></head><body><template data-social-card-source><h1>One retained image</h1><img id="hero" data-gazette-image="content:home#image-1" src="/image.svg"></template><div data-social-card-media></div><h1 data-social-card-title>Fallback</h1><script type="module" src="/card.mjs"></script></body></html>',
    width: 200, height: 100, media_selector: '#hero', media_kind: 'image', assets,
    image_variants: [{ address: 'content:home#image-1', destination: 'https://card.test/image.svg', variants: { light: 'https://card.test/image.svg', dark: 'https://card.test/image-dark.svg' } }],
    browser: { program, bytes: executable.length, sha256 }, limits: { timeout_ms: 10000, max_output_bytes: 1 << 20 },
  }, root));
  const png = Buffer.from(result.png_base64, 'base64');
  assert.equal(result.title, 'One retained image');
  assert.equal(png.subarray(1, 4).toString(), 'PNG');
  assert.equal(png.readUInt32BE(16), 200); assert.equal(png.readUInt32BE(20), 100);
  assert.equal(children.length, 1); assert.equal(children[0].exited, true);
  assert.equal(children[0].code, 0); assert.equal(children[0].signal, null);
  await assert.rejects(stat(children[0].profile), error => error.code === 'ENOENT');
  process.stdout.write(JSON.stringify({ captures: 1, selected_variant: 'https://card.test/image-dark.svg', browser: { program, bytes: executable.length, sha256 }, title: result.title,
    png_bytes: png.length, png_sha256: hash(png), width: 200, height: 100, child: children[0], profile_removed: true }) + '\n');
} finally {
  childProcess.spawn = originalSpawn; syncBuiltinESMExports();
  await rm(root, { recursive: true, force: true });
}
