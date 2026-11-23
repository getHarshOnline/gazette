// A controlled editor/transport double for JavaScript lifecycle proof, not real LSP proof.
const assert = require('node:assert/strict');
const Module = require('node:module');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const { EventEmitter } = require('node:events');
const { PassThrough } = require('node:stream');

class Uri {
  constructor(value) { this.path = value; this.fsPath = value; this.scheme = 'file'; this.authority = ''; this.query = ''; this.fragment = ''; }
  toString() { return pathToFileURL(this.path).href; }
  static joinPath(root, ...parts) { return new Uri(path.posix.join(root.path, ...parts)); }
  static parse(value) {
    if (value.startsWith('file:')) return new Uri(require('node:url').fileURLToPath(value));
    const url = new URL(value);
    return {scheme: url.protocol.slice(0, -1), path: url.pathname, toString: () => value};
  }
}

function event() {
  const listeners = new Set();
  return { listeners, listen: callback => { listeners.add(callback); return { dispose: () => listeners.delete(callback) }; }, fire: value => { for (const callback of listeners) callback(value); } };
}

function createEditor(paths = ['/work/one', '/work/two'], processProof = false, discovery = false) {
  const host = { clients: [], reads: [], files: new Map(), settings: new Map(), commands: new Map(), logs: [], messages: [], stops: [], failStart: new Set(), failStop: new Set(), capabilities: { experimental: { gazette: { authoring: '0.1.0' } }, documentSymbolProvider: true, textDocumentSync: { openClose: true, change: 1 } } };
  if (discovery) host.capabilities.experimental.gazette.sourceDiscovery = {selectSource: 'gazette/selectSource'};
  host.contextKeys = new Map();
  host.children = []; host.spawns = []; host.closeFailedStart = true;
  host.open = event(); host.close = event(); host.edit = event(); host.selection = event(); host.active = event();
  host.watchers = [];
  host.fileEvent = (value, kind = 'change') => {
    const uri = new Uri(value);
    for (const watcher of host.watchers) if (!watcher.disposed) watcher[kind].fire(uri);
  };
  host.configuration = event(); host.folders = event(); host.trust = event();
  host.folder = value => ({ uri: new Uri(value), name: path.posix.basename(value) });
  const workspace = {
    isTrusted: true,
    textDocuments: [],
    workspaceFolders: paths.map(host.folder),
    getWorkspaceFolder: uri => workspace.workspaceFolders.filter(folder => uri.scheme === folder.uri.scheme && (uri.path === folder.uri.path || uri.path.startsWith(folder.uri.path + '/'))).sort((a, b) => b.uri.path.length - a.uri.path.length)[0],
    getConfiguration: (name, uri) => {
      assert.equal(name, 'gazette');
      const values = host.settings.get(uri.toString()) ?? {};
      // Existing lifecycle cases exercise explicit selection; native discovery
      // cases opt into its separate initialization and source-query contract.
      return { inspect: () => values.executable ?? { globalValue: '/exact/gazette' }, get: (key, fallback) => values[key] ?? (key === 'workspace.discovery' ? discovery : fallback) };
    },
    fs: { stat: async uri => {
      const bytes = host.files.get(uri.toString());
      if (!bytes) throw Object.assign(new Error('private missing path must not be logged'), {code: 'FileNotFound'});
      return { type: 1, size: bytes.byteLength };
    }, readFile: async uri => {
      host.reads.push(uri.toString());
      if (!host.files.has(uri.toString())) throw new Error('private missing path must not be logged');
      return host.files.get(uri.toString());
    } },
    registerTextDocumentContentProvider: (scheme, provider) => {
      host.contentProviders ??= new Map();
      host.contentProviders.set(scheme, provider);
      return {dispose: () => host.contentProviders.delete(scheme)};
    },
    onDidOpenTextDocument: host.open.listen,
    onDidCloseTextDocument: host.close.listen,
    onDidChangeTextDocument: host.edit.listen,
    onDidChangeConfiguration: host.configuration.listen,
    onDidChangeWorkspaceFolders: host.folders.listen,
    onDidGrantWorkspaceTrust: host.trust.listen,
    createFileSystemWatcher: pattern => {
      const create = event(), change = event(), remove = event();
      const watcher = {pattern, create, change, delete: remove, disposed: false,
        onDidCreate: create.listen, onDidChange: change.listen, onDidDelete: remove.listen,
        dispose() { this.disposed = true; create.listeners.clear(); change.listeners.clear(); remove.listeners.clear(); }};
      host.watchers.push(watcher);
      return watcher;
    },
  };
  host.vscode = { Uri, RelativePattern: class {constructor(baseUri, pattern) { this.baseUri = baseUri; this.pattern = pattern; }}, FileType: { File: 1 }, workspace, commands: { executeCommand: async (name, key, value) => { if (name === 'setContext') host.contextKeys.set(key, value); }, registerCommand: (name, callback) => { host.commands.set(name, callback); return { dispose: () => host.commands.delete(name) }; } }, window: { createOutputChannel: () => ({ appendLine: line => host.logs.push(line), dispose() {} }) } };
  host.vscode.StatusBarAlignment = {Right: 1};
  host.vscode.EventEmitter = class { constructor() { this.source = event(); this.event = this.source.listen; } fire(value) { this.source.fire(value); } dispose() { this.source.listeners.clear(); } };
  host.vscode.languages = {setTextDocumentLanguage: async (document, language) => { document.languageId = language; return document; }};
  Object.assign(host.vscode.window, {
    showErrorMessage: async message => { host.messages.push({kind: 'error', message}); },
    createStatusBarItem: () => ({show() {}, hide() {}, dispose() {}}),
    registerTreeDataProvider: () => ({dispose() {}}),
    onDidChangeActiveTextEditor: host.active.listen,
    onDidChangeTextEditorSelection: host.selection.listen,
  });
  host.put = (value, text) => host.files.set(new Uri(value).toString(), typeof text === 'string' ? Buffer.from(text) : text);
  for (const folder of workspace.workspaceFolders) host.put(folder.uri.path + '/gazette.toml', 'selected configuration');
  class LanguageClient {
    constructor(id, name, serverOptions, clientOptions) {
      Object.assign(this, { id, name, serverOptions, clientOptions, initializeResult: { capabilities: host.capabilities }, stopped: false });
      host.clients.push(this);
      this.notifications = new Map();
      this.protocol2CodeConverter = { asUri: value => new Uri(require('node:url').fileURLToPath(value)), asDiagnostics: async values => values };
      this.code2ProtocolConverter = { asUri: uri => uri.toString(), asChangeTextDocumentParams: document => ({
        textDocument: { uri: document.uri.toString(), version: document.version },
        contentChanges: [{ text: document.getText() }],
      }) };
      this.sent = [];
      this.published = [];
      this.cleared = [];
      this.symbolRegistrations = [];
      this.symbolFeature = {
        clear: () => { this.symbolRegistrations = []; this.onSymbolsChanged?.(); },
        register: data => { this.symbolRegistrations.push(data); this.onSymbolsChanged?.(); },
      };
      this.diagnostics = { set: (uri, diagnostics) => this.published.push({ uri, diagnostics }), delete: uri => this.cleared.push(uri.toString()), dispose: () => { this.diagnosticsDisposed = true; } };
    }
    onNotification(type, handler) {
      assert.equal(type.method, 'textDocument/publishDiagnostics');
      assert.equal(this.started, undefined, 'register raw diagnostic callback before starting');
      this.notifications.set(type.method, handler);
      return { dispose: () => this.notifications.delete(type.method) };
    }
    getFeature(method) { assert.equal(method, 'textDocument/documentSymbol'); return this.symbolFeature; }
    notifyDiagnostics(params) {
      const handler = this.notifications.get('textDocument/publishDiagnostics');
      assert.ok(handler, 'production registered raw diagnostic handler');
      handler(params);
    }
    fillInitializeParams(params) { params.baseInitializationHook = true; }
    async sendNotification(type, params) {
      const packet = { method: typeof type === 'string' ? type : type.method, params };
      this.sent.push(packet);
      if (host.delivery) await host.delivery(packet);
    }
    async start() {
      this.started = true;
      if (processProof) this.process = await this.serverOptions();
      this.initializeParams = {};
      this.fillInitializeParams(this.initializeParams);
      if (host.startGate) await host.startGate;
      if (host.failStart.has(this.clientOptions.workspaceFolder.uri.path)) {
        if (processProof === true && host.closeFailedStart) this.process.emit('close', 0, null);
        throw new Error('private startup payload');
      }
    }
    async stop() {
      host.stops.push(this.id);
      if (host.failStop.has(this.clientOptions.workspaceFolder.uri.path)) throw new Error('private uncertain process');
      if (processProof && host.failStart.has(this.clientOptions.workspaceFolder.uri.path)) throw new Error('Client is not running and cannot be stopped');
      this.stopped = true;
      if (this.process && processProof === true && host.closeRunningStop !== false) this.process.emit('close', 0, null);
    }
  }
  const original = Module._load;
  Module._load = function load(request, parent, isMain) {
    if (request === 'vscode') return host.vscode;
    if (request === 'node:child_process' && processProof === true) return { spawn(command, args, options) {
      host.spawns.push({ command, args, options });
      const child = new EventEmitter();
      child.stdin = new PassThrough(); child.stdout = new PassThrough(); child.stderr = new PassThrough();
      child.kill = () => { throw new Error('unproved child must not be killed or replaced'); };
      host.children.push(child);
      queueMicrotask(() => child.emit('spawn'));
      return child;
    } };
    if (request === 'vscode-languageclient/node') return { LanguageClient, CancellationToken: {None: {isCancellationRequested: false}}, PublishDiagnosticsNotification: { type: { method: 'textDocument/publishDiagnostics' } }, DidChangeTextDocumentNotification: { type: { method: 'textDocument/didChange' } }, TextDocumentSyncKind: { None: 0, Full: 1, Incremental: 2 }, ErrorAction: { Shutdown: 1 }, CloseAction: { DoNotRestart: 2 }, RevealOutputChannelOn: { Never: 3 } };
    return original.call(this, request, parent, isMain);
  };
  try {
    for (const name of ['extension', 'workspaceSession', 'languageClient', 'sourceCapture', 'settings', 'authoringView', 'editorSnapshot']) delete require.cache[require.resolve('../../dist/' + name + '.js')];
    host.extension = require('../../dist/extension.js');
    host.WorkspaceSession = require('../../dist/workspaceSession.js').WorkspaceSession;
    host.SelectedLanguageClient = require('../../dist/languageClient.js').SelectedLanguageClient;
  } finally { Module._load = original; }
  host.flush = async () => { for (let i = 0; i < 6; i++) await new Promise(resolve => setImmediate(resolve)); };
  return host;
}

module.exports = { createEditor, Uri };
