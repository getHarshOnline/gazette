const {test} = require('node:test');
const assert = require('node:assert/strict');
const {createHash} = require('node:crypto');
const {captureEditorSnapshot, exportEditorContext} = require('../dist/editorSnapshot');

function fixture() {
  const input = [
    ['root', 'workspace', Buffer.from("# current unsaved 🦀\n"), 12],
    ['module', 'workspace_document', Buffer.from("sites=[]\n"), null],
    ['page', 'document', Buffer.from('+++\ntitle="é"\n+++\n' + '🦀'.repeat(20000)), 19],
    ['asset', 'page_input', Buffer.from([0, 255, 128, 10, 13]), null],
    ['theme', 'theme', Buffer.from("id='theme:a'\n"), null],
  ];
  const manifest = {generation: 7, structurally_valid: false, modular: true, sources: input.map(([id, kind, bytes, version]) => ({
    id, path: id, role: {kind, ...(kind === 'theme' ? {package: 'theme:a'} : {})}, version, bytes: bytes.length,
    source_sha256: createHash('sha256').update(bytes).digest('hex'),
  }))};
  const host = {changed: false, corrupt: false, async sendRequest(method, params) {
    if (method === 'gazette/editorSnapshot') return {...manifest, generation: host.changed ? 8 : 7};
    assert.equal(params.generation, 7); assert.ok(params.length <= 65536);
    const bytes = input.find(([id]) => id === params.document)[2].subarray(params.offset, params.offset + params.length);
    return {generation: 7, document: params.document, offset: params.offset, bytes_base64: (host.corrupt ? Buffer.from('wrong') : bytes).toString('base64')};
  }};
  return {host, input};
}

test('exact native editor snapshot preserves roles, unsaved versions, Unicode split across chunks and opaque bytes', async () => {
  const {host, input} = fixture();
  const result = await captureEditorSnapshot(host);
  assert.equal(result.start.workspace.text, input[0][2].toString());
  assert.equal(result.start.documents[0].text, input[2][2].toString());
  assert.equal(result.start.workspace_documents.length, 1);
  assert.equal(result.start.package_baselines[0].kind, 'theme');
  assert.deepEqual(Buffer.from(result.start.page_inputs[0].bytes_base64, 'base64'), input[3][2]);
  assert.deepEqual(result.overlays.map(value => [value.id, value.version]), [['root', 12], ['page', 19]]);
  assert.equal(await result.current(), true); host.changed = true; assert.equal(await result.current(), false);
});
test('a corrupted chunk is rejected rather than importing partial content into MCP', async () => {
  const {host} = fixture(); host.corrupt = true;
  await assert.rejects(captureEditorSnapshot(host), /digest mismatch|Incomplete/);
});

test('capture progress counts only sources whose bytes were received and checked', async () => {
  const {host, input} = fixture();
  const reports = [];
  host.corrupt = true;
  await assert.rejects(captureEditorSnapshot(host, (completed, total) => reports.push([completed, total])));
  assert.deepEqual(reports, [[0, input.length]]);
  host.corrupt = false;
  reports.length = 0;
  await captureEditorSnapshot(host, (completed, total) => reports.push([completed, total]));
  assert.deepEqual(reports.at(-1), [input.length, input.length]);
});

test('portable context preserves complete input roles, active identity and unsaved versions, and refuses stale exports', async () => {
  const {host} = fixture();
  const snapshot = await captureEditorSnapshot(host);
  const exported = JSON.parse(await exportEditorContext(snapshot, 'file:///owner', 'page', 0));
  assert.equal(exported.format, 'gazette-editor-context/1');
  assert.equal(exported.active.version, 19);
  assert.equal(exported.active.source_sha256, snapshot.manifest.sources.find(s => s.id === 'page').source_sha256);
  assert.deepEqual(exported.start, snapshot.start);
  assert.deepEqual(exported.overlays, snapshot.overlays);
  await assert.rejects(exportEditorContext(snapshot, 'file:///owner', 'unknown', 0), /not in this snapshot/);
  await assert.rejects(exportEditorContext(snapshot, 'file:///owner', 'asset', 0), /not in this snapshot/);
  host.changed = true;
  await assert.rejects(exportEditorContext(snapshot, 'file:///owner', 'page', 0), /changed before export/);
});
test('a changed generation at the final fence refuses the complete handoff', async () => {
  const {host} = fixture(); let requests = 0; const send = host.sendRequest.bind(host);
  host.sendRequest = async (...args) => { if (args[0] === 'gazette/editorSnapshot' && ++requests > 1) host.changed = true; return send(...args); };
  await assert.rejects(captureEditorSnapshot(host), /changed/);
});

function nativeFixture() {
  const {host, input} = fixture();
  const send = host.sendRequest.bind(host);
  const reads = [];
  host.sendRequest = async (method, params) => {
    const result = await send(method, params);
    if (method === 'gazette/editorSnapshot') {
      result.native_selection = {root: '/owner', workspace: 'gazette.toml', manuals: [], configuration: {}};
      result.asset_observations = {'file:///owner/video.mp4': {path: 'video.mp4', observed_bytes: 64 * 2 ** 30}};
      result.sources = result.sources.map(source => ({...source, baseline_sha256: source.id === 'page' ? 'a'.repeat(64) : source.source_sha256}));
    } else reads.push(params.document);
    return result;
  };
  return {host, input, reads};
}

test('native discovery cannot fall back to the full-copy path of an older server', async () => {
  const {host} = fixture();
  await assert.rejects(captureEditorSnapshot(host, undefined, true), /version 2/);
  assert.ok(await captureEditorSnapshot(host), 'explicit supplied-buffer sessions keep version 1');
});

test('native handoff transfers only changed text, retaining clean open versions, membership and asset observations', async () => {
  const {host, reads} = nativeFixture();
  const snapshot = await captureEditorSnapshot(host);
  const context = JSON.parse(await exportEditorContext(snapshot, 'file:///owner', 'page', 0));
  assert.equal(context.format, 'gazette-editor-context/2');
  assert.deepEqual([...new Set(reads)], ['page']);
  assert.equal(context.manifest.sources.length, 5);
  assert.equal(context.manifest.sources.find(source => source.id === 'root').version, 12);
  assert.equal(context.overlays.length, 1);
  assert.equal(context.overlays[0].id, 'page');
  assert.ok(context.manifest.asset_observations);
  assert.equal(context.start, undefined);
  assert.equal(context.allow_native_reads, undefined, 'an export cannot grant its own receiving-side read permission');
  assert.equal(context.manifest.native_selection, undefined);
});

test('saved sources above the old full-copy ceiling need no text transfer', async () => {
  const {host, reads} = nativeFixture();
  const send = host.sendRequest.bind(host);
  host.sendRequest = async (method, params) => {
    const result = await send(method, params);
    if (method === 'gazette/editorSnapshot') {
      const clean = result.sources.find(source => source.id === 'module');
      clean.bytes = 13 * 1024 * 1024;
    }
    return result;
  };
  const result = await captureEditorSnapshot(host);
  assert.equal(result.overlays.length, 1);
  assert.ok(!reads.includes('module'));
});

test('a missing native baseline or unversioned changed source refuses export without full-copy fallback', async () => {
  for (const failure of ['baseline', 'version']) {
    const {host, reads} = nativeFixture();
    const send = host.sendRequest.bind(host);
    host.sendRequest = async (method, params) => {
      const result = await send(method, params);
      if (method === 'gazette/editorSnapshot') {
        const page = result.sources.find(source => source.id === 'page');
        if (failure === 'baseline') delete page.baseline_sha256;
        else page.version = null;
      }
      return result;
    };
    await assert.rejects(captureEditorSnapshot(host), /baseline|overlay/);
    assert.deepEqual(reads, []);
  }
});
