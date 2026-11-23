const test = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const { PassThrough } = require('node:stream');
const { createMessageConnection } = require('vscode-jsonrpc/node');
const Module = require('node:module');

test('actual pinned Full converter snapshots complete Unicode/CRLF bytes and version synchronously', () => {
  const root = path.dirname(require.resolve('vscode-languageclient/package.json'));
  const original = Module._load;
  // Only the unavailable editor classes are inert here. Conversion is the
  // installed native implementation, not the controlled lifecycle converter.
  Module._load = function(request, parent, isMain) {
    if (request === 'vscode') return new Proxy({}, { get: () => class {} });
    return original.call(this, request, parent, isMain);
  };
  let converter;
  try { converter = require(path.join(root, 'lib/common/codeConverter.js')).createConverter(); }
  finally { Module._load = original; }
  const document = { uri: { toString: () => 'file:///selected/%C3%A9.md' }, version: 4,
    text: '# é😀\r\nComplete source\r\n', getText() { return this.text; } };
  const packet = converter.asChangeTextDocumentParams(document);
  document.version = 5; document.text = 'next';
  assert.deepEqual(packet, { textDocument: { uri: 'file:///selected/%C3%A9.md', version: 4 },
    contentChanges: [{ text: '# é😀\r\nComplete source\r\n' }] });
});

test('actual pinned Delayer reproduces the post-retirement reentrant task loss', async () => {
  const root = path.dirname(require.resolve('vscode-languageclient/package.json'));
  const { Delayer } = require(path.join(root, 'lib/common/utils/async.js'));
  const delayer = new Delayer(0);
  let retriggered;
  await delayer.trigger(() => {
    // client.js retriggers when the active connection has been retired. The
    // outer native then-handler clears the newly assigned task after invocation.
    retriggered = delayer.trigger(() => assert.fail('replacement task was retained'), 0);
    retriggered.catch(() => {});
  }, 0);
  await assert.rejects(retriggered, /this.task is not a function/);
});

test('pinned client installs pre-start public notification handlers after its builtin diagnostics handler', () => {
  const root = path.dirname(require.resolve('vscode-languageclient/package.json'));
  assert.equal(require('vscode-languageclient/package.json').version, '9.0.1');
  const source = readFileSync(path.join(root, 'lib/common/client.js'), 'utf8');
  const registration = source.indexOf('this._pendingNotificationHandlers.set(method, handler);');
  const builtin = source.indexOf('connection.onNotification(vscode_languageserver_protocol_1.PublishDiagnosticsNotification.type,');
  const pending = source.indexOf('for (const [method, handler] of this._pendingNotificationHandlers)', builtin);
  const install = source.indexOf('connection.onNotification(method, handler)', pending);
  assert.ok(registration >= 0, 'public registration retains handlers while no connection exists');
  assert.ok(builtin > registration && pending > builtin && install > pending,
    'qualified provider source installs pending public handler after builtin handler');
});

test('pinned client documented initialization hook runs after its single-folder projection', () => {
  const root = path.dirname(require.resolve('vscode-languageclient/package.json'));
  const source = readFileSync(path.join(root, 'lib/common/client.js'), 'utf8');
  const folder = source.indexOf('workspaceFolders: workspaceFolders');
  const hook = source.indexOf('this.fillInitializeParams(initParams);', folder);
  const dispatch = source.indexOf('return this.doInitialize(connection, initParams);', hook);
  assert.ok(folder >= 0 && hook > folder && dispatch > hook, 'documented hook supplies rooted permissions before native initialize');
});

test('actual pinned JSON-RPC transport delivers the full versioned packet only to the replacement handler', async () => {
  const input = new PassThrough();
  const output = new PassThrough();
  const connection = createMessageConnection(input, output);
  let builtinCalls = 0;
  connection.onNotification('textDocument/publishDiagnostics', () => builtinCalls++);
  let timeout;
  const delivered = new Promise((resolve, reject) => {
    timeout = setTimeout(() => reject(new Error('native notification was not delivered')), 2000);
    connection.onNotification('textDocument/publishDiagnostics', resolve);
  });
  try {
    connection.listen();
    const params = { uri: 'file:///selected.md', version: 42, diagnostics: [] };
    const message = JSON.stringify({ jsonrpc: '2.0', method: 'textDocument/publishDiagnostics', params });
    input.write(`Content-Length: ${Buffer.byteLength(message)}\r\n\r\n${message}`);
    assert.deepEqual(await delivered, params);
    assert.equal(builtinCalls, 0, 'replacement does not allow an unversioned builtin publication');
  } finally {
    clearTimeout(timeout);
    connection.dispose();
    input.destroy();
    output.destroy();
  }
});
