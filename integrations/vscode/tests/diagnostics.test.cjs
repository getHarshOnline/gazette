const assert = require('node:assert/strict');
const { test } = require('node:test');
const { createEditor, Uri } = require('./support/editor.cjs');

async function fixture() {
  const host = createEditor(['/work/one']);
  await host.extension.activate({ subscriptions: [] });
  const uri = new Uri('/work/one/gazette.toml');
  const document = { uri, version: 4, isClosed: false };
  host.vscode.workspace.textDocuments.push(document);
  return { host, client: host.clients[0], uri: uri.toString(), document };
}

test('diagnostics require exact selection and current open version, including empty results', async () => {
  const { host, client, uri, document } = await fixture();
  for (const diagnostics of [[], [{ message: 'old' }]]) {
    client.notifyDiagnostics({ uri, version: 3, diagnostics });
    client.notifyDiagnostics({ uri, diagnostics });
    client.notifyDiagnostics({ uri: new Uri('/work/one/other.toml').toString(), version: 4, diagnostics });
  }
  await host.flush();
  assert.deepEqual(client.published, []);
  client.notifyDiagnostics({ uri, version: 4, diagnostics: [{ message: 'current' }] });
  await host.flush();
  assert.equal(client.published[0].diagnostics[0].message, 'current');
  client.notifyDiagnostics({ uri, version: 4, diagnostics: [] });
  await host.flush();
  assert.deepEqual(client.published[1].diagnostics, []);
  document.isClosed = true;
  client.notifyDiagnostics({ uri, version: 4, diagnostics: [{ message: 'closed overlay' }] });
  client.notifyDiagnostics({ uri, diagnostics: [{ message: 'baseline' }] });
  await host.flush();
  assert.equal(client.published.length, 3);
  assert.equal(client.published[2].diagnostics[0].message, 'baseline');
  await host.extension.deactivate();
});

test('conversion rechecks versions and coalesces arrivals so late results cannot replace newer ones', async () => {
  const { host, client, uri, document } = await fixture();
  let release;
  let conversions = 0;
  const delayed = new Promise(resolve => { release = resolve; });
  client.protocol2CodeConverter.asDiagnostics = async values => {
    conversions++;
    if (conversions === 1) await delayed;
    return values;
  };
  client.notifyDiagnostics({ uri, version: 4, diagnostics: [{ message: 'late' }] });
  await host.flush();
  document.version = 5;
  for (let index = 0; index < 20; index++) client.notifyDiagnostics({ uri, version: 5, diagnostics: [{ message: `new ${index}` }] });
  client.notifyDiagnostics({ uri, version: 5, diagnostics: [] });
  assert.equal(conversions, 1);
  release();
  await host.flush();
  assert.equal(conversions, 2, 'one active conversion plus latest packet, not an unbounded queue');
  assert.equal(client.published.length, 1);
  assert.deepEqual(client.published[0].diagnostics, []);
  await host.extension.deactivate();
});

test('close/reopen identity and retired sessions invalidate in-flight conversion', async () => {
  for (const transition of ['reopen', 'retire']) {
    const { host, client, uri, document } = await fixture();
    let release;
    const delayed = new Promise(resolve => { release = resolve; });
    client.protocol2CodeConverter.asDiagnostics = async values => { await delayed; return values; };
    client.notifyDiagnostics({ uri, version: 4, diagnostics: [] });
    await host.flush();
    if (transition === 'reopen') {
      document.isClosed = true;
      host.vscode.workspace.textDocuments.push({ ...document, isClosed: false });
    } else client.retire();
    release();
    await host.flush();
    assert.deepEqual(client.published, []);
    await host.extension.deactivate();
  }
});

test('structured resource collisions and documentation closure evidence survive the VS Code bridge', async () => {
  const { host, client, uri } = await fixture();
  const diagnostics = [
    {
      range: { start: { line: 4, character: 0 }, end: { line: 4, character: 24 } },
      severity: 1,
      code: 'resource_input',
      source: 'gazette',
      message: 'page resources claim the same output',
      data: { generation: 8, source_sha256: 'sha256:current', output: '/same.js' },
    },
    {
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
      severity: 3,
      code: 'documentation_validation_incomplete',
      source: 'gazette',
      message: 'complete retained manual/reference trees are unavailable in the editor session',
      data: {
        generation: 8,
        documentation: {
          manual_pages: [{ source: 'source:docs', route: 'guide/start', input: { kind: 'file', path: 'docs/start.md' } }],
          closure: { status: 'unavailable' },
        },
      },
    },
  ];
  client.notifyDiagnostics({ uri, version: 4, diagnostics });
  await host.flush();
  assert.deepEqual(client.published[0].diagnostics, diagnostics);
  await host.extension.deactivate();
});

test('removing any explicitly selected root invalidates foreign diagnostics already in conversion', async () => {
  const host = createEditor(['/work/config', '/work/one', '/work/two']);
  const [folder, one, two] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), {
    'workspace.roots': [{ id: 'one', uri: one.uri.toString() }, { id: 'two', uri: two.uri.toString() }],
    'workspace.documents': [{ root: 'one', file: 'index.md', path: 'one/index.md' }, { root: 'two', file: 'index.md', path: 'two/index.md' }],
  });
  const session = new host.WorkspaceSession(folder, { appendLine() {} });
  await session.refresh();
  const client = session.client;
  let release;
  client.protocol2CodeConverter.asDiagnostics = async values => { await new Promise(resolve => { release = resolve; }); return values; };
  client.notifyDiagnostics({ uri: new Uri('/work/one/index.md').toString(), diagnostics: [] });
  await host.flush();
  host.vscode.workspace.workspaceFolders = [folder, one];
  release();
  await host.flush();
  assert.deepEqual(client.published, [], 'whole selected-root custody, not only the diagnostic source folder');
  await session.shutdown();
});
