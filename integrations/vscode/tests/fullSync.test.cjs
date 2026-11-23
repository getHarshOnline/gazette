const assert = require('node:assert/strict');
const { test } = require('node:test');
const { createEditor, Uri } = require('./support/editor.cjs');

async function fixture() {
  const host = createEditor(['/work/one']);
  const folder = host.vscode.workspace.workspaceFolders[0];
  host.settings.set(folder.uri.toString(), { 'workspace.documents': ['guide.md'] });
  const session = new host.WorkspaceSession(folder, { appendLine: value => host.logs.push(value) });
  await session.refresh();
  const document = { uri: new Uri('/work/one/guide.md'), version: 1, isClosed: false,
    text: '# é😀\r\n', getText() { return this.text; } };
  host.vscode.workspace.textDocuments.push(document);
  const client = session.client;
  const change = () => client.clientOptions.middleware.didChange({ document, contentChanges: [{ text: 'incremental event is not the source' }] },
    () => assert.fail('native delayed full-sync path must not be entered'));
  const query = forward => client.clientOptions.middleware.provideDefinition(document, {}, { isCancellationRequested: false }, forward);
  return { host, session, client, document, change, query };
}

test('rapid full changes snapshot exact native URI/version/Unicode source and queries await delivery', async () => {
  const f = await fixture();
  const releases = [];
  f.host.delivery = () => new Promise(resolve => releases.push(resolve));
  try {
    f.document.version = 2;
    const firstText = f.document.text = '# First é😀\r\n';
    const first = f.change();
    f.document.version = 3;
    const secondText = f.document.text = '# Latest 🦀\r\n';
    const second = f.change();
    let forwarded = false;
    const queried = f.query(() => { forwarded = true; return 'native current result'; });
    assert.deepEqual(f.client.sent, [
      { method: 'textDocument/didChange', params: { textDocument: { uri: f.document.uri.toString(), version: 2 }, contentChanges: [{ text: firstText }] } },
      { method: 'textDocument/didChange', params: { textDocument: { uri: f.document.uri.toString(), version: 3 }, contentChanges: [{ text: secondText }] } },
    ]);
    assert.equal(forwarded, false);
    releases[0](); await first;
    assert.equal(forwarded, false, 'first delivery is not the latest selected source');
    releases[1](); await second;
    assert.equal(await queried, 'native current result');
  } finally { await f.session.shutdown(); }
});

test('retirement refuses new changes and stale queries but waits accepted bytes before shutdown', async () => {
  const f = await fixture();
  let release;
  f.host.delivery = () => new Promise(resolve => { release = resolve; });
  const changed = f.change();
  const queried = f.query(() => assert.fail('retired query forwarded'));
  f.client.retire();
  const stopping = f.session.shutdown();
  await f.host.flush();
  assert.equal(f.host.stops.length, 0, 'protocol shutdown cannot overtake owned full replacement');
  await f.change();
  assert.equal(f.client.sent.length, 1);
  release(); await changed;
  assert.equal(await queried, null);
  await stopping;
  assert.equal(f.host.stops.length, 1);
});

test('failed full delivery persists through later requests and proven child cleanup', async () => {
  const f = await fixture();
  const failure = new Error('native full replacement writer failed');
  f.host.delivery = () => Promise.reject(failure);
  await assert.rejects(f.change(), error => error === failure);
  await assert.rejects(f.query(() => assert.fail('failed state forwarded')), error => error === failure);
  await assert.rejects(f.change(), error => error === failure);
  assert.equal(f.client.sent.length, 1);
  await assert.rejects(f.session.shutdown(), error => error === failure);
  assert.equal(f.session.client, f.client, 'uncertain semantics never silently release custody');
});

test('an unproven writer is bounded at shutdown and remains explicitly refused', async () => {
  const f = await fixture();
  let release;
  f.host.delivery = () => new Promise(resolve => { release = resolve; });
  const changed = f.change();
  f.client.retire();
  await assert.rejects(f.client.stop(10), /full-change delivery remains unproven/);
  release(); await changed;
  await assert.rejects(f.client.stop(10), /full-change delivery remains unproven/);
});

test('unselected changes and unsupported incremental servers cannot use the Full bridge', async () => {
  const f = await fixture();
  try {
    f.document.uri = new Uri('/work/one/unselected.md');
    await f.change();
    assert.equal(f.client.sent.length, 0);
    f.document.uri = new Uri('/work/one/guide.md');
    f.client.initializeResult.capabilities.textDocumentSync = 2;
    await assert.rejects(f.change(), /full-document synchronization/);
    assert.equal(f.client.sent.length, 0);
  } finally { await f.session.shutdown(); }
});
