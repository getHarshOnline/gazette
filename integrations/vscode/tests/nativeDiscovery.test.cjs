const assert = require('node:assert/strict');
const {test} = require('node:test');
const {createEditor, Uri} = require('./support/editor.cjs');

test('configuration recovery invalidates cached native symbols without changing the article version', async () => {
  const host = createEditor(['/work/config'], false, true);
  const root = 'file:///work/config/gazette.toml', page = 'file:///work/content/page.md';
  const configuration = {uri: Uri.parse(root), version: 1, isClosed: false, languageId: 'gazette', text: 'valid', getText() { return this.text; }};
  const article = {uri: Uri.parse(page), version: 7, isClosed: false, getText: () => '# Retained heading'};
  host.vscode.workspace.textDocuments.push(configuration, article);
  let generation = 1, valid = true, rootVersion = 1;
  host.delivery = packet => {
    if (packet.params.textDocument?.uri === root && packet.method === 'textDocument/didChange') {
      rootVersion = packet.params.textDocument.version;
      valid = packet.params.contentChanges[0].text === 'valid'; generation++;
    }
  };
  host.SelectedLanguageClient.prototype.sendRequest = async function(method) {
    if (method === 'gazette/selectSource') return {selected: true, generation, watchFiles: [root]};
    return {generation, sources: [root, page].map(id => ({id, role: {kind: id === root ? 'workspace' : 'document'}, version: id === root ? rootVersion : 7}))};
  };
  await host.extension.activate({subscriptions: []});
  const client = host.clients[0];
  let cached = false, symbols;
  client.onSymbolsChanged = () => { cached = false; };
  const outline = async () => {
    if (!cached) {
      symbols = await client.clientOptions.middleware.provideDocumentSymbols(article, {isCancellationRequested: false},
        () => valid ? [{name: 'Retained heading'}] : null);
      cached = true;
    }
    return symbols;
  };
  configuration.version = 2; configuration.text = 'broken';
  await client.clientOptions.middleware.didChange({document: configuration});
  assert.equal(await outline(), null);
  configuration.version = 3; configuration.text = 'valid';
  await client.clientOptions.middleware.didChange({document: configuration});
  assert.deepEqual(await outline(), [{name: 'Retained heading'}]);
  assert.equal(article.version, 7);
  assert.equal(client.symbolRegistrations.length, 1, 'renewal replaces the native provider without duplicating it');
  assert.deepEqual(client.symbolRegistrations[0].registerOptions.documentSelector, [{scheme: 'file'}]);
  assert.equal(host.clients.length, 1);
  await host.extension.deactivate();
});

test('the first provider query waits for native admission and the current open text', async () => {
  const host = createEditor(['/work/config'], false, true);
  const root = 'file:///work/config/gazette.toml', page = 'file:///work/content/page.md';
  let release, nativeVersion = null, nativeText = '# Saved', calls = 0;
  host.SelectedLanguageClient.prototype.sendRequest = async function(method, params) {
    if (method === 'gazette/selectSource') {
      if (params.uri === page) await new Promise(resolve => { release = resolve; });
      return {selected: true, generation: 1, watchFiles: [root]};
    }
    return {sources: [root, page].map(id => ({id, role: {kind: id === root ? 'workspace' : 'document'}, version: id === page ? nativeVersion : null}))};
  };
  host.delivery = packet => {
    const value = packet.params.textDocument;
    if (value?.uri !== page) return;
    if (packet.method === 'textDocument/didOpen') { nativeVersion = value.version; nativeText = value.text; }
    else if (packet.method === 'textDocument/didChange' && nativeVersion !== null) {
      nativeVersion = value.version; nativeText = packet.params.contentChanges[0].text;
    }
  };
  await host.extension.activate({subscriptions: []});
  const client = host.clients[0];
  const document = {uri: Uri.parse(page), version: 1, isClosed: false, languageId: 'plaintext', text: '# Saved', getText() { return this.text; }};
  host.vscode.workspace.textDocuments.push(document);
  const opening = client.clientOptions.middleware.didOpen(document, value => client.sendNotification('textDocument/didOpen',
    {textDocument: {uri: page, version: value.version, languageId: value.languageId, text: value.getText()}}));
  await host.flush();
  document.version = 2; document.text = '# Unsaved';
  await client.clientOptions.middleware.didChange({document});
  const query = client.clientOptions.middleware.provideDocumentSymbols(document, {isCancellationRequested: false}, () => {
    calls++; return [{name: nativeText, version: nativeVersion}];
  });
  await host.flush();
  assert.equal(calls, 0, 'do not cache saved-baseline symbols for a still-admitting unsaved buffer');
  release();
  await opening;
  assert.deepEqual(await query, [{name: '# Unsaved', version: 2}]);
  await host.extension.deactivate();
});

test('replayed included configuration discovers and opens its already-unsaved page before synchronization finishes', async () => {
  const host = createEditor(['/work/config'], false, true);
  const root = 'file:///work/config/gazette.toml';
  const module = 'file:///work/config/included.toml';
  const previous = 'file:///work/content/a.md';
  const page = 'file:///work/content/b.md';
  const document = (id, version, text) => ({uri: Uri.parse(id), version, isClosed: false,
    languageId: 'markdown', getText: () => text});
  const configuration = document(root, 1, 'include unsaved module');
  host.vscode.workspace.textDocuments.push(configuration, document(module, 4, 'select B'), document(page, 8, '# Unsaved B'));
  let included = false, selectedB = false;
  const versions = new Map([[root, 1]]);
  host.delivery = packet => {
    const value = packet.params.textDocument;
    if (!value) return;
    versions.set(value.uri, value.version);
    if (value.uri === root) included = true;
    if (value.uri === module) selectedB = true;
    if (value.uri === page) host.clients[0].notifyDiagnostics({uri: page, version: 8, diagnostics: [{message: 'Current B'}]});
  };
  host.SelectedLanguageClient.prototype.sendRequest = async function(method, params) {
    const ids = [root, ...(included ? [module] : []), selectedB ? page : previous];
    if (method === 'gazette/selectSource') return {selected: ids.includes(params.uri), generation: 1, watchFiles: [root]};
    return {sources: ids.map(id => ({id, role: {kind: id === root ? 'workspace' : id === module ? 'workspace_document' : 'document'}, version: versions.get(id) ?? null}))};
  };
  await host.extension.activate({subscriptions: []});
  const client = host.clients[0];
  configuration.version = 2;
  await client.clientOptions.middleware.didChange({document: configuration});
  await host.flush();
  assert.ok(client.sent.some(packet => packet.method === 'textDocument/didOpen'
    && packet.params.textDocument.uri === page && packet.params.textDocument.version === 8
    && packet.params.textDocument.text === '# Unsaved B'));
  assert.ok(client.published.some(packet => packet.uri.toString() === page && packet.diagnostics[0]?.message === 'Current B'));
  assert.ok(host.watchers.some(watcher => !watcher.disposed && watcher.pattern.pattern === 'b.md'));
  assert.ok(host.watchers.filter(watcher => watcher.pattern.pattern === 'a.md').every(watcher => watcher.disposed));
  await host.extension.deactivate();
});

test('native navigation rejects a changed closed-file epoch even after its refresh settles', async () => {
  for (const kind of ['definition', 'references', 'hover']) for (const settle of [false, true]) {
    const host = createEditor(['/work/config'], false, true);
    const root = 'file:///work/config/gazette.toml';
    const page = 'file:///work/content/page.md';
    const target = 'file:///work/content/target.md';
    const document = {uri: Uri.parse(page), version: 1, isClosed: false, getText: () => '# Page'};
    host.vscode.workspace.textDocuments.push(document);
    host.SelectedLanguageClient.prototype.sendRequest = async function(method) {
      if (method === 'gazette/selectSource') return {selected: true, generation: 1, watchFiles: [root]};
      return {sources: [root, page, target].map(id => ({id, role: {kind: id === root ? 'workspace' : 'document'}, version: id === page ? 1 : null}))};
    };
    await host.extension.activate({subscriptions: []});
    const middleware = host.clients[0].clientOptions.middleware;
    const token = {isCancellationRequested: false};
    const query = forward => kind === 'references'
      ? middleware.provideReferences(document, {}, {includeDeclaration: true}, token, forward)
      : middleware[kind === 'hover' ? 'provideHover' : 'provideDefinition'](document, {}, token, forward);
    let release;
    const pending = query(() => new Promise(resolve => { release = resolve; }));
    await host.flush();
    host.fileEvent('/work/content/target.md');
    if (settle) { await new Promise(resolve => setTimeout(resolve, 80)); await host.flush(); }
    const result = {native: 'old target'};
    release(result);
    assert.equal(await pending, null, `${kind}, refresh settled: ${settle}`);
    if (settle) assert.equal(await query(() => result), result, 'fresh queries remain available');
    await host.extension.deactivate();
  }
});

test('automatic discovery stays inactive in unrelated folders before looking for a server program', async () => {
  const host = createEditor(['/work/unrelated'], false, true);
  host.files.clear();
  host.settings.set('file:///work/unrelated', {executable: {}});
  await host.extension.activate({subscriptions: []});
  assert.equal(host.clients.length, 0);
  assert.equal(host.messages.length, 0);
  assert.equal(host.reads.length, 0);
  assert.equal(host.watchers.length, 1, 'creation of the exact configuration can activate the folder');
  await host.extension.deactivate();
});

test('native selection admits a new external page before forwarding text and retains the client through configuration edits', async () => {
  const host = createEditor(['/work/config'], false, true);
  const root = 'file:///work/config/gazette.toml';
  const page = 'file:///work/content/new.md';
  const selected = new Set([root]);
  const versions = new Map();
  const queries = [];
  host.SelectedLanguageClient.prototype.sendRequest = async function(method, params) {
    queries.push({method, params});
    if (method === 'gazette/selectSource') {
      if (params.uri === page) selected.add(page);
      return {selected: selected.has(params.uri), generation: 1, watchFiles: [root]};
    }
    assert.equal(method, 'gazette/editorSnapshot');
    return {sources: [...selected].map(id => ({id, role: {kind: id === root ? 'workspace' : 'document'}, version: versions.get(id) ?? null}))};
  };
  await host.extension.activate({subscriptions: []});
  const client = host.clients[0];
  assert.equal(client.clientOptions.initializationOptions.native_selection.discover, true);
  assert.deepEqual(client.clientOptions.documentSelector, [{scheme: 'file'}]);
  const document = {uri: Uri.parse(page), version: 1, languageId: 'markdown', isClosed: false, getText: () => '# New'};
  host.vscode.workspace.textDocuments.push(document);
  const forwarded = [];
  await client.clientOptions.middleware.didOpen(document, async value => {
    forwarded.push(value); versions.set(value.uri.toString(), value.version);
  });
  assert.deepEqual(forwarded, [document]);
  assert.equal(queries.some(query => query.method === 'gazette/selectSource' && query.params.uri === page), true);
  await client.clientOptions.middleware.didOpen({...document, uri: Uri.parse('file:///work/other/private.md')}, async () => assert.fail('unselected text must not be forwarded'));
  const configuration = {...document, uri: Uri.parse(root), version: 2, getText: () => '[[broken'};
  await client.clientOptions.middleware.didChange({document: configuration});
  assert.equal(host.clients.length, 1);
  assert.equal(client.stopped, false);
  assert.equal(client.sent.at(-1).method, 'textDocument/didChange');
  assert.equal(host.reads.length, 0);
  document.isClosed = true;
  host.fileEvent('/work/content/new.md');
  await new Promise(resolve => setTimeout(resolve, 80));
  await host.flush();
  assert.equal(host.clients.length, 1, 'native file refresh retains the running client');
  assert.equal(client.sent.at(-1).method, 'workspace/didChangeWatchedFiles');
  await host.extension.deactivate();
});

test('discovered diagnostics survive membership synchronization and leave with deselected files', async () => {
  const host = createEditor(['/work/config'], false, true);
  const root = 'file:///work/config/gazette.toml';
  const page = 'file:///work/content/problem.md';
  let include = true;
  host.SelectedLanguageClient.prototype.sendRequest = async function(method) {
    if (method === 'gazette/selectSource') return {selected: true, generation: 1, watchFiles: [root]};
    assert.equal(method, 'gazette/editorSnapshot');
    if (include) this.notifyDiagnostics({uri: page, diagnostics: [{message: 'Selected page problem'}]});
    return {sources: [root, ...(include ? [page] : [])].map(id => ({id, role: {kind: id === root ? 'workspace' : 'document'}, version: null}))};
  };
  await host.extension.activate({subscriptions: []});
  await host.flush();
  const client = host.clients[0];
  assert.ok(client.published.some(value => value.uri.toString() === page && value.diagnostics[0]?.message === 'Selected page problem'));
  include = false;
  host.fileEvent('/work/config/gazette.toml');
  await new Promise(resolve => setTimeout(resolve, 80));
  await host.flush();
  assert.ok(client.cleared.includes(page), 'deselection clears an earlier diagnostic even if its conversion was in flight');
  await host.extension.deactivate();
});

test('pending discovery diagnostics cannot cross a close and reopen with the same version', async () => {
  const host = createEditor(['/work/config'], false, true);
  const root = 'file:///work/config/gazette.toml';
  const page = 'file:///work/content/problem.md';
  const original = {uri: Uri.parse(page), version: 4, isClosed: false};
  host.vscode.workspace.textDocuments.push(original);
  host.SelectedLanguageClient.prototype.sendRequest = async function(method) {
    if (method === 'gazette/selectSource') return {selected: true, generation: 1, watchFiles: [root]};
    this.notifyDiagnostics({uri: page, version: 4, diagnostics: [{message: 'Earlier buffer'}]});
    original.isClosed = true;
    host.vscode.workspace.textDocuments.push({...original, isClosed: false});
    return {sources: [root, page].map(id => ({id, role: {kind: id === root ? 'workspace' : 'document'}, version: id === page ? 4 : null}))};
  };
  await host.extension.activate({subscriptions: []});
  await host.flush();
  const client = host.clients[0];
  assert.deepEqual(client.published, []);
  client.notifyDiagnostics({uri: page, version: 4, diagnostics: [{message: 'Current buffer'}]});
  await host.flush();
  assert.equal(client.published[0].diagnostics[0].message, 'Current buffer');
  await host.extension.deactivate();
});
