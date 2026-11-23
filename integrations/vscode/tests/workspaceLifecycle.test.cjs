const assert = require('node:assert/strict');
const { test } = require('node:test');
const { createEditor, Uri } = require('./support/editor.cjs');
const { once } = require('node:events');
const { createHash } = require('node:crypto');

async function filesystemSettled(host) {
  await new Promise(resolve => setTimeout(resolve, 75));
  await host.flush();
}

test('a discovered workspace refuses an older snapshot without copying saved sources', async () => {
  const host = createEditor(['/work/one'], false, true);
  const folder = host.vscode.workspace.workspaceFolders[0];
  const document = {uri: new Uri('/work/one/gazette.toml'), version: 1, isClosed: false,
    languageId: 'gazette', getText: () => ''};
  host.vscode.workspace.textDocuments.push(document);
  const digest = createHash('sha256').update('').digest('hex');
  let chunks = 0;
  host.SelectedLanguageClient.prototype.sendRequest = async function(method) {
    if (method === 'gazette/selectSource') return {selected: true, generation: 1, watchFiles: [document.uri.toString()]};
    if (method === 'gazette/editorSource') chunks++;
    return {generation: 1, structurally_valid: true, sources: [{id: document.uri.toString(), path: 'gazette.toml',
      role: {kind: 'workspace'}, version: 1, bytes: 0, source_sha256: digest}]};
  };
  const session = new host.WorkspaceSession(folder, {appendLine() {}}, new Map(), () => {});
  try {
    await session.refresh();
    await assert.rejects(session.snapshot(document), /unsaved-only editor handoff/);
    assert.equal(chunks, 0);
  } finally {await session.shutdown();}
});

test('selected-source refresh invalidates the view and export waits for its new client', async () => {
  const host = createEditor(['/work/one']);
  const folder = host.vscode.workspace.workspaceFolders[0];
  host.settings.set(folder.uri.toString(), {'workspace.documents': ['guide.md']});
  let changes = 0;
  const session = new host.WorkspaceSession(folder, {appendLine() {}}, new Map(), () => {changes++;});
  await session.refresh();
  const before = changes;
  const document = {uri: new Uri('/work/one/guide.md'), version: 1, isClosed: false, getText: () => 'Guide'};
  host.vscode.workspace.textDocuments.push(document);
  let release;
  host.startGate = new Promise(resolve => {release = resolve;});
  host.fileEvent('/work/one/gazette.toml');
  assert.ok(changes > before, 'an exact disk change invalidates old ready feedback');
  let finished = false;
  const context = session.snapshot(document).then(value => {finished = true; return value;});
  await host.flush();
  assert.equal(finished, false, 'a pending selected recapture is awaited');
  const next = host.clients.at(-1);
  const sources = [['gazette.toml', 'workspace'], ['guide.md', 'document']].map(([file, kind]) => ({
    id: new Uri('/work/one/' + file).toString(), path: file, role: {kind}, bytes: 0, version: null,
    source_sha256: createHash('sha256').update('').digest('hex'),
  }));
  next.sendRequest = async method => {assert.equal(method, 'gazette/editorSnapshot'); return {generation: 8, structurally_valid: true, sources};};
  release();
  assert.equal((await context).manifest.generation, 8);
  assert.ok(changes > before + 1, 'settled analysis schedules a fresh view');
  await session.shutdown();
});

test('exact selected closed-file events coalesce and refresh only their distributed owner', async () => {
  const host = createEditor(['/work/config', '/work/source', '/work/other']);
  const [owner, source, other] = host.vscode.workspace.workspaceFolders;
  host.settings.set(source.uri.toString(), { 'authoring.enabled': false });
  host.settings.set(owner.uri.toString(), {
    'workspace.roots': [{id: 'source', uri: source.uri.toString()}],
    'workspace.configurationDocuments': ['config/site.toml'],
    'workspace.manuals': [{source: 'source:manual', root: 'source', file: 'docs/[manual].md', path: 'docs/manual.md'}],
    'workspace.pagePackageInputs': [{root: 'source', file: 'image.png', path: 'image.png'}],
  });
  await host.extension.activate({subscriptions: []});
  const first = host.clients.find(client => client.clientOptions.workspaceFolder === owner);
  const untouched = host.clients.find(client => client.clientOptions.workspaceFolder === other);
  const patterns = host.watchers.filter(watcher => !watcher.disposed).map(watcher => [watcher.pattern.baseUri.path, watcher.pattern.pattern]);
  assert.ok(patterns.some(([base, pattern]) => base === '/work/source/docs' && pattern === '[[]manual[]].md'));
  assert.ok(patterns.every(([, pattern]) => !pattern.includes('**') && !pattern.includes('/')));
  host.fileEvent('/work/source/docs/unselected.md');
  await filesystemSettled(host);
  assert.equal(host.clients.length, 2, 'unselected neighbors do not trigger source discovery');
  host.fileEvent('/work/source/docs/[manual].md', 'change');
  host.fileEvent('/work/config/config/site.toml', 'delete');
  host.vscode.workspace.textDocuments.push({uri: new Uri('/work/source/image.png'), isClosed: false, languageId: 'plaintext'});
  host.fileEvent('/work/source/image.png', 'create');
  await filesystemSettled(host);
  assert.equal(host.clients.length, 3, 'one coalesced replacement for the owning composition');
  assert.equal(first.stopped, true);
  assert.equal(untouched.stopped, false);
  assert.deepEqual(host.reads, [], 'recapture remains native, not a JavaScript file reader');
  await host.extension.deactivate();
  assert.ok(host.watchers.every(watcher => watcher.disposed));
  assert.equal(host.close.listeners.size, 0);
});

test('open selected buffers retain their text and version until an observed disk change is closed', async () => {
  const host = createEditor(['/work/one']);
  const folder = host.vscode.workspace.workspaceFolders[0];
  host.settings.set(folder.uri.toString(), {'workspace.documents': ['article.md']});
  const document = {uri: new Uri('/work/one/article.md'), version: 9, isClosed: false, languageId: 'markdown', getText: () => '# Unsaved'};
  host.vscode.workspace.textDocuments.push(document);
  await host.extension.activate({subscriptions: []});
  const first = host.clients[0];
  host.fileEvent(document.uri.path);
  await filesystemSettled(host);
  assert.equal(host.clients.length, 1);
  assert.equal(first.stopped, false);
  assert.equal(document.getText(), '# Unsaved');
  assert.equal(document.version, 9);
  document.isClosed = true;
  host.close.fire(document);
  await filesystemSettled(host);
  assert.equal(host.clients.length, 2, 'the now-closed changed baseline is recaptured');
  await host.extension.deactivate();
});

test('changing the explicit selection retires old file watches instead of watching former sources', async () => {
  const host = createEditor(['/work/one']);
  const folder = host.vscode.workspace.workspaceFolders[0];
  host.settings.set(folder.uri.toString(), {'workspace.documents': ['old.md']});
  const session = new host.WorkspaceSession(folder, {appendLine() {}});
  await session.refresh();
  assert.equal(session.selects(new Uri('/work/one/old.md')), true);
  assert.equal(session.selects(new Uri('/work/one/unselected.py')), false);
  host.settings.set(folder.uri.toString(), {'workspace.documents': ['new.md']});
  await session.refresh();
  host.fileEvent('/work/one/old.md');
  await filesystemSettled(host);
  assert.equal(host.clients.length, 2);
  assert.ok(host.watchers.filter(watcher => watcher.pattern.pattern === 'old.md').every(watcher => watcher.disposed));
  host.fileEvent('/work/one/new.md');
  await filesystemSettled(host);
  assert.equal(host.clients.length, 3);
  await session.shutdown();
});

test('deletion refusal retains exact watches so selected-file recreation recovers without an unrelated retry', async () => {
  const host = createEditor(['/work/one']);
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], {appendLine() {}});
  await session.refresh();
  host.failStart.add('/work/one');
  host.fileEvent('/work/one/gazette.toml', 'delete');
  await filesystemSettled(host);
  assert.equal(host.clients.length, 2);
  assert.equal(host.clients[1].stopped, true);
  assert.ok(host.watchers.some(watcher => !watcher.disposed));
  host.failStart.clear();
  host.fileEvent('/work/one/gazette.toml', 'create');
  await filesystemSettled(host);
  assert.equal(host.clients.length, 3);
  assert.equal(host.clients[2].stopped, false);
  await session.shutdown();
});

test('a filesystem change during initialization requests one replacement and shutdown cancels a pending burst', async () => {
  const host = createEditor(['/work/one']);
  host.settings.set(host.vscode.workspace.workspaceFolders[0].uri.toString(), {'workspace.configurationDocuments': ['module.toml']});
  let release;
  host.startGate = new Promise(resolve => { release = resolve; });
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], {appendLine() {}});
  const starting = session.refresh();
  await host.flush();
  const pendingDocument = {uri: new Uri('/work/one/gazette.toml'), version: 1, isClosed: false, getText: () => 'current'};
  host.vscode.workspace.textDocuments.push(pendingDocument);
  await assert.rejects(session.snapshot(pendingDocument), /active Gazette composition/,
    'initializing clients cannot export a snapshot before protocol admission');
  for (let i = 0; i < 12; i++) host.fileEvent('/work/one/module.toml');
  release();
  await starting;
  assert.equal(host.clients.length, 2);
  host.fileEvent('/work/one/module.toml');
  await session.shutdown();
  await filesystemSettled(host);
  assert.equal(host.clients.length, 2, 'no delayed replacement survives shutdown');
  assert.ok(host.watchers.every(watcher => watcher.disposed));
});

test('snapshot freshness is bound to its owning client even when replacement generations repeat', async () => {
  const host = createEditor(['/work/one']);
  host.settings.set(host.vscode.workspace.workspaceFolders[0].uri.toString(), {'workspace.configurationDocuments': ['module.toml']});
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], {appendLine() {}});
  const document = {uri: new Uri('/work/one/gazette.toml'), version: 1, isClosed: false, languageId: 'gazette', getText: () => 'unsaved'};
  host.vscode.workspace.textDocuments.push(document);
  await session.refresh();
  const manifest = {generation: 3, structurally_valid: true, modular: false, sources: [{
    id: document.uri.toString(), path: 'gazette.toml', role: {kind: 'workspace'}, version: 1,
    bytes: 7, source_sha256: createHash('sha256').update('unsaved').digest('hex'),
  }]};
  const respond = async (method, params) => method === 'gazette/editorSnapshot' ? manifest : {
    generation: 3, document: params.document, offset: params.offset, bytes_base64: Buffer.from('unsaved').toString('base64'),
  };
  host.clients[0].sendRequest = respond;
  const snapshot = await session.snapshot(document);
  assert.equal(await snapshot.current(), true);
  host.fileEvent('/work/one/module.toml');
  assert.equal(await snapshot.current(), false, 'pending retirement invalidates immediately');
  await filesystemSettled(host);
  host.clients[1].sendRequest = respond;
  const current = await session.snapshot(document);
  assert.equal(current.generation, snapshot.generation);
  assert.equal(await snapshot.current(), false, 'same generation on a different client never revives old capture');
  assert.equal(await current.current(), true);
  const stopping = session.stop();
  assert.equal(await current.current(), false, 'retired client cannot claim freshness while stopping');
  await stopping;
  await session.shutdown();
});

test('an in-flight capture is discarded when a closed selected source changes', async () => {
  const host = createEditor(['/work/one']);
  host.settings.set(host.vscode.workspace.workspaceFolders[0].uri.toString(), {'workspace.configurationDocuments': ['module.toml']});
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], {appendLine() {}});
  const document = {uri: new Uri('/work/one/gazette.toml'), version: 1, isClosed: false, languageId: 'gazette', getText: () => 'unsaved'};
  host.vscode.workspace.textDocuments.push(document);
  await session.refresh();
  let release;
  const pending = new Promise(resolve => {release = resolve;});
  host.clients[0].sendRequest = async (method, params) => {
    if (method === 'gazette/editorSnapshot') {
      await pending;
      return {generation: 3, structurally_valid: true, modular: false, sources: [{
        id: document.uri.toString(), path: 'gazette.toml', role: {kind: 'workspace'}, version: 1,
        bytes: 7, source_sha256: createHash('sha256').update('unsaved').digest('hex'),
      }]};
    }
    return {generation: 3, document: params.document, offset: params.offset, bytes_base64: Buffer.from('unsaved').toString('base64')};
  };
  const capture = session.snapshot(document);
  await host.flush();
  host.fileEvent('/work/one/module.toml');
  release();
  await assert.rejects(capture, /changed during capture/);
  await session.shutdown();
});

test('source navigation refuses pending refreshes and discards a retired manifest response', async () => {
  const host = createEditor(['/work/one']);
  const folder = host.vscode.workspace.workspaceFolders[0];
  const session = new host.WorkspaceSession(folder, {appendLine() {}});
  await session.refresh();
  let release;
  const pending = new Promise(resolve => {release = resolve;});
  const uri = new Uri('/work/one/gazette.toml');
  let oldRequests = 0;
  host.clients[0].sendRequest = async () => {
    oldRequests++;
    await pending;
    return {sources: [{id: uri.toString(), path: 'gazette.toml'}]};
  };
  const navigation = session.sourceUri('gazette.toml');
  await host.flush();
  host.fileEvent('/work/one/gazette.toml');
  await assert.rejects(session.sourceUri('gazette.toml'), /no longer active/);
  assert.equal(oldRequests, 1, 'pending refresh never queries the retired client');
  release();
  await assert.rejects(navigation, /changed before navigation/);
  await filesystemSettled(host);
  host.clients[1].sendRequest = async () => ({sources: [{id: uri.toString(), path: 'gazette.toml'}]});
  assert.equal((await session.sourceUri('gazette.toml')).toString(), uri.toString());
  await session.shutdown();
});

test('source-only folders stay inactive and disabling an owner releases its session custody', async () => {
  const host = createEditor();
  const [owner, source] = host.vscode.workspace.workspaceFolders;
  host.settings.set(source.uri.toString(), { 'authoring.enabled': false });
  await host.extension.activate({ subscriptions: [] });
  assert.equal(host.clients.length, 1);
  assert.equal(host.clients[0].clientOptions.workspaceFolder, owner);
  host.settings.set(owner.uri.toString(), { 'authoring.enabled': false });
  host.configuration.fire({ affectsConfiguration: (name, uri) => name === 'gazette.authoring.enabled'
    && uri.toString() === owner.uri.toString() });
  await host.flush();
  assert.equal(host.clients[0].stopped, true);
  host.settings.set(owner.uri.toString(), { 'authoring.enabled': true });
  host.configuration.fire({ affectsConfiguration: (name, uri) => name === 'gazette.authoring.enabled'
    && uri.toString() === owner.uri.toString() });
  await host.flush();
  assert.equal(host.clients.length, 2);
  await host.extension.deactivate();
});

test('real failed-start child receives owned stdin EOF and closes without a forced kill', async () => {
  const host = createEditor(['/work/one'], 'native');
  host.failStart.add('/work/one');
  const client = new host.SelectedLanguageClient('native', 'native', {
    command: process.execPath, args: ['-e', "process.stdin.resume(); process.stdin.on('end', () => process.exit(0));"],
  }, { workspaceFolder: host.vscode.workspace.workspaceFolders[0], outputChannel: { appendLine() {} } }, () => true, host.vscode.workspace.workspaceFolders);
  await assert.rejects(client.start(), /private startup payload/);
  const child = client.process;
  child.kill = () => { throw new Error('graceful EOF must not force termination'); };
  await client.stop(1000);
  assert.equal(child.stdin.writableEnded, true);
  assert.equal(child.exitCode, 0);
  assert.equal(client.diagnosticsDisposed, true);
});

test('real failed-start child that remains alive after EOF retains unproven cleanup', async () => {
  const host = createEditor(['/work/one'], 'native');
  host.failStart.add('/work/one');
  const client = new host.SelectedLanguageClient('live', 'live', {
    command: process.execPath, args: ['-e', 'process.stdin.resume(); setInterval(() => {}, 1000);'],
  }, { workspaceFolder: host.vscode.workspace.workspaceFolders[0], outputChannel: { appendLine() {} } }, () => true, host.vscode.workspace.workspaceFolders);
  await assert.rejects(client.start(), /private startup payload/);
  const child = client.process;
  try {
    await assert.rejects(client.stop(30), /cleanup remains unproven/);
    assert.equal(child.stdin.writableEnded, true);
    assert.equal(child.exitCode, null);
    assert.notEqual(client.diagnosticsDisposed, true);
  } finally {
    // Explicitly clean up this exact test-owned stubborn child, never a PID scan.
    const completed = once(child, 'close');
    child.kill('SIGTERM');
    await completed;
    await client.stop(1000);
  }
});

test('failed native initialization releases custody only after exact child close and can restart', async () => {
  const host = createEditor(['/work/one'], true);
  host.failStart.add('/work/one');
  const custody = new Map();
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], { appendLine() {} }, custody);
  await session.refresh();
  assert.equal(host.children.length, 1);
  assert.deepEqual(host.spawns[0], { command: '/exact/gazette', args: ['lsp'],
    options: { cwd: '/work/one', shell: false, detached: false, stdio: ['pipe', 'pipe', 'pipe'] } });
  assert.equal(host.clients[0].diagnosticsDisposed, true);
  assert.equal(custody.size, 0, 'observed child/stdio close discharges failed-start custody');
  host.failStart.clear();
  await session.refresh();
  assert.equal(host.children.length, 2);
  assert.equal(custody.size, 1);
  await session.shutdown();
  assert.equal(custody.size, 0);
});

test('exit alone and provider non-running state do not discharge a live failed-start child', async () => {
  const host = createEditor(['/work/one'], true);
  host.failStart.add('/work/one'); host.closeFailedStart = false;
  const custody = new Map();
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], { appendLine() {} }, custody);
  const starting = session.refresh();
  await host.flush();
  const client = host.clients[0];
  host.children[0].emit('exit', 0, null);
  await assert.rejects(client.stop(5), /cleanup remains unproven/);
  assert.equal(host.children[0].stdin.writableEnded, true, 'failed initialize closes only its owned input');
  assert.equal(custody.size, 1);
  assert.equal(host.children.length, 1, 'no automatic retry or replacement child');
  host.children[0].emit('close', 0, null);
  await starting;
  assert.equal(custody.size, 0);
  await session.shutdown();
});

test('a successful-start shutdown failure keeps custody even when the child later closes', async () => {
  const host = createEditor(['/work/one'], true);
  const custody = new Map();
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], { appendLine() {} }, custody);
  await session.refresh();
  host.failStop.add('/work/one');
  host.children[0].emit('close', 0, null);
  await assert.rejects(session.refresh(), /private uncertain process/);
  assert.equal(host.children[0].stdin.writableEnded, false, 'an initialized shutdown failure is not converted to EOF teardown');
  assert.equal(custody.size, 1);
  await assert.rejects(session.refresh(), /cleanup remains unproven/);
  assert.equal(host.children.length, 1);
  await assert.rejects(session.shutdown(), /private uncertain process/);
});

test('explicit retry recovers a failed stop only after public stop and exact child closure succeed', async () => {
  const host = createEditor(['/work/one'], true);
  const custody = new Map();
  const session = new host.WorkspaceSession(host.vscode.workspace.workspaceFolders[0], {appendLine() {}}, custody);
  await session.refresh();
  const client = host.clients[0];
  const stop = client.stop.bind(client);
  client.stop = () => stop(5);
  host.failStop.add('/work/one');
  host.closeRunningStop = false;
  await assert.rejects(session.refresh(), /cleanup remains unproven/);
  await assert.rejects(session.refresh(), /cleanup remains unproven/);
  assert.equal(custody.size, 1);
  host.failStop.clear();
  await assert.rejects(session.retry(), /closure remains unproven/);
  host.children[0].emit('exit', 0, null);
  await assert.rejects(session.retry(), /closure remains unproven/);
  assert.equal(custody.size, 1, 'exit without closed inherited streams does not discharge custody');
  assert.equal(host.children.length, 1, 'failed retries never spawn a second process');
  host.children[0].emit('close', 0, null);
  await session.retry();
  assert.equal(custody.size, 0);
  host.closeRunningStop = true;
  await session.refresh();
  assert.equal(host.children.length, 2);
  await session.shutdown();
});

test('folder sessions capture separate sources and restart/remove only affected folders', async () => {
  const host = createEditor();
  const [one, two] = host.vscode.workspace.workspaceFolders;
  host.settings.set(one.uri.toString(), { 'workspace.documents': ['article.md'] });
  host.put('/work/one/article.md', '# One');
  await host.extension.activate({ subscriptions: [] });
  assert.equal(host.clients.length, 2);
  const first = host.clients.find(client => client.clientOptions.workspaceFolder === one);
  const second = host.clients.find(client => client.clientOptions.workspaceFolder === two);
  assert.notEqual(first.id, second.id);
  assert.equal(first.clientOptions.workspaceFolder, one);
  assert.equal(second.clientOptions.workspaceFolder, two);
  assert.equal(typeof first.serverOptions, 'function', 'public factory owns the exact child lifetime');
  assert.equal(first.clientOptions.initializationOptions.native_selection.documents[0].entry, 'article.md');
  assert.deepEqual(second.clientOptions.initializationOptions.native_selection.documents, []);
  assert.deepEqual(host.reads, [], 'byte capture belongs to native LSP');

  host.settings.set(two.uri.toString(), { 'workspace.configuration': 'other.toml' });
  host.put('/work/two/other.toml', 'replacement baseline');
  host.configuration.fire({ affectsConfiguration: (name, uri) => name === 'gazette.workspace.configuration' && uri.toString() === two.uri.toString() });
  await host.flush();
  assert.equal(host.clients.length, 3);
  assert.equal(first.stopped, false);
  assert.equal(second.stopped, true);
  assert.equal(host.clients[2].clientOptions.initializationOptions.native_selection.workspace.path, 'other.toml');
  host.vscode.workspace.workspaceFolders = [two];
  host.folders.fire({ removed: [one], added: [] });
  await host.flush();
  assert.equal(first.stopped, true);
  assert.equal(host.clients.length, 3);
  await host.extension.deactivate();
  assert.equal(host.clients[2].stopped, true);
  assert.equal(host.configuration.listeners.size, 0);
  assert.equal(host.commands.size, 0);
});

test('changing modular configuration documents recaptures and restarts only the affected folder', async () => {
  const host = createEditor();
  const [one, two] = host.vscode.workspace.workspaceFolders;
  await host.extension.activate({ subscriptions: [] });
  const first = host.clients.find(client => client.clientOptions.workspaceFolder === one);
  const second = host.clients.find(client => client.clientOptions.workspaceFolder === two);
  host.settings.set(two.uri.toString(), { 'workspace.configurationDocuments': ['config/site.toml'] });
  host.put('/work/two/config/site.toml', "[[sites]]\nid='site:two'\npublisher='owner:two'\ndeployable=false\n");
  host.configuration.fire({
    affectsConfiguration: (name, uri) => name === 'gazette.workspace.configurationDocuments'
      && uri.toString() === two.uri.toString(),
  });
  await host.flush();
  assert.equal(host.clients.length, 3);
  assert.equal(first.stopped, false);
  assert.equal(second.stopped, true);
  assert.deepEqual(host.clients[2].clientOptions.initializationOptions.native_selection.workspace_documents, [{
    root: 'workspace', entry: 'config/site.toml',
    path: 'config/site.toml',
  }]);
  assert.deepEqual(host.reads, []);
  await host.extension.deactivate();
});

test('changing typed package baselines recaptures and restarts only the affected folder', async () => {
  const host = createEditor();
  const [one, two] = host.vscode.workspace.workspaceFolders;
  await host.extension.activate({ subscriptions: [] });
  const first = host.clients.find(client => client.clientOptions.workspaceFolder === one);
  const second = host.clients.find(client => client.clientOptions.workspaceFolder === two);
  host.settings.set(two.uri.toString(), { 'workspace.packageBaselines': [
    { kind: 'theme', package: 'theme:two', file: 'theme/theme.toml' },
  ] });
  host.put('/work/two/theme/theme.toml', "package='theme:two'\n");
  host.configuration.fire({
    affectsConfiguration: (name, uri) => name === 'gazette.workspace.packageBaselines'
      && uri.toString() === two.uri.toString(),
  });
  await host.flush();
  assert.equal(host.clients.length, 3);
  assert.equal(first.stopped, false);
  assert.equal(second.stopped, true);
  assert.deepEqual(host.clients[2].clientOptions.initializationOptions.native_selection.package_baselines, [{
    kind: 'theme',
    package: 'theme:two',
    root: 'workspace', entry: 'theme/theme.toml',
    path: 'theme/theme.toml',
  }]);
  assert.deepEqual(host.reads, []);
  await host.extension.deactivate();
});

test('changing authored manuals recaptures and restarts only the affected folder', async () => {
  const host = createEditor();
  const [one, two] = host.vscode.workspace.workspaceFolders;
  await host.extension.activate({ subscriptions: [] });
  const first = host.clients.find(client => client.clientOptions.workspaceFolder === one);
  const second = host.clients.find(client => client.clientOptions.workspaceFolder === two);
  host.settings.set(two.uri.toString(), { 'workspace.manuals': [{ source: 'source:manual', file: 'docs/manual.md' }] });
  host.put('/work/two/docs/manual.md', '# Manual\n');
  host.configuration.fire({
    affectsConfiguration: (name, uri) => name === 'gazette.workspace.manuals'
      && uri.toString() === two.uri.toString(),
  });
  await host.flush();
  assert.equal(host.clients.length, 3);
  assert.equal(first.stopped, false);
  assert.equal(second.stopped, true);
  assert.deepEqual(host.clients[2].clientOptions.initializationOptions.native_selection.manuals, [{
    source: 'source:manual', root: 'workspace', entry: 'docs/manual.md',
    path: 'docs/manual.md',
  }]);
  await host.extension.deactivate();
});

test('changing page-package inputs recaptures opaque bytes without selecting them as editor documents', async () => {
  const host = createEditor();
  const [one, two] = host.vscode.workspace.workspaceFolders;
  await host.extension.activate({ subscriptions: [] });
  const first = host.clients.find(client => client.clientOptions.workspaceFolder === one);
  const second = host.clients.find(client => client.clientOptions.workspaceFolder === two);
  host.settings.set(two.uri.toString(), { 'workspace.pagePackageInputs': ['content/guide/image.png'] });
  host.put('/work/two/content/guide/image.png', Buffer.from([0, 255, 1]));
  host.configuration.fire({
    affectsConfiguration: (name, uri) => name === 'gazette.workspace.pagePackageInputs'
      && uri.toString() === two.uri.toString(),
  });
  await host.flush();
  assert.equal(host.clients.length, 3);
  assert.equal(first.stopped, false);
  assert.equal(second.stopped, true);
  assert.deepEqual(host.clients[2].clientOptions.initializationOptions.native_selection.page_inputs, [{
    root: 'workspace', entry: 'content/guide/image.png',
    path: 'content/guide/image.png',
  }]);
  assert.equal(
    host.clients[2].clientOptions.documentSelector.some(selector => selector.pattern.endsWith('image.png')),
    false,
  );
  await host.extension.deactivate();
});

test('only exact selected editor identities receive overlays, symbols and diagnostics', async () => {
  const host = createEditor(['/work/one']);
  const [folder] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), { 'workspace.documents': ['[draft].md'] });
  host.put('/work/one/[draft].md', '# Disk baseline');
  await host.extension.activate({ subscriptions: [] });
  const client = host.clients[0];
  const middleware = client.clientOptions.middleware;
  const selected = { uri: new Uri('/work/one/[draft].md'), version: 7, getText: () => '# Unsaved', languageId: 'markdown', isClosed: false };
  host.vscode.workspace.textDocuments.push(selected);
  const other = { ...selected, uri: new Uri('/work/one/draft.md') };
  const calls = [];
  const forward = async value => calls.push(value);
  await middleware.didOpen(other, forward);
  await middleware.didChange({ document: other }, forward);
  await middleware.didClose(other, forward);
  assert.equal(await middleware.provideDocumentSymbols(other, {}, forward), null);
  middleware.handleDiagnostics(other.uri, [{ message: 'foreign' }], () => calls.push('foreign'));
  assert.deepEqual(calls, []);
  await middleware.didOpen(selected, forward);
  await middleware.didChange({ document: selected }, forward);
  await middleware.didClose(selected, forward);
  await middleware.provideDocumentSymbols(selected, {}, forward);
  middleware.handleDiagnostics(selected.uri, [], () => calls.push('selected diagnostics'));
  assert.equal(calls[0].getText(), '# Unsaved');
  assert.deepEqual(client.sent, [{ method: 'textDocument/didChange', params: {
    textDocument: { uri: selected.uri.toString(), version: 7 }, contentChanges: [{ text: '# Unsaved' }],
  } }], 'selected full changes use native immediate typed delivery, not provider delayed forward');
  assert.equal(calls.length, 4);
  assert.equal(host.reads.length, 0, 'native capture owns baselines; overlays do not reread editor files');
  assert.equal(client.clientOptions.initializationOptions.native_selection.documents[0].entry, '[draft].md');
  assert.equal(host.files.get(selected.uri.toString()).toString(), '# Disk baseline');
  assert.deepEqual(client.clientOptions.errorHandler.closed(), { action: 2 });
  await host.extension.deactivate();
});

test('native initialization refusal and rejected protocol clean up only their own session', async () => {
  const host = createEditor();
  host.failStart.add('/work/one');
  await host.extension.activate({ subscriptions: [] });
  assert.equal(host.clients.length, 2);
  assert.equal(host.clients[0].stopped, true);
  assert.equal(host.clients[1].clientOptions.workspaceFolder.uri.path, '/work/two');
  assert.ok(host.logs.length);
  assert.ok(host.logs.some(line => line.includes('private startup payload')), 'the actual startup cause remains available in local output');
  assert.equal(host.messages.filter(value => value.kind === 'error').length, 1);
  assert.match(host.messages[0].message, /Gazette could not start in one: private startup payload/);
  await host.extension.deactivate();

  for (const capabilities of [{}, { experimental: { gazette: { authoring: 'other' } }, documentSymbolProvider: true }, { experimental: { gazette: { authoring: '0.1.0' } } }]) {
    const unlike = createEditor(['/work/one']);
    unlike.capabilities = capabilities;
    await unlike.extension.activate({ subscriptions: [] });
    assert.equal(unlike.clients.length, 1);
    assert.equal(unlike.clients[0].stopped, true);
    await unlike.extension.deactivate();
  }
});

test('startup recovery names a missing program once without holding refresh open', async () => {
  const host = createEditor(['/work/one']);
  const folder = host.vscode.workspace.workspaceFolders[0];
  host.settings.set(folder.uri.toString(), {executable: {}});
  const notices = [];
  let dismiss;
  host.vscode.window.showErrorMessage = (message, ...actions) => {
    notices.push({message, actions});
    return new Promise(resolve => {dismiss = resolve;});
  };
  const session = new host.WorkspaceSession(folder, {appendLine() {}});
  await session.refresh();
  await session.refresh();
  assert.equal(notices.length, 1);
  assert.match(notices[0].message, /gazette.server.path.*user settings/);
  assert.deepEqual(notices[0].actions, ['Choose Gazette Program', 'Open Settings', 'Show Output']);
  assert.equal(host.clients.length, 0);
  await session.shutdown();
  dismiss('Choose Gazette Program');
  await host.flush();
});

test('startup recovery actions open the requested view or save a user-level program choice', async () => {
  for (const action of ['Choose Gazette Program', 'Open Settings', 'Show Output']) {
    const host = createEditor(['/work/one']);
    const folder = host.vscode.workspace.workspaceFolders[0];
    const updates = [], commands = [], shown = [];
    host.vscode.ConfigurationTarget = {Global: 1};
    const configuration = host.vscode.workspace.getConfiguration;
    host.vscode.workspace.getConfiguration = (...args) => ({...configuration(...args),
      update: async (...args) => {updates.push(args);}});
    host.settings.set(folder.uri.toString(), {executable: {}});
    host.vscode.window.showErrorMessage = async () => action;
    host.vscode.window.showOpenDialog = async options => {
      assert.equal(options.canSelectFolders, false);
      assert.equal(options.canSelectMany, false);
      return [new Uri('/chosen/gazette')];
    };
    host.vscode.commands.executeCommand = async (...args) => {commands.push(args);};
    const session = new host.WorkspaceSession(folder, {appendLine() {}, show: value => shown.push(value)});
    await session.refresh();
    await host.flush();
    assert.deepEqual(updates, action === 'Choose Gazette Program' ? [['server.path', '/chosen/gazette', 1]] : []);
    assert.deepEqual(commands, action === 'Open Settings' ? [['workbench.action.openSettings', '@ext:rtepl.gazette-vscode']] : []);
    assert.deepEqual(shown, action === 'Show Output' ? [true] : []);
    assert.equal(host.clients.length, 0, 'the recovery action itself does not launch a process');
    await session.shutdown();
  }
});

test('nested folder selection cannot be sent by both parent and child sessions', async () => {
  const host = createEditor(['/work/one', '/work/one/nested']);
  const [parent, child] = host.vscode.workspace.workspaceFolders;
  host.settings.set(parent.uri.toString(), { 'workspace.documents': ['nested/article.md'] });
  host.settings.set(child.uri.toString(), { 'workspace.documents': ['article.md'] });
  host.put('/work/one/nested/article.md', '# Child-owned');
  await host.extension.activate({ subscriptions: [] });
  assert.equal(host.clients.length, 1);
  assert.equal(host.clients[0].clientOptions.workspaceFolder, child);
  assert.deepEqual(host.reads, []);
  await host.extension.deactivate();
});

test('failed stop retains custody and blocks replacement while other folders still clean up', async () => {
  const host = createEditor();
  await host.extension.activate({ subscriptions: [] });
  const [one, two] = host.clients;
  host.failStop.add('/work/one');
  await assert.rejects(host.commands.get('gazette.restartLanguageServer')(), AggregateError);
  assert.equal(host.clients.filter(client => client.clientOptions.workspaceFolder.uri.path === '/work/one').length, 1);
  assert.equal(one.stopped, false);
  assert.equal(two.stopped, true);
  await assert.rejects(host.commands.get('gazette.restartLanguageServer')(), AggregateError);
  assert.equal(host.clients.filter(client => client.clientOptions.workspaceFolder.uri.path === '/work/one').length, 1);
  await assert.rejects(host.extension.deactivate(), AggregateError);
  assert.ok(host.clients.filter(client => client !== one).every(client => client.stopped));
  assert.equal(host.folders.listeners.size, 0);
  await assert.rejects(host.extension.activate({ subscriptions: [] }), AggregateError);
  assert.equal(host.clients.filter(client => client.clientOptions.workspaceFolder.uri.path === '/work/one').length, 1);
});

test('startup plus uncertain cleanup is retained, and restart bursts remain coalesced', async () => {
  const failed = createEditor(['/work/one']);
  failed.failStart.add('/work/one');
  failed.failStop.add('/work/one');
  await assert.rejects(failed.extension.activate({ subscriptions: [] }), AggregateError);
  await assert.rejects(failed.commands.get('gazette.restartLanguageServer')(), AggregateError);
  assert.equal(failed.clients.length, 1);
  await assert.rejects(failed.extension.deactivate(), AggregateError);

  const host = createEditor(['/work/one']);
  let release;
  host.startGate = new Promise(resolve => { release = resolve; });
  const started = host.extension.activate({ subscriptions: [] });
  await host.flush();
  const restart = host.commands.get('gazette.restartLanguageServer');
  const requests = Array.from({ length: 20 }, () => restart());
  release();
  await Promise.all([started, ...requests]);
  assert.equal(host.clients.length, 2, 'one active start and one coalesced replacement');
  assert.equal(host.clients[0].stopped, true);
  await host.extension.deactivate();
});

test('distributed roots are exact initialize permissions and overlays; removing one retires the whole composition', async () => {
  const host = createEditor(['/work/config', '/work/owner', '/work/unrelated']);
  const [folder, owner, unrelated] = host.vscode.workspace.workspaceFolders;
  host.settings.set(folder.uri.toString(), {
    'workspace.roots': [{ id: 'owner', uri: owner.uri.toString() }],
    'workspace.documents': [{ root: 'owner', file: 'article/index.md', path: 'content/article/index.md' }],
  });
  await host.extension.activate({ subscriptions: [] });
  const client = host.clients.find(client => client.clientOptions.workspaceFolder === folder);
  assert.deepEqual(client.initializeParams.workspaceFolders, [{ uri: folder.uri.toString(), name: folder.name }, { uri: owner.uri.toString(), name: owner.name }]);
  assert.equal(client.initializeParams.baseInitializationHook, true, 'documented provider hook is preserved');
  assert.equal(client.initializeParams.workspaceFolders.some(value => value.uri === unrelated.uri.toString()), false);
  const selected = { uri: new Uri('/work/owner/article/index.md'), version: 7, getText: () => '# Overlay' };
  let calls = 0;
  await client.clientOptions.middleware.didOpen(selected, () => { calls++; });
  assert.equal(calls, 1, 'explicit foreign-root overlay reaches native transport');
  host.vscode.workspace.workspaceFolders = [folder, unrelated];
  host.folders.fire({ added: [], removed: [owner] });
  await host.flush();
  assert.equal(client.stopped, true);
  await client.clientOptions.middleware.didChange({ document: selected }, () => { calls++; });
  assert.equal(calls, 1, 'no subset continues after dependent root removal');
  assert.equal(host.clients.filter(value => value.clientOptions.workspaceFolder === folder).length, 1, 'missing explicit root refuses replacement');
  await host.extension.deactivate();
});

test('one physical source cannot be held by two compositions, including uncertain stop custody', async () => {
  const host = createEditor(['/work/one', '/work/two', '/work/shared']);
  const [one, two, shared] = host.vscode.workspace.workspaceFolders;
  for (const folder of [one, two]) host.settings.set(folder.uri.toString(), {
    'workspace.roots': [{ id: 'shared', uri: shared.uri.toString() }],
    'workspace.documents': [{ root: 'shared', file: 'index.md', path: 'content/index.md' }],
  });
  await host.extension.activate({ subscriptions: [] });
  assert.equal(host.clients.filter(client => client.clientOptions.workspaceFolder === one || client.clientOptions.workspaceFolder === two).length, 1);
  const held = host.clients.find(client => client.clientOptions.workspaceFolder === one || client.clientOptions.workspaceFolder === two);
  host.failStop.add(held.clientOptions.workspaceFolder.uri.path);
  await assert.rejects(host.commands.get('gazette.restartLanguageServer')(), AggregateError);
  assert.equal(host.clients.filter(client => client.clientOptions.workspaceFolder === one || client.clientOptions.workspaceFolder === two).length, 1, 'uncertain cleanup cannot reassign physical source');
  host.failStop.clear();
  await host.extension.deactivate();
});
