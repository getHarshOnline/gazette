const assert = require('node:assert/strict');
const { test } = require('node:test');
const { createEditor, Uri } = require('./support/editor.cjs');

async function fixture() {
  const host = createEditor(['/work/one']);
  const folder = host.vscode.workspace.workspaceFolders[0];
  host.settings.set(folder.uri.toString(), { 'workspace.documents': ['guide.md'] });
  host.put('/work/one/guide.md', 'Note[^one].\n\n[^one]: Definition.');
  const session = new host.WorkspaceSession(folder, { appendLine: line => host.logs.push(line) });
  await session.refresh();
  const document = {
    uri: new Uri('/work/one/guide.md'), version: 2, isClosed: false,
    text: 'Note[^one].\n\n[^one]: Definition.', getText() { return this.text; },
  };
  host.vscode.workspace.textDocuments.push(document);
  const client = session.client;
  const token = { isCancellationRequested: false };
  const position = { line: 0, character: 6 };
  const options = { includeDeclaration: true };
  const invoke = (kind, forward, selected = document) => kind === 'references'
    ? client.clientOptions.middleware.provideReferences(selected, position, options, token, forward)
    : kind === 'links' ? client.clientOptions.middleware.provideDocumentLinks(selected, token, forward)
    : kind === 'completion' ? client.clientOptions.middleware.provideCompletionItem(selected, position, options, token, forward)
    : client.clientOptions.middleware[kind === 'hover' ? 'provideHover' : 'provideDefinition'](selected, position, token, forward);
  return { host, session, client, document, token, position, options, invoke };
}

test('standard navigation forwards exact selected arguments and preserves server results without interpretation', async () => {
  const f = await fixture();
  try {
    for (const kind of ['definition', 'references', 'hover', 'completion', 'links']) {
      const result = kind === 'hover' ? { contents: ['Resolved principal: owner:one'], range: {} }
        : [{ uri: f.document.uri, range: { start: { line: 2, character: 2 }, end: { line: 2, character: 5 } } }];
      let calls = 0;
      assert.equal(await f.invoke(kind, (...args) => {
        calls++;
        assert.deepEqual(args, kind === 'links' ? [f.document, f.token] : kind === 'references' || kind === 'completion'
          ? [f.document, f.position, f.options, f.token]
          : [f.document, f.position, f.token]);
        return Promise.resolve(result);
      }), result);
      assert.equal(calls, 1);
      assert.equal(await f.invoke(kind, () => null), null);
    }
  } finally { await f.session.shutdown(); }
});

test('unselected, closed, foreign-folder and already cancelled requests never reach transport', async () => {
  for (const refusal of ['unselected', 'closed', 'foreign-folder', 'cancelled', 'not-open']) {
    const f = await fixture();
    try {
      let document = f.document;
      if (refusal === 'unselected') {
        document = { ...document, uri: new Uri('/work/one/unselected.md') };
        f.host.vscode.workspace.textDocuments.push(document);
      } else if (refusal === 'closed') document.isClosed = true;
      else if (refusal === 'foreign-folder') f.host.vscode.workspace.workspaceFolders = [];
      else if (refusal === 'cancelled') f.token.isCancellationRequested = true;
      else f.host.vscode.workspace.textDocuments = [];
      for (const kind of ['definition', 'references', 'hover', 'completion', 'links']) {
        assert.equal(await f.invoke(kind, () => assert.fail('refused request reached transport'), document), null);
      }
    } finally { await f.session.shutdown(); }
  }
});

test('in-flight navigation cannot escape changed buffers, ownership, cancellation or retired sessions', async () => {
  for (const kind of ['definition', 'references', 'hover', 'completion', 'links']) {
    for (const transition of ['version', 'source', 'reopen', 'folder', 'cancel', 'restart', 'shutdown', 'failed-stop']) {
      const f = await fixture();
      try {
        let release;
        const pending = f.invoke(kind, () => new Promise(resolve => { release = resolve; }));
        if (transition === 'version') f.document.version++;
        else if (transition === 'source') f.document.text += '\nChanged without a trusted version.';
        else if (transition === 'reopen') {
          f.document.isClosed = true;
          f.host.vscode.workspace.textDocuments.push({ ...f.document, isClosed: false });
        } else if (transition === 'folder') f.host.vscode.workspace.workspaceFolders = [];
        else if (transition === 'cancel') f.token.isCancellationRequested = true;
        else if (transition === 'restart') await f.session.refresh();
        else if (transition === 'shutdown') await f.session.shutdown();
        else {
          f.host.failStop.add('/work/one');
          await assert.rejects(f.session.refresh(), /private uncertain process/);
          assert.equal(f.session.client, f.client, 'uncertain process custody is retained');
        }
        release([{ uri: f.document.uri, range: {} }]);
        assert.equal(await pending, null, `${kind}: ${transition}`);
      } finally {
        f.host.failStop.clear();
        await f.session.shutdown();
      }
    }
  }
});

test('current transport failures remain failures, while retired results and errors are discarded', async () => {
  const f = await fixture();
  try {
    const failure = new Error('native transport failure');
    await assert.rejects(f.invoke('definition', () => Promise.reject(failure)), error => error === failure);
    let reject;
    const pending = f.invoke('references', () => new Promise((_, fail) => { reject = fail; }));
    f.client.retire();
    reject(failure);
    assert.equal(await pending, null);
    assert.equal(await f.invoke('definition', () => assert.fail('retired client forwarded')), null);
  } finally { await f.session.shutdown(); }
});

test('cross-document results bind every selected open sibling without tracking unrelated buffers', async () => {
  for (const transition of ['version', 'source', 'close', 'reopen', 'open', 'unselected']) {
    const f = await fixture();
    try {
      // The configured workspace is a selected sibling even when querying Markdown.
      const sibling = { ...f.document, uri: new Uri('/work/one/gazette.toml'), text: 'original workspace' };
      if (transition !== 'open') f.host.vscode.workspace.textDocuments.push(sibling);
      let release;
      const result = [{ uri: sibling.uri, range: {} }];
      const pending = f.invoke('definition', () => new Promise(resolve => { release = resolve; }));
      if (transition === 'version') sibling.version++;
      else if (transition === 'source') sibling.text += ' changed';
      else if (transition === 'close') sibling.isClosed = true;
      else if (transition === 'reopen') {
        sibling.isClosed = true;
        f.host.vscode.workspace.textDocuments.push({ ...sibling, isClosed: false });
      } else if (transition === 'open') f.host.vscode.workspace.textDocuments.push(sibling);
      else f.host.vscode.workspace.textDocuments.push({ ...sibling, uri: new Uri('/work/one/unselected.md') });
      release(result);
      assert.equal(await pending, transition === 'unselected' ? result : null, transition);
    } finally { await f.session.shutdown(); }
  }
});

test('foreign-root sibling edits and removed roots invalidate every in-flight navigation result', async () => {
  for (const transition of ['version', 'source', 'root']) {
    const host = createEditor(['/work/config', '/work/one', '/work/two']);
    const [folder, one, two] = host.vscode.workspace.workspaceFolders;
    host.settings.set(folder.uri.toString(), {
      'workspace.roots': [{ id: 'one', uri: one.uri.toString() }, { id: 'two', uri: two.uri.toString() }],
      'workspace.documents': [{ root: 'one', file: 'index.md', path: 'one/index.md' }, { root: 'two', file: 'index.md', path: 'two/index.md' }],
    });
    const session = new host.WorkspaceSession(folder, { appendLine() {} });
    await session.refresh();
    const document = { uri: new Uri('/work/one/index.md'), version: 1, text: '# One', isClosed: false, getText() { return this.text; } };
    const sibling = { ...document, uri: new Uri('/work/two/index.md'), text: '# Two' };
    host.vscode.workspace.textDocuments.push(document, sibling);
    let release;
    const pending = session.client.clientOptions.middleware.provideDefinition(document, {}, { isCancellationRequested: false }, () => new Promise(resolve => { release = resolve; }));
    if (transition === 'version') sibling.version++;
    else if (transition === 'source') sibling.text += ' unsaved';
    else host.vscode.workspace.workspaceFolders = [folder, one];
    release([{ uri: sibling.uri, range: {} }]);
    assert.equal(await pending, null, transition);
    await session.shutdown();
  }
});
