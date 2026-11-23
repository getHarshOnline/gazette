import assert from 'node:assert/strict';
import test from 'node:test';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { mkdtemp, mkdir, writeFile, rm, readFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, isAbsolute } from 'node:path';
import { inflateSync } from 'node:zlib';
import { capture } from '../social-card.mjs';
import { request, hash } from './social-card.mjs';

const program = process.env.GAZETTE_TEST_CHROMIUM;
const pin = process.env.GAZETTE_TEST_CHROMIUM_SHA256;
// Four VP8 frames: one second red, then one second blue, at 16 x 16 pixels.
const video = 'GkXfo59ChoEBQveBAULygQRC84EIQoKEd2VibUKHgQJChYECGFOAZwEAAAAAAAKCEU2bdLpNu4tTq4QVSalmU6yBoU27i1OrhBZUrmtTrIHYTbuMU6uEElTDZ1OsggElTbuMU6uEHFO7a1OsggJa7AEAAAAAAABZAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAVSalmsirXsYMPQkBNgI1MYXZmNjIuMTIuMTAxV0GNTGF2ZjYyLjEyLjEwMUSJiECfQAAAAAAAFlSua8iuAQAAAAAAAD/XgQFzxYioyNsglVw0GJyBACK1nIN1bmSIgQCGhVZfVlA4g4EBI+ODhB3NZQDgkLCBELqBEJqBAlWwhFW5gQESVMNn/HNzoGPAgGfImkWjh0VOQ09ERVJEh41MYXZmNjIuMTIuMTAxc3PWY8CLY8WIqMjbIJVcNBhnyKFFo4dFTkNPREVSRIeUTGF2YzYyLjI4LjEwMSBsaWJ2cHhnyKFFo4hEVVJBVElPTkSHkzAwOjAwOjAyLjAwMDAwMDAwMAAfQ7Z1QK7ngQCjvYEAAIDwAgCdASoQABAAAEcIhYWIhYSIAgICdaoD+AP6Ag1NGAD+/W7z/+OZNzDE/45t//FhPA4oyP/xUQCjlYEB9ACxAQABEBAAGAAYWC/0AAgAAKO8gQPogLACAJ0BKhAAEAAARwiFhYiFhIgCAgJ1qgP4Agz9KAD+/00S//xYV/FhX8WFf/FhX/z8zu3F/OYAo5WBBdwAsQEAARAQABgAGFgv9AAIAAAcU7tro7uPs4EAt4r3gQHxggGm8IEDu5CzggPot4r3gQHxggGm8IFZ';

function firstPixel(png) {
  const parts = [];
  for (let offset = 8; offset < png.length;) {
    const size = png.readUInt32BE(offset), kind = png.toString('ascii', offset + 4, offset + 8);
    if (kind === 'IDAT') parts.push(png.subarray(offset + 8, offset + 8 + size));
    offset += size + 12;
  }
  assert.equal(png[24], 8); assert.ok([2, 6].includes(png[25]));
  return [...inflateSync(Buffer.concat(parts)).subarray(1, 4)];
}
test('real pinned Chrome captures selected retained media and fails closed', { skip: !program || !pin, timeout: 120000 }, async t => {
  assert.ok(isAbsolute(program));
  const executable = await readFile(program);
  assert.equal(hash(executable), pin, 'Use the explicitly supplied browser digest');
  const root = await mkdtemp(join(tmpdir(), 'gazette-card-test-'));
  await mkdir(join(root, 'payload'));
  const children = [];
  let active = 0, maximumActive = 0;
  const originalSpawn = childProcess.spawn;
  // Observe real launches and exits; the helper still owns every process and cleanup decision.
  childProcess.spawn = (...args) => {
    const child = originalSpawn(...args);
    const observed = { pid: child.pid, profile: args[1].find(arg => arg.startsWith('--user-data-dir=')).slice('--user-data-dir='.length), exited: false };
    children.push(observed); maximumActive = Math.max(maximumActive, ++active);
    child.once('exit', (code, signal) => { active--; Object.assign(observed, { exited: true, code, signal }); });
    return child;
  };
  syncBuiltinESMExports();
  const assets = [];
  const add = async (name, body, media_type, role) => {
    const bytes = Buffer.from(body);
    const asset = { url: `https://card.test/${name}`, entry: `payload/${name}`, media_type, role, bytes: bytes.length, sha256: hash(bytes) };
    await writeFile(join(root, asset.entry), bytes); assets.push(asset); return asset;
  };
  try {
    await add('image.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="red"/></svg>', 'image/svg+xml', 'media');
    await add('card.css', 'html,body{margin:0;width:200px;height:100px;background:white} [data-social-card-media]{width:20px;height:20px} h1{margin:0;font-size:12px}', 'text/css', 'stylesheet');
    const module = await add('card.mjs', `const slot=document.querySelector('[data-social-card-media]');slot.addEventListener('gazette:social-card-media-ready',()=>{if(slot.firstElementChild.dataset.treatment!=='natural')throw Error('lost treatment');slot.dispatchEvent(new CustomEvent('gazette:social-card-media-complete',{detail:{status:'complete'}}));},{once:true});`, 'text/javascript', 'module');
    const source = '<h1> Actual\n  source <em>title</em> </h1><img id="hero" src="/image.svg" data-treatment="natural"><img src="https://unretained.invalid/unselected"><script>throw Error("source executed")</script>';
    const html = body => `<!doctype html><html><head><link rel="stylesheet" href="/card.css"></head><body><template data-social-card-source>${body}</template><div data-social-card-media></div><h1 data-social-card-title>Initial</h1><script type="module" src="/card.mjs"></script></body></html>`;
    const input = () => ({ ...request(), browser: { program, bytes: executable.length, sha256: pin }, assets: structuredClone(assets), html: html(source) });
    await t.test('source heading, one media, inert source scripts and fixed PNG geometry', async () => {
      const result = JSON.parse(await capture(input(), root));
      assert.equal(result.title, 'Actual source title'); assert.equal(result.request_sha256, request().request_sha256);
      const png = Buffer.from(result.png_base64, 'base64');
      assert.equal(png.subarray(1, 4).toString(), 'PNG');
      assert.equal(png.readUInt32BE(16), 200); assert.equal(png.readUInt32BE(20), 100);
      assert.ok(png.length > 100);
      assert.deepEqual(firstPixel(png), [255, 0, 0]);
    });
    await t.test('absent heading uses exact fallback', async () => {
      const value = input(); value.html = html('<img id="hero" src="/image.svg" data-treatment="natural">');
      assert.equal(JSON.parse(await capture(value, root)).title, 'Fallback');
    });
    await t.test('ambiguous media, missing marker, source kind and unretained URL refuse', async () => {
      for (const body of [source + '<img id="hero" src="/image.svg">', '<h1>Title</h1>', '<a id="hero" href="/image.svg">Link</a>', '<img id="hero" src="file:///tmp/private">']) {
        const value = input(); value.html = html(body);
        await assert.rejects(capture(value, root));
      }
    });
    await t.test('card external scripts and retained media used as executable refuse', async () => {
      for (const src of ['https://unretained.invalid/script', '/image.svg']) {
        const value = input(); value.html = value.html.replace('/card.mjs', src);
        await assert.rejects(capture(value, root));
      }
    });
    await t.test('missing completion and bounded output never return partial capture', async () => {
      const value = input(); value.html = value.html.replace('<script type="module" src="/card.mjs"></script>', ''); value.limits.timeout_ms = 800;
      await assert.rejects(capture(value, root), /capture-timeout/);
      const small = input(); small.limits.max_output_bytes = 20;
      await assert.rejects(capture(small, root), /output-limit/);
    });
    await t.test('retained resource hash mismatch refuses', async () => {
      const value = input(); value.assets.find(asset => asset.url === module.url).sha256 = '0'.repeat(64);
      await assert.rejects(capture(value, root), /asset-binding/);
    });
    await t.test('explicit video anchor seeks the retained frame without filename inference', async () => {
      await add('clip', Buffer.from(video, 'base64'), 'video/webm', 'media');
      const value = input(); value.media_kind = 'video'; value.video_time_ms = 1250;
      value.html = html('<h1>Video title</h1><a id="hero" href="/clip" data-treatment="natural">Film</a>');
      const result = JSON.parse(await capture(value, root));
      assert.equal(result.title, 'Video title');
      const pixel = firstPixel(Buffer.from(result.png_base64, 'base64'));
      assert.ok(pixel[2] > 200 && pixel[0] < 80 && pixel[1] < 80, `Expected the blue sought frame, got ${pixel}`);
      value.video_time_ms = 0;
      const first = JSON.parse(await capture(value, root));
      const red = firstPixel(Buffer.from(first.png_base64, 'base64'));
      assert.ok(red[0] > 200 && red[1] < 80 && red[2] < 80, `Expected the red initial frame, got ${red}`);
      value.video_time_ms = 3000;
      await assert.rejects(capture(value, root));
    });
    await t.test('completion may follow asynchronous theme treatment', async () => {
      const original = await readFile(join(root, module.entry));
      const body = Buffer.from(`await new Promise(resolve=>setTimeout(resolve,40));const slot=document.querySelector('[data-social-card-media]');slot.addEventListener('gazette:social-card-media-ready',()=>setTimeout(()=>{slot.firstElementChild.hidden=true;slot.style.backgroundColor='blue';slot.dispatchEvent(new CustomEvent('gazette:social-card-media-complete',{detail:{status:'complete'}}));},40));`);
      await writeFile(join(root, module.entry), body); module.bytes = body.length; module.sha256 = hash(body);
      const treated = JSON.parse(await capture(input(), root));
      assert.equal(treated.title, 'Actual source title');
      assert.deepEqual(firstPixel(Buffer.from(treated.png_base64, 'base64')), [0, 0, 255]);
      await writeFile(join(root, module.entry), original); module.bytes = original.length; module.sha256 = hash(original);
    });
    await t.test('completion failures, malformed detail and blocked module effects refuse', async () => {
      const original = await readFile(join(root, module.entry));
      for (const action of [
        `slot.dispatchEvent(new CustomEvent('gazette:social-card-media-complete',{detail:{status:'failed',reason:'unsupported-treatment'}}))`,
        `slot.dispatchEvent(new CustomEvent('gazette:social-card-media-complete',{detail:{status:'complete',extra:true}}))`,
        `fetch('https://unretained.invalid/private').catch(()=>{});slot.dispatchEvent(new CustomEvent('gazette:social-card-media-complete',{detail:{status:'complete'}}))`,
      ]) {
        const body = Buffer.from(`const slot=document.querySelector('[data-social-card-media]');slot.addEventListener('gazette:social-card-media-ready',()=>{${action}});`);
        await writeFile(join(root, module.entry), body); module.bytes = body.length; module.sha256 = hash(body);
        await assert.rejects(capture(input(), root));
      }
      await writeFile(join(root, module.entry), original); module.bytes = original.length; module.sha256 = hash(original);
    });
  } finally {
    childProcess.spawn = originalSpawn; syncBuiltinESMExports();
    await rm(root, { recursive: true, force: true });
    assert.equal(maximumActive, 1, 'Only one owned browser child runs at a time');
    assert.equal(active, 0);
    for (const child of children) {
      assert.equal(child.exited, true);
      await assert.rejects(stat(child.profile), error => error.code === 'ENOENT');
    }
    t.diagnostic(JSON.stringify({ browser: { program, bytes: executable.length, sha256: pin }, maximum_active: maximumActive, children, profiles_removed: children.length }));
  }
});
