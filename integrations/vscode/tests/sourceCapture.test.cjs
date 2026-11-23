const assert = require('node:assert/strict');
const { test } = require('node:test');
const { captureSources, TRANSFER_BYTES, SOURCE_COUNT } = require('../dist/sourceCapture.js');
const { createEditor, Uri } = require('./support/editor.cjs');
const local = path => ({ root: 'workspace', entry: path, path });

test('editor projects exact locators only, leaving BOM, CRLF, UTF8 and file capture to native ownership', async () => {
  const host = createEditor(['/work/config', '/work/content']);
  const [folder, owner] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), {
    'workspace.configuration': 'config/root.toml',
    'workspace.configurationDocuments': ['config/site.toml'],
    'workspace.roots': [{ id: 'content', uri: owner.uri.toString() }],
    'workspace.documents': [{ root: 'content', file: 'café%20[one]/index.md', path: 'articles/one/index.md' }],
    'workspace.manuals': [{ source: 'source:manual', root: 'content', file: 'manual.md', path: 'manual.md' }],
  });
  host.put('/work/content/café%20[one]/index.md', '\ufeff# café 🦀\r\n');
  host.vscode.workspace.fs.stat = () => assert.fail('editor must not capture native bytes');
  host.vscode.workspace.fs.readFile = () => assert.fail('editor must not capture native bytes');
  const result = await captureSources(host.vscode, folder);
  assert.deepEqual(result.initializationOptions, { native_selection: {
    allow_read: true,
    roots: [{ id: 'workspace', uri: folder.uri.toString() }, { id: 'content', uri: owner.uri.toString() }],
    workspace: local('config/root.toml'), workspace_documents: [local('config/site.toml')],
    documents: [{ root: 'content', entry: 'café%20[one]/index.md', path: 'articles/one/index.md' }],
    manuals: [{ source: 'source:manual', root: 'content', entry: 'manual.md', path: 'manual.md' }],
    page_inputs: [], package_baselines: [],
  } });
  assert.equal(result.owners.get(new Uri('/work/content/café%20[one]/index.md').toString()), owner.uri.toString());
  assert.deepEqual(result.rootUris, [folder.uri.toString(), owner.uri.toString()]);
  assert.deepEqual(host.reads, []);
});

test('opaque inputs select exact native capture without editor text, base64 or overlays', async () => {
  const host = createEditor(['/work/one']);
  const [folder] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), { 'workspace.documents': ['content/guide/index.md'], 'workspace.pagePackageInputs': ['content/guide/image.png'] });
  const result = await captureSources(host.vscode, folder);
  assert.deepEqual(result.initializationOptions.native_selection.page_inputs, [local('content/guide/image.png')]);
  assert.deepEqual(result.uris.map(uri => uri.toString()), [new Uri('/work/one/gazette.toml').toString(), new Uri('/work/one/content/guide/index.md').toString()]);
  assert.equal(result.owners.has(new Uri('/work/one/content/guide/image.png').toString()), true, 'opaque inputs also retain root custody');
  assert.equal(JSON.stringify(result.initializationOptions).includes('bytes_base64'), false);
});

test('packages retain typed role while physical locator and logical identity stay independent', async () => {
  const host = createEditor(['/work/one', '/work/theme']);
  const [folder, theme] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), {
    'workspace.roots': [{ id: 'theme', uri: theme.uri.toString() }],
    'workspace.packageBaselines': [{ kind: 'theme', package: 'theme:main', root: 'theme', file: 'theme.toml', path: 'packages/main.toml' }],
  });
  const result = await captureSources(host.vscode, folder);
  assert.deepEqual(result.initializationOptions.native_selection.package_baselines, [{ kind: 'theme', package: 'theme:main', root: 'theme', entry: 'theme.toml', path: 'packages/main.toml' }]);
  assert.equal('source' in result.initializationOptions.native_selection.package_baselines[0], false);
  assert.equal('manifest' in result.initializationOptions.native_selection.package_baselines[0], false);
});

test('duplicates, traversal and foreign nested-folder ownership are rejected without file IO', async () => {
  for (const documents of [['../outside.md'], ['same.md', 'same.md'], ['gazette.toml'], ['nested/article.md']]) {
    const host = createEditor(['/work/one', '/work/one/nested']);
    const [folder] = host.vscode.workspace.workspaceFolders;
    host.settings.set(folder.uri.toString(), { 'workspace.documents': documents });
    await assert.rejects(captureSources(host.vscode, folder));
    assert.deepEqual(host.reads, []);
  }
});

test('explicit roots must be opened and unique; no ambient external access or owner guessing', async () => {
  for (const roots of [
    [{ id: 'content', uri: 'file:///not-opened' }],
    [{ id: 'workspace', uri: 'file:///work/two' }],
    [{ id: 'a', uri: 'file:///work/two' }, { id: 'b', uri: 'file:///work/two' }],
    [{ id: 'content', uri: 'https://example.test/content' }],
  ]) {
    const host = createEditor();
    const [folder] = host.vscode.workspace.workspaceFolders;
    host.settings.set(folder.uri.toString(), { 'workspace.roots': roots });
    await assert.rejects(captureSources(host.vscode, folder));
    assert.deepEqual(host.reads, []);
  }
});

test('missing/non-UTF8 files are still complete locators; only native initialization may admit their bytes', async () => {
  const host = createEditor(['/work/one']);
  const [folder] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), { 'workspace.documents': ['missing.md', 'bad.md'] });
  host.put('/work/one/bad.md', Buffer.from([0xff]));
  const result = await captureSources(host.vscode, folder);
  assert.deepEqual(result.initializationOptions.native_selection.documents, [local('missing.md'), local('bad.md')]);
  assert.deepEqual(host.reads, [], 'real native refusal is qualified by framed LSP tests');
});

test('more than32 sources fit locator transport; count and serialized metadata remain finite', async () => {
  const host = createEditor(['/work/one']);
  const [folder] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), { 'workspace.documents': Array.from({ length: 200 }, (_, index) => `${index}.md`) });
  const result = await captureSources(host.vscode, folder);
  assert.equal(result.initializationOptions.native_selection.documents.length, 200);
  assert.ok(Buffer.byteLength(JSON.stringify(result.initializationOptions)) < TRANSFER_BYTES);
  host.settings.set(folder.uri.toString(), { 'workspace.documents': Array.from({ length: SOURCE_COUNT }, (_, index) => `${index}.md`) });
  await assert.rejects(captureSources(host.vscode, folder), /transfer count/);
  host.settings.set(folder.uri.toString(), { 'workspace.documents': ['x'.repeat(TRANSFER_BYTES) + '.md'] });
  await assert.rejects(captureSources(host.vscode, folder), /locator transfer size/);
  assert.deepEqual(host.reads, []);
});
