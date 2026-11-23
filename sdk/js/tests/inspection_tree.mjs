// Real addon against the same complete-tree owner, not a JS capture or parser.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 4, 'supply exact addon and inspection_reference paths');
const [, , addon, referenceExecutable] = process.argv;
assert.ok(isAbsolute(referenceExecutable));
const api = loadGazette(addon);
const allowed = { allowRead: true };
const hash = value => createHash('sha256').update(value).digest('hex');

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'gazette-node-inspection-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const page = Buffer.from('# Café 🦀\r\n\r\n**Original** bytes.\r\n');
  const framed = Buffer.from(readFileSync(new URL('./fixtures/document.md', import.meta.url), 'utf8').replaceAll('\n', '\r\n'));
  writeFileSync(join(root, 'page.md'), page);
  writeFileSync(join(root, 'framed.md'), framed);
  writeFileSync(join(root, '.asset'), Buffer.alloc(4096, 7));
  writeFileSync(join(root, 'binary'), Buffer.from([0xff]));
  mkdirSync(join(root, 'directory'));
  const reference = JSON.parse(execFileSync(referenceExecutable, [root], { encoding: 'utf8', maxBuffer: 4 << 20 }));
  return { root, page, framed, reference };
}

function failure(call, code) {
  let caught;
  try { call(); } catch (error) { caught = error; }
  assert.ok(caught instanceof Error, 'failure cannot yield a partial successful projection');
  const value = JSON.parse(caught.message);
  if (code !== undefined) assert.deepEqual(value, { code });
  return value;
}

test('complete pinned Markdown and framed documents match direct Rust evidence and HTML', t => {
  const f = fixture(t);
  for (const [name, inspect, supplied, bytes] of [
    ['markdown', api.inspectMarkdownTree, api.inspectMarkdown, f.page],
    ['document', api.inspectDocumentTree, api.inspectDocument, f.framed],
  ]) {
    const selected = JSON.stringify(f.reference[name].selection);
    for (const html of [false, true]) {
      const raw = inspect(selected, allowed, { html });
      assert.equal(typeof raw, 'string');
      const report = JSON.parse(raw);
      assert.deepEqual(report, f.reference[name][html ? 'html' : 'plain']);
      assert.equal(inspect(selected, allowed, { html }), raw);
      const { input, ...projection } = report;
      assert.deepEqual(input.identity, f.reference[name].selection.identity);
      assert.deepEqual(projection, JSON.parse(supplied(bytes, { html })));
      assert.equal((report.inspection ?? report.document).source_sha256, hash(bytes));
      assert.equal(inspect(selected, allowed, { html, resultBytes: Buffer.byteLength(raw) }), raw);
      failure(() => inspect(selected, allowed, { html, resultBytes: Buffer.byteLength(raw) - 1 }), 'node_result_limit');
    }
  }
});

test('permission and strict native selection precede capture, with independent bounded JSON', t => {
  const f = fixture(t);
  const selected = f.reference.markdown.selection;
  const raw = JSON.stringify(selected);
  failure(() => api.inspectMarkdownTree(JSON.stringify({ ...selected, root: join(f.root, 'missing') }), { allowRead: false }), 'markdown_read_permission');
  failure(() => api.inspectDocumentTree('{}', { allowRead: false }), 'markdown_read_permission');
  assert.throws(() => api.inspectMarkdownTree(raw), 'native permission object is mandatory');
  assert.equal(api.inspectMarkdownTree(raw, { ...allowed, queryBytes: Buffer.byteLength(raw) }), api.inspectMarkdownTree(raw, allowed));
  failure(() => api.inspectMarkdownTree(raw, { ...allowed, queryBytes: Buffer.byteLength(raw) - 1 }), 'node_query_limit');
  for (const query of [
    '[]', 'null', raw + ' trailing', '{"root":"x","r\\u006fot":"x"}',
    JSON.stringify({ ...selected, unknown: true }),
    JSON.stringify({ ...selected, root: '.' }),
    JSON.stringify({ ...selected, document: '../page.md' }),
    JSON.stringify({ ...selected, document: '/page.md' }),
    JSON.stringify({ ...selected, limits: [] }),
    JSON.stringify({ ...selected, identity: [] }),
    JSON.stringify({ ...selected, limits: {entries:10,depth:2,file_bytes:9007199254740993,total_bytes:4096,extra:true} }),
    raw.replace('"identity":{', '"identity":{"format":"duplicate",'),
  ]) failure(() => api.inspectMarkdownTree(query, allowed), 'node_query');
  for (const value of [-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
    failure(() => api.inspectMarkdownTree(raw, { ...allowed, queryBytes: value }), 'node_limits');
  }
});

test('capture includes hidden assets and never falls back to otherwise valid page bytes', t => {
  const f = fixture(t);
  const selection = JSON.stringify(f.reference.markdown.selection);
  const original = api.inspectMarkdownTree(selection, allowed, {html:true});
  writeFileSync(join(f.root, '.asset'), Buffer.alloc(4096, 8));
  assert.equal(readFileSync(join(f.root, 'page.md')).equals(f.page), true);
  failure(() => api.inspectMarkdownTree(selection, allowed));
  assert.deepEqual(JSON.parse(original), f.reference.markdown.html, 'earlier returned evidence retains its original pin and bytes');
  writeFileSync(join(f.root, 'page.md'), '# Later\n');
  failure(() => api.inspectMarkdownTree(selection, allowed));
  assert.match(JSON.parse(original).html, /Original/);
});

test('native capture, selected UTF-8, framing and parser bounds remain independent', t => {
  const f = fixture(t);
  const selected = f.reference.markdown.selection;
  for (const [document, code] of [['binary','markdown_input'],['directory','markdown_document'],['absent','markdown_document']]) {
    failure(() => api.inspectMarkdownTree(JSON.stringify({ ...selected, document }), allowed), code);
  }
  const bounds = { entries:100, depth:10, file_bytes:8192, total_bytes:16384 };
  const exact = { ...bounds, file_bytes:4096, total_bytes:f.page.length + f.framed.length + 4097 };
  assert.deepEqual(JSON.parse(api.inspectMarkdownTree(JSON.stringify({ ...selected, limits:exact }), allowed)), f.reference.markdown.plain);
  for (const changed of [{entries:1},{depth:0},{file_bytes:4095},{total_bytes:4096}]) {
    failure(() => api.inspectMarkdownTree(JSON.stringify({ ...selected, limits:{...bounds,...changed} }), allowed));
  }
  const raw = JSON.stringify(selected);
  failure(() => api.inspectMarkdownTree(raw, allowed, {limits:{sourceBytes:f.page.length-1}}), 'markdown_limit');
  failure(() => api.inspectMarkdownTree(raw, allowed, {limits:{nodes:0}}), 'markdown_limit');
  failure(() => api.inspectDocumentTree(raw, allowed));
  failure(() => api.inspectDocumentTree(JSON.stringify(f.reference.document.selection), allowed, {sourceBytes:f.framed.length-1}), 'document_limit');
  assert.deepEqual(JSON.parse(api.inspectMarkdownTree(raw, allowed)), f.reference.markdown.plain);
});
