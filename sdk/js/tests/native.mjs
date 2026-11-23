// Real addon behavior; Rust reference JSON arrives on stdin, never through a CLI/MCP bridge.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 3, 'supply one exact local addon path');
const api = loadGazette(process.argv[2]);
const reference = JSON.parse(readFileSync(0, 'utf8'));
const source = Buffer.from(readFileSync(new URL('./fixtures/workspace.toml', import.meta.url), 'utf8').replaceAll('\n', '\r\n'));
const queries = JSON.parse(readFileSync(new URL('./fixtures/queries.json', import.meta.url), 'utf8'));
const query = name => JSON.stringify(queries[name]);
const fixture = name => Buffer.from(readFileSync(new URL(`./fixtures/${name}.md`, import.meta.url), 'utf8').replaceAll('\n', '\r\n'));
const markdown = fixture('markdown');
const document = fixture('document');
const pagePackageWorkspace = readFileSync(new URL('./fixtures/page-package-workspace.toml', import.meta.url));
const pagePackage = fixture('page-package');
const pagePackageStandalone = fixture('page-package-standalone');
const pagePackageImage = readFileSync(new URL('./fixtures/page-package-image.png', import.meta.url));

const packageRequest = queryPath => ({
  workspace: {path:'workspace.toml', bytes:pagePackageWorkspace},
  documents: [
    {path:'content/guide/index.md', bytes:pagePackage},
    {path:'content/standalone.md', bytes:pagePackageStandalone},
  ],
  companions: [{path:'content/guide/image.png', bytes:pagePackageImage}],
  query: queryPath,
});

function failure(call, code) {
  let caught;
  try { call(); } catch (error) { caught = error; }
  assert.ok(caught instanceof Error, 'a failure must not return a partial model');
  const value = JSON.parse(caught.message);
  assert.equal(value.code, code);
  return value;
}

test('complete supplied workspace matches native Rust without reading declared locations', () => {
  const result = api.checkWorkspace(source);
  assert.equal(typeof result, 'string', 'the boundary does not convert Rust numbers');
  assert.deepEqual(JSON.parse(result), reference.check);
  assert.equal(reference.check.source_sha256, createHash('sha256').update(source).digest('hex'));
  assert.equal(api.checkWorkspace(source), result);
  const copied = new Uint8Array(source);
  const retained = api.checkWorkspace(copied);
  copied.fill(0);
  assert.equal(retained, result, 'caller mutation cannot modify a completed native result');
});

test('field origins preserve Unicode CRLF and original unsorted declaration coordinates', () => {
  const value = JSON.parse(api.fieldOrigin(source, query('field')));
  assert.deepEqual(value, reference.field);
  assert.equal(source.subarray(value.span.start, value.span.end).toString(), "'Rêve fixture'");
  assert.equal(value.source_sha256, reference.check.source_sha256);
});

test('composition origins preserve independent resource, Theme and capability declaration origins', () => {
  for (const name of ['inherited', 'overridden', 'disabled']) {
    assert.deepEqual(JSON.parse(api.compositionOrigin(source, query(name))), reference[name]);
  }
  assert.equal(reference.inherited.selection.field.kind, 'resource');
  assert.equal(reference.overridden.selection.field.kind, 'theme');
  assert.equal(reference.disabled.selection.field.kind, 'capability');
  assert.equal(source.subarray(reference.inherited.span.start, reference.inherited.span.end).toString(), "'dependency'");
  assert.equal(source.subarray(reference.overridden.span.start, reference.overridden.span.end).toString(), "'theme:other'");
  assert.equal(source.subarray(reference.disabled.span.start, reference.disabled.span.end).toString(), "'disabled'");
});

test('native invalid graph and absent/default fields retain shared failures without partial models', () => {
  const invalid = Buffer.from(source.toString().replaceAll("publisher='owner:one'", "publisher='owner:missing'"));
  assert.deepEqual(failure(() => api.checkWorkspace(invalid), 'workspace_invalid'), reference.invalid);
  failure(() => api.fieldOrigin(source, JSON.stringify({kind:'brand',identity:'brand:one',field:['publications']})), 'origin_no_explicit_field');
  failure(() => api.fieldOrigin(source, JSON.stringify({kind:'site',identity:'site:missing',field:['id']})), 'origin_unknown_identity');
  const syntax = failure(() => api.checkWorkspace(Buffer.from("[[sites]\n")), 'toml_invalid');
  assert.ok(syntax.span && Number.isInteger(syntax.span.start));
});

test('input/query/result bounds and malformed queries fail explicitly and recover independently', () => {
  const result = api.checkWorkspace(source);
  assert.equal(api.checkWorkspace(source, {sourceBytes:source.length,resultBytes:Buffer.byteLength(result)}), result);
  failure(() => api.checkWorkspace(source, {sourceBytes:source.length-1}), 'toml_limit');
  failure(() => api.checkWorkspace(source, {nodes:0}), 'toml_limit');
  failure(() => api.checkWorkspace(source, {depth:0}), 'toml_limit');
  failure(() => api.checkWorkspace(source, {resultBytes:Buffer.byteLength(result)-1}), 'node_result_limit');
  failure(() => api.checkWorkspace(Buffer.from([0xff])), 'node_source_utf8');
  failure(() => api.fieldOrigin(source, query('field'), {queryBytes:0}), 'node_query_limit');
  assert.deepEqual(JSON.parse(api.fieldOrigin(source, query('field'), {queryBytes:Buffer.byteLength(query('field'))})), reference.field);
  for (const value of ['[]', '{}', 'null', '{"kind":"brand","kind":"site"}', query('field')+' trailing', query('field').replace('"kind":', '"unknown":')]) {
    failure(() => api.fieldOrigin(source, value), 'node_query');
  }
  failure(() => api.fieldOrigin(source, '{"kind":"brand","k\\u0069nd":"brand","identity":"brand:one","field":["name"]}'), 'node_query');
  failure(() => api.compositionOrigin(source, '{"site":"site:a","field":{"kind":"theme","k\\u0069nd":"theme"},"path":[]}'), 'node_query');
  failure(() => api.compositionOrigin(source, '{"site":"site:a","field":{"kind":"module","module":"module:search"},"path":["state"]}'), 'node_query');
  for (const value of [-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER+1]) {
    failure(() => api.checkWorkspace(source, {sourceBytes:value}), 'node_limits');
  }
  assert.equal(api.checkWorkspace(source), result);
});

test('loading is explicit and returns only this bounded local native API', () => {
  assert.throws(() => loadGazette('gazette.node'), /absolute/);
  assert.throws(() => loadGazette('/tmp/not-an-addon.js'), /absolute/);
  assert.deepEqual(Object.keys(api).sort(), ['assembleDocumentation','checkWorkspace','compositionOrigin','fieldOrigin','inspectDocument','inspectDocumentTree','inspectMarkdown','inspectMarkdownTree','inspectPagePackage','inspectReleaseHistory','observeDelivery','prepareSites','qualifySites','renderSiteFiles','renderSites','reviewPublication']);
  assert.ok(Object.isFrozen(api));
  assert.throws(() => { api.renderSiteFiles = () => ({}); }, TypeError);
});

test('page-package inspection projects the shared retained-byte ownership contract', () => {
  for (const [name, path] of [
    ['document', 'content/guide/index.md'],
    ['companion', 'content/guide/image.png'],
    ['standalone', 'content/standalone.md'],
  ]) {
    const result = api.inspectPagePackage(packageRequest(path));
    assert.equal(typeof result, 'string');
    assert.deepEqual(JSON.parse(result), reference.pagePackage[name]);
    assert.equal(api.inspectPagePackage(packageRequest(path)), result, 'supplied-byte query is repeatable');
  }
  assert.equal(reference.pagePackage.document.role.kind, 'document');
  assert.equal(reference.pagePackage.companion.role.kind, 'companion');
  assert.equal(reference.pagePackage.standalone, null);
  assert.equal(reference.pagePackage.document.package.document, 'content/guide/index.md');
  assert.equal(reference.pagePackage.document.package.companions[0].path, 'content/guide/image.png');
  assert.ok(Array.isArray(reference.pagePackage.document.resources), 'compiler-validated resources remain in the ownership projection');
});

test('page-package inspection rejects changed selections and independent host bounds', () => {
  failure(() => api.inspectPagePackage(packageRequest('content/missing.md')), 'authoring_source');
  const invalidUtf8 = packageRequest('content/guide/index.md');
  invalidUtf8.documents[0].bytes = new Uint8Array([0xff]);
  failure(() => api.inspectPagePackage(invalidUtf8), 'authoring_source_utf8');
  const exact = api.inspectPagePackage(packageRequest('content/guide/index.md'));
  const retainedBytes = pagePackageWorkspace.length + pagePackage.length + pagePackageStandalone.length + pagePackageImage.length;
  assert.equal(api.inspectPagePackage(packageRequest('content/guide/index.md'), {
    selectionEntries:4, sourceBytes:retainedBytes, resultBytes:Buffer.byteLength(exact),
  }), exact);
  failure(() => api.inspectPagePackage(packageRequest('content/guide/index.md'), {selectionEntries:3}), 'authoring_session_limit');
  failure(() => api.inspectPagePackage(packageRequest('content/guide/index.md'), {sourceBytes:retainedBytes-1}), 'authoring_session_limit');
  failure(() => api.inspectPagePackage(packageRequest('content/guide/index.md'), {resultBytes:Buffer.byteLength(exact)-1}), 'node_result_limit');
});

test('supplied Markdown and framed document projections match the shared Rust SDK including HTML', () => {
  for (const [name, inspect, bytes] of [
    ['markdown', api.inspectMarkdown, markdown], ['document', api.inspectDocument, document],
  ]) {
    const result = inspect(bytes);
    assert.equal(typeof result, 'string');
    assert.deepEqual(JSON.parse(result), reference[name]);
    assert.ok(!Object.hasOwn(JSON.parse(result), 'html'), 'HTML omission is explicit');
    assert.deepEqual(JSON.parse(inspect(bytes, {html:true})), reference[name+'Html']);
    assert.equal(inspect(bytes), result, 'native inspection is repeatable');
    const copied = new Uint8Array(bytes);
    const retained = inspect(copied);
    copied.fill(0);
    assert.equal(retained, result, 'result is independent of later caller mutation');
  }
  assert.equal(reference.markdown.inspection.source_sha256, createHash('sha256').update(markdown).digest('hex'));
  assert.equal(reference.document.document.source_sha256, createHash('sha256').update(document).digest('hex'));
  const body = reference.document.document.body_span;
  assert.equal(document.subarray(body.start, body.end).toString(), '# Café 🦀\r\n\r\nBody with **meaning**.\r\n');
  const heading = reference.document.document.markdown.headings[0];
  assert.ok(document.subarray(body.start + heading.span.start, body.start + heading.span.end).toString().includes('Café 🦀'));
  const bom = Buffer.concat([Buffer.from([0xef,0xbb,0xbf]), markdown]);
  assert.equal(JSON.parse(api.inspectMarkdown(bom)).inspection.source_sha256, createHash('sha256').update(bom).digest('hex'));
});

test('explicit framing and optional HTML failures produce no partial report or private payload', () => {
  failure(() => api.inspectDocument(markdown), 'document_framing');
  const raw = Buffer.from('<b>private-canary</b>');
  assert.ok(JSON.parse(api.inspectMarkdown(raw)).inspection);
  const rejected = failure(() => api.inspectMarkdown(raw, {html:true}), 'markdown_raw_html_unavailable');
  assert.deepEqual(rejected, {code:'markdown_raw_html_unavailable'});
  const framed = Buffer.from("+++\ntitle='private-canary'\n+++\n<b>private-canary</b>");
  assert.ok(JSON.parse(api.inspectDocument(framed)).document);
  assert.deepEqual(failure(() => api.inspectDocument(framed, {html:true}), 'markdown_raw_html_unavailable'), rejected);
  failure(() => api.inspectDocument(Buffer.from('+++\n[invalid\n+++\n# Body')), 'toml_invalid');
  failure(() => api.inspectMarkdown(Buffer.from('[x](javascript:alert(1))'), {html:true}), 'markdown_unsafe_url');
  for (const inspect of [api.inspectMarkdown, api.inspectDocument]) {
    failure(() => inspect(Buffer.from([0xff])), 'node_source_utf8');
  }
  assert.deepEqual(JSON.parse(api.inspectDocument(document)), reference.document);
});

test('Markdown/document native and host ceilings are independent and exact', () => {
  const result = api.inspectMarkdown(markdown);
  assert.equal(api.inspectMarkdown(markdown, {limits:{sourceBytes:markdown.length},resultBytes:Buffer.byteLength(result)}), result);
  failure(() => api.inspectMarkdown(markdown, {limits:{sourceBytes:markdown.length-1}}), 'markdown_limit');
  failure(() => api.inspectMarkdown(markdown, {resultBytes:Buffer.byteLength(result)-1}), 'node_result_limit');
  for (const key of ['nodes','depth','factBytes']) {
    failure(() => api.inspectMarkdown(markdown, {limits:{[key]:0}}), 'markdown_limit');
  }
  failure(() => api.inspectMarkdown(markdown, {limits:{outputBytes:0}}), 'markdown_limit');
  const framed = api.inspectDocument(document);
  assert.equal(api.inspectDocument(document, {sourceBytes:document.length,resultBytes:Buffer.byteLength(framed)}), framed);
  failure(() => api.inspectDocument(document, {sourceBytes:document.length-1}), 'document_limit');
  failure(() => api.inspectDocument(document, {resultBytes:Buffer.byteLength(framed)-1}), 'node_result_limit');
  for (const key of ['sourceBytes','nodes','depth']) {
    failure(() => api.inspectDocument(document, {metadata:{[key]:0}}), 'toml_limit');
  }
  failure(() => api.inspectDocument(document, {markdown:{sourceBytes:0}}), 'markdown_limit');
  failure(() => api.inspectDocument(document, {markdown:{outputBytes:0}}), 'markdown_limit');
  for (const invalid of [-1,0.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1]) {
    failure(() => api.inspectMarkdown(markdown, {limits:{factBytes:invalid}}), 'node_limits');
    failure(() => api.inspectDocument(document, {metadata:{nodes:invalid}}), 'node_limits');
  }
});
