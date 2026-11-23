// The real command/controller with editor and application-transport doubles.
// Native transport/process qualification lives in applicationSession and host tests.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const Module = require('node:module');
const { createEditor } = require('./support/editor.cjs');
const applicationCommandCatalog = require('../src/applicationCommandCatalog.json');
const extensionManifest = require('../package.json');
const { GUIDED_REQUIRED_FIELD_LIMIT, guidedRequest } = require('../dist/applicationRequest.js');
const { coherentEffectProfile } = require('../dist/applicationSession.js');

function deferred() {
  let resolve;
  const promise = new Promise(value => { resolve = value; });
  return { promise, resolve };
}

function tool(name, inputSchema = { type: 'object' }, capability = 'build', metadata = {}) {
  const defaults = {
    'io.rtepl.gazette/intent': 'act',
    'io.rtepl.gazette/effect': 'external',
    'io.rtepl.gazette/potentialEffects': ['network'],
    'io.rtepl.gazette/prominence': 'common',
    'io.rtepl.gazette/closure': 'self-terminal',
  };
  return {
    name,
    title: `${capability} · ${name}`,
    description: 'Native operation',
    inputSchema,
    outputSchema: { type: 'object' },
    annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false, openWorldHint: true },
    _meta: { 'io.rtepl.gazette/capability': capability, ...defaults, ...metadata },
  };
}

function requiredLeafSchema(count) {
  const names = Array.from({ length: count }, (_, index) => `required_${index + 1}`);
  return {
    type: 'object',
    required: names,
    properties: Object.fromEntries(names.map(name => [name, { type: 'string' }])),
  };
}

function fixture(paths = ['/work/one']) {
  const state = {
    sessions: [], errors: [], displayed: [], pickers: [], dialogs: [], notices: [], inputs: [],
    tools: [tool('native_future_operation')],
    catalog: '{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"native_future_operation","title":"Build · Native future operation","inputSchema":{"maximum":9007199254740993},"outputSchema":{"type":"object"},"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true},"_meta":{"io.rtepl.gazette/capability":"build"}}]}}',
    result: { raw: '{"jsonrpc":"2.0","id":2,"result":{"content":[],"isError":false}}', isError: false },
  };
  class ApplicationError extends Error {
    constructor(message, effect, raw) { super(message); this.effect = effect; this.raw = raw; }
  }
  class ApplicationSession {
    constructor(executable, cwd) { Object.assign(this, { executable, cwd, calls: [], discoveries: 0, stops: 0, closed: false, busy: false }); state.sessions.push(this); }
    async discover() {
      this.discoveries++;
      if (state.discoverGate) await state.discoverGate.promise;
      return { tools: state.tools, raw: state.catalog };
    }
    async call(name, argumentsJson) {
      if (this.busy || this.closed) throw new ApplicationError('private busy detail', 'not-dispatched');
      this.busy = true;
      this.calls.push({ name, argumentsJson });
      try {
        if (state.callGate) await state.callGate.promise;
        if (state.callError) throw state.callError;
        return state.callResult ? await state.callResult(name) : state.result;
      } finally { this.busy = false; }
    }
    async shutdown() {
      this.stops++;
      if (this.busy || state.failStop) throw new ApplicationError('private retained process', 'unknown');
      this.closed = true;
    }
  }
  const original = Module._load;
  Module._load = function(request, parent, isMain) {
    if (request === './applicationSession' && parent?.filename.endsWith('/applicationCommands.js')) {
      return { ApplicationSession, ApplicationError, coherentEffectProfile };
    }
    return original.call(this, request, parent, isMain);
  };
  let host;
  try {
    delete require.cache[require.resolve('../dist/applicationCommands.js')];
    host = createEditor(paths);
  } finally { Module._load = original; }
  let sequence = 0;
  const window = host.vscode.window;
  const backButton = {};
  host.vscode.QuickInputButtons = { Back: backButton };
  const signal = () => {
    const listeners = new Set();
    return {
      listen(callback) { listeners.add(callback); return { dispose: () => listeners.delete(callback) }; },
      fire(value) { for (const callback of [...listeners]) callback(value); },
    };
  };
  const control = () => {
    const accept = signal();
    const button = signal();
    const hide = signal();
    let visible = false;
    return {
      title: '', step: undefined, totalSteps: undefined, prompt: undefined, value: '', password: false,
      ignoreFocusOut: false, buttons: [], items: [], selectedItems: [],
      onDidAccept: accept.listen,
      onDidTriggerButton: button.listen,
      onDidHide: hide.listen,
      hide() { if (visible) { visible = false; hide.fire(); } },
      dispose() {},
      showQuickPick() {
        visible = true;
        const options = { title: this.title, step: this.step, totalSteps: this.totalSteps, ignoreFocusOut: this.ignoreFocusOut };
        state.pickers.push({ items: this.items, options });
        const selected = state.pick ? state.pick(this.items, options) : this.items[0];
        if (selected?.back === true) button.fire(backButton);
        else if (selected === undefined) this.hide();
        else { this.selectedItems = [selected]; accept.fire(); }
      },
      showInputBox() {
        visible = true;
        state.inputs.push(this);
        const entered = state.input ? state.input(this) : '';
        if (entered?.back === true) button.fire(backButton);
        else if (entered === undefined) this.hide();
        else { this.value = entered; accept.fire(); }
      },
    };
  };
  window.createQuickPick = () => {
    const input = control();
    input.show = input.showQuickPick;
    return input;
  };
  window.createInputBox = () => {
    const input = control();
    input.show = input.showInputBox;
    return input;
  };
  host.vscode.workspace.openTextDocument = async options => {
    if (state.failOpen) throw new Error('private editor failure');
    const id = ++sequence;
    const virtual = options.scheme === 'gazette-result';
    const existing = virtual && host.vscode.workspace.textDocuments.find(document => !document.isClosed && document.uri.toString() === options.toString());
    if (existing) return existing;
    const uri = virtual ? options : { scheme: 'untitled', toString: () => `untitled:gazette-request-${id}` };
    const text = virtual ? host.contentProviders.get(uri.scheme).provideTextDocumentContent(uri) : options.content;
    const document = { uri, languageId: virtual ? 'plaintext' : options.language, text, version: 1, isClosed: false, isUntitled: !virtual, isDirty: !virtual, getText() { return this.text; } };
    host.vscode.workspace.textDocuments.push(document);
    return document;
  };
  window.showTextDocument = async (document, options) => {
    if (state.failDisplay) throw new Error('private display failure');
    state.displayed.push(document);
    if (!options.preserveFocus) window.activeTextEditor = { document };
    return { document };
  };
  window.showQuickPick = async (items, options) => {
    state.pickers.push({ items, options });
    if (state.pick) return state.pick(items, options);
    return items[0];
  };
  window.showWarningMessage = async (message, options, ...remaining) => {
    const actions = typeof options === 'string' ? [options, ...remaining] : remaining;
    const settings = typeof options === 'string' ? { modal: false } : options;
    state.dialogs.push({ message, options: settings, action: actions[0], actions });
    if (settings.modal) return state.confirm ? state.confirm(actions[0]) : actions[0];
    return state.notice ? state.notice(actions) : actions[0];
  };
  window.showInformationMessage = async (message, ...actions) => {
    state.notices.push({ message, action: actions[0], actions });
    return state.notice ? state.notice(actions) : actions[0];
  };
  window.showInputBox = async options => {
    state.inputs.push(options);
    return state.input ? state.input(options) : '';
  };
  window.showErrorMessage = async message => { state.errors.push(message); };
  const run = (name, ...args) => {
    const command = host.commands.get(`gazette.${name}`);
    assert.ok(command, `registered application command ${name}`);
    return command(...args);
  };
  const edit = (document, text) => { document.text = text; document.version++; window.activeTextEditor = { document }; };
  const create = async name => {
    const uri = await run('createOperationRequest', name);
    return host.vscode.workspace.textDocuments.find(document => document.uri.toString() === uri.toString());
  };
  return { host, state, run, edit, create, ApplicationError };
}

for (const configuration of ['gazette.toml', 'sites/authoring.toml']) test(`Check Saved Site uses the active folder's ${configuration} and shows plain problems`, async () => {
  const f = fixture(['/work/one', '/work/two']);
  f.host.settings.set('file:///work/two', {'workspace.configuration': configuration});
  const descriptor = tool('check_workspace', {type: 'object'}, 'workspace', {
    'io.rtepl.gazette/effect': 'read', 'io.rtepl.gazette/potentialEffects': ['filesystem-read'],
  });
  descriptor.annotations = {readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false};
  f.state.tools = [descriptor];
  f.state.result = {isError: true, raw: JSON.stringify({result: {structuredContent: {
    workspace_location: {root: '/work/two', workspace: 'gazette.toml', found_by: 'request'},
    checked_sites: ['site:two'], diagnostics: [{file: '/work/two/page.md', line: 7, column: 14,
      code: 'content_reference', message: 'Missing content: collection:missing'}],
  }}})};
  const progress = [];
  f.host.vscode.ProgressLocation = {Window: 10};
  f.host.vscode.window.withProgress = async (options, run) => {progress.push(options); return run();};
  await f.host.extension.activate({subscriptions: []});
  f.host.vscode.window.activeTextEditor = {document: {uri: f.host.vscode.Uri.parse('file:///work/two/page.md')}};
  f.state.callGate = deferred();
  const first = f.run('checkSite');
  await f.host.flush();
  assert.equal(await f.run('checkSite'), undefined, 'a repeated click cannot queue a second check');
  f.state.callGate.resolve();
  const document = await first;
  assert.deepEqual(f.state.sessions[0].calls, [{name: 'check_workspace', argumentsJson: JSON.stringify({workspace: `/work/two/${configuration}`})}]);
  assert.equal(progress[0].location, 10);
  assert.equal(document.languageId, 'plaintext');
  assert.equal(document.uri.scheme, 'gazette-result');
  assert.match(document.getText(), /Sites checked: 1\nProblems found: 1/);
  assert.match(document.getText(), /\/work\/two\/page.md:7:14: content_reference: Missing content: collection:missing/);
  assert.equal(f.state.dialogs.length, 0);
  assert.equal(f.state.pickers.length, 0);
  await f.host.extension.deactivate();
});

test('Check Saved Site refuses a tool advertising writes and permits a later corrected check', async () => {
  const f = fixture();
  f.state.tools = [tool('check_workspace')];
  f.host.vscode.ProgressLocation = {Window: 10};
  f.host.vscode.window.withProgress = async (_options, run) => run();
  await f.host.extension.activate({subscriptions: []});
  await assert.rejects(f.run('checkSite'), /read-only site check/);
  assert.equal(f.state.sessions[0].calls.length, 0);
  f.state.tools[0]._meta['io.rtepl.gazette/effect'] = 'read';
  f.state.tools[0]._meta['io.rtepl.gazette/potentialEffects'] = ['filesystem-read'];
  f.state.tools[0].annotations = {readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false};
  f.state.result = {isError: true, raw: JSON.stringify({result: {structuredContent: {
    code: 'workspace_not_found', message: 'No gazette.toml found.',
  }}})};
  const document = await f.run('checkSite');
  assert.match(document.getText(), /check could not finish/);
  assert.match(document.getText(), /No gazette.toml found/);
  await f.host.extension.deactivate();
});

test('live catalog drives new operations independently of failed LSP capture and preserves exact private bytes', async () => {
  const f = fixture();
  f.host.files.clear();
  f.host.failStart.add('/work/one');
  await f.host.extension.activate({ subscriptions: [] });
  assert.equal(f.host.clients.length, 1, 'exact locators reach the native capture owner');
  assert.equal(f.host.clients[0].stopped, true, 'failed native LSP capture is cleaned up, not a usable authoring session');
  const request = await f.create();
  assert.equal(f.state.sessions.length, 1);
  assert.equal(f.state.sessions[0].executable, '/exact/gazette');
  assert.equal(f.state.sessions[0].cwd, '/work/one');
  assert.equal(request.getText(), '{}\n', 'no permission, path or schema default is inserted');
  assert.ok(!f.state.displayed.some(document => document.getText() === f.state.catalog),
    'creating one request does not dump the complete catalog into the editor');
  assert.deepEqual(f.state.sessions[0].calls, [], 'creation and schema viewing do not execute tools');
  await f.run('showApplicationTools');
  assert.ok(f.state.displayed.some(document => document.getText() === f.state.catalog),
    'the complete catalog remains explicitly inspectable');
  assert.deepEqual(f.state.sessions[0].calls, []);
  const exact = '{\n "large": 9007199254740993, "precise": 1.2300e+40,\n "escaped": "\\u0061", "duplicate": 1, "duplicate": 2\n}\n';
  f.edit(request, exact);
  const response = await f.run('executeOperationRequest');
  assert.deepEqual(f.state.sessions[0].calls, [{ name: 'native_future_operation', argumentsJson: exact }]);
  assert.equal(response.getText(), f.state.result.raw);
  assert.equal(request.getText(), exact, 'no save, formatting or buffer rewrite');
  assert.equal(f.state.dialogs[0].options.modal, true);
  assert.equal(f.state.dialogs[0].action, 'Run');
  assert.match(f.state.dialogs[0].message, /uses the network.*may replace or remove state.*repeat may create another effect/);
  assert.doesNotMatch(f.state.dialogs[0].message, /authority/);
  assert.match(f.state.notices[0].message, /completed successfully.*full native receipt is retained/i);
  assert.deepEqual(f.host.files.size, 0);
  assert.deepEqual(f.host.logs, ['Gazette could not start in one: private startup payload'],
    'local output names the LSP startup cause without copying application requests or results');
  await f.host.extension.deactivate();
});

test('explicit Execute skips a modal only for coherent closed-world pure reads', async () => {
  const cases = [
    ['pure read', 'read', [], { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, false, false],
    ['local effect', 'local', ['memory-state'], { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false }, true, false],
    ['documentation assembly', 'local', ['filesystem-read', 'filesystem-write', 'sqlite', 'process'], { readOnlyHint: false, destructiveHint: true, idempotentHint: false, openWorldHint: true }, true, false],
    ['external effect', 'external', ['network'], { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: true }, true, false],
    ['remote observation', 'external', ['filesystem-read', 'process', 'network'], { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: true }, true, false],
    ['repeat-sensitive read', 'read', [], { readOnlyHint: true, destructiveHint: false, idempotentHint: false, openWorldHint: false }, true, false],
    ['native write conflict', 'read', ['filesystem-write'], { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false }, true, true],
    ['destructive conflict', 'read', [], { readOnlyHint: true, destructiveHint: true, idempotentHint: true, openWorldHint: false }, true, true],
    ['local read-only mismatch', 'local', ['memory-state'], { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, true, true],
  ];
  for (const [name, effect, potentialEffects, annotations, confirmation, unknown] of cases) {
    const f = fixture();
    const descriptor = tool('classified_operation', { type: 'object' }, 'build', {
      'io.rtepl.gazette/intent': 'inspect',
      'io.rtepl.gazette/effect': effect,
      'io.rtepl.gazette/potentialEffects': potentialEffects,
    });
    descriptor.annotations = annotations;
    f.state.tools = [descriptor];
    await f.host.extension.activate({ subscriptions: [] });
    const request = await f.create('classified_operation');
    f.edit(request, '{}');
    await f.run('executeOperationRequest');
    assert.equal(f.state.dialogs.length, confirmation ? 1 : 0, name);
    if (confirmation) {
      assert.equal(/effect unknown/.test(f.state.dialogs[0].message), unknown, name);
      if (name === 'documentation assembly') {
        assert.match(f.state.dialogs[0].message, /reads local files.*writes local files.*SQLite coordination or database state.*runs native subprocesses/);
        assert.doesNotMatch(f.state.dialogs[0].message, /network|external systems/);
      }
      if (name === 'remote observation') {
        assert.match(f.state.dialogs[0].message, /read-only.*reads local files.*runs native subprocesses.*uses the network/);
      }
    }
    assert.deepEqual(f.state.sessions[0].calls, [{ name: 'classified_operation', argumentsJson: '{}' }], name);
    await f.host.extension.deactivate();
  }
});

test('task commands select exact live tools and start from a minimal guided request without choosing values or effects', async () => {
  const f = fixture();
  const workflows = [
    ['application.check', 'check_workspace'],
    ['application.render-sites', 'render_sites'],
    ['application.preview-sites', 'start_preview'],
    ['application.assemble-docs', 'assemble_docs'],
    ['application.qualify-sites', 'qualify_sites'],
    ['application.prepare-sites', 'prepare_sites'],
  ];
  const schema = {
    type: 'object',
    properties: {
      recipe: { $ref: '#/$defs/recipe' },
      allow_execute: { type: 'boolean' },
      optional_mode: { enum: ['local', 'remote'] },
    },
    $defs: {
      recipe: {
        type: 'object',
        properties: {
          configuration: { type: 'string' },
          sites: { type: 'array', items: { type: 'string' } },
        },
      },
    },
  };
  f.state.tools = [...new Set(applicationCommandCatalog.flatMap(route => route.tools))]
    .map(name => tool(name, schema));
  f.state.catalog = JSON.stringify({ jsonrpc: '2.0', id: 1, result: { tools: f.state.tools } });
  await f.host.extension.activate({ subscriptions: [] });
  for (const [command, name] of workflows) {
    const uri = await f.run(command);
    const document = f.host.vscode.workspace.textDocuments.find(candidate => candidate.uri.toString() === uri.toString());
    assert.deepEqual(JSON.parse(document.getText()), {});
    assert.equal(f.state.sessions[0].calls.length, 0, `${name} command prepares but never dispatches`);
  }
  assert.equal(f.state.sessions[0].discoveries, workflows.length);
  await f.host.extension.deactivate();
});

test('guided requests collect required and selected optional live-schema fields without recreating native validation', async () => {
  const f = fixture();
  f.state.tools = [tool('render_sites', {
    type: 'object',
    required: ['recipe', 'allow_execute'],
    properties: {
      recipe: { $ref: '#/$defs/recipe' },
      allow_execute: { type: 'boolean', description: 'Explicit execution permission.' },
      optional_mode: { enum: ['local', 'remote'] },
    },
    $defs: {
      recipe: {
        type: 'object',
        required: ['configuration'],
        properties: {
          configuration: { type: 'string', description: 'Exact configuration path.' },
          sites: { type: 'array', items: { type: 'string' } },
        },
      },
    },
  })];
  let optionalChosen = false;
  f.state.pick = (items, options) => {
    if (options.title.endsWith(': request mode')) return items.find(item => item.mode === 'guided');
    if (options.title.includes('allow_execute')) return items.find(item => item.value === true);
    if (options.title.endsWith(': optional fields')) {
      if (!optionalChosen) {
        optionalChosen = true;
        return items.find(item => item.value?.path.at(-1) === 'optional_mode');
      }
      return items.find(item => item.label === 'Review request');
    }
    if (options.title.includes('optional_mode')) return items.find(item => item.value === 'remote');
    return items[0];
  };
  f.state.input = options => options.title.includes('configuration') ? '/workspace/gazette.toml' : undefined;
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('render_sites');
  assert.deepEqual(JSON.parse(request.getText()), {
    recipe: { configuration: '/workspace/gazette.toml' },
    allow_execute: true,
    optional_mode: 'remote',
  });
  assert.equal(f.state.sessions[0].calls.length, 0);
  assert.equal(f.state.inputs.length, 1);
  await f.host.extension.deactivate();
});

test('live required-leaf complexity has one explicit guided threshold', () => {
  assert.equal(GUIDED_REQUIRED_FIELD_LIMIT, 8);
  const boundary = guidedRequest(requiredLeafSchema(GUIDED_REQUIRED_FIELD_LIMIT));
  assert.equal(boundary.requiredLeafCount, 8);
  assert.equal(boundary.defaultMode, 'guided');
  for (const count of [13, 16, 21]) {
    const representative = guidedRequest(requiredLeafSchema(count));
    assert.equal(representative.requiredLeafCount, count);
    assert.equal(representative.defaultMode, 'advanced');
  }
});

test('bounded native multi-step guidance exposes step totals and supports Back without dispatch', async () => {
  const f = fixture();
  f.state.tools = [tool('two_step', requiredLeafSchema(2))];
  let input = 0;
  f.state.pick = (items, options) => options.title.endsWith(': request mode')
    ? items.find(item => item.mode === 'guided') : items[0];
  f.state.input = options => {
    input++;
    if (input === 1) return 'first-original';
    if (input === 2) return { back: true };
    if (input === 3) return 'first-revised';
    return 'second';
  };
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('two_step');
  assert.deepEqual(JSON.parse(request.getText()), {
    required_1: 'first-revised',
    required_2: 'second',
  });
  assert.deepEqual(f.state.inputs.map(entry => [entry.step, entry.totalSteps]), [
    [1, 2], [2, 2], [1, 2], [2, 2],
  ]);
  assert.deepEqual(f.state.sessions[0].calls, []);
  await f.host.extension.deactivate();
});

test('cancelling a native guided step creates no request and dispatches nothing', async () => {
  const f = fixture();
  f.state.tools = [tool('two_step', requiredLeafSchema(2))];
  f.state.pick = (items, options) => options.title.endsWith(': request mode')
    ? items.find(item => item.mode === 'guided') : items[0];
  f.state.input = options => options.step === 1 ? 'first' : undefined;
  await f.host.extension.activate({ subscriptions: [] });
  assert.equal(await f.run('createOperationRequest', 'two_step'), undefined);
  assert.equal(f.state.displayed.length, 0);
  assert.deepEqual(f.state.sessions[0].calls, []);
  await f.host.extension.deactivate();
});

test('each native guided step rechecks the exact folder-bound session before continuing', async () => {
  const f = fixture();
  f.state.tools = [tool('two_step', requiredLeafSchema(2))];
  f.state.pick = (items, options) => options.title.endsWith(': request mode')
    ? items.find(item => item.mode === 'guided') : items[0];
  f.state.input = () => {
    const folder = f.host.vscode.workspace.workspaceFolders[0];
    f.host.settings.set(folder.uri.toString(), { executable: { globalValue: '/changed/gazette' } });
    return 'entered-before-change';
  };
  await f.host.extension.activate({ subscriptions: [] });
  await assert.rejects(f.run('createOperationRequest', 'two_step'));
  assert.equal(f.state.inputs.length, 1);
  assert.equal(f.state.displayed.length, 0);
  assert.deepEqual(f.state.sessions[0].calls, []);
  await f.host.extension.deactivate();
});

test('complex live schemas open exact inert Advanced JSON without a serial wizard or dispatch', async () => {
  const f = fixture();
  const schema = requiredLeafSchema(GUIDED_REQUIRED_FIELD_LIMIT + 1);
  f.state.tools = [tool('complex_operation', schema)];
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('complex_operation');
  assert.deepEqual(JSON.parse(request.getText()), Object.fromEntries(
    Object.keys(schema.properties).map(name => [name, null]),
  ));
  assert.equal(f.state.pickers.some(picker => picker.options.title.endsWith(': request mode')), false);
  assert.equal(f.state.inputs.length, 0);
  assert.deepEqual(f.state.sessions[0].calls, []);
  await f.host.extension.deactivate();
});

test('optional objects stay atomic so required descendants are never presented as independently optional', async () => {
  const f = fixture();
  f.state.tools = [tool('render_sites', {
    type: 'object',
    properties: {
      destination: {
        type: 'object',
        description: 'One optional destination selection.',
        required: ['kind', 'path'],
        properties: {
          kind: { enum: ['local'] },
          path: { type: 'string' },
        },
      },
    },
  })];
  let destinationChosen = false;
  f.state.pick = (items, options) => {
    if (options.title.endsWith(': request mode')) return items.find(item => item.mode === 'guided');
    if (options.title.endsWith(': optional fields')) {
      if (!destinationChosen) {
        destinationChosen = true;
        return items.find(item => item.value?.path.join('.') === 'destination');
      }
      return items.find(item => item.label === 'Review request');
    }
    return items[0];
  };
  f.state.input = options => options.title.includes('destination')
    ? '{"kind":"local","path":"/output"}' : undefined;
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('render_sites');
  assert.deepEqual(JSON.parse(request.getText()), {
    destination: { kind: 'local', path: '/output' },
  });
  const optional = f.state.pickers.find(picker => picker.options.title.endsWith(': optional fields'));
  assert.deepEqual(optional.items.filter(item => item.value).map(item => item.value.path), [['destination']]);
  assert.equal(f.state.sessions[0].calls.length, 0);
  await f.host.extension.deactivate();
});

test('advanced JSON is an explicit escape hatch and retains the complete inert schema projection', async () => {
  const f = fixture();
  f.state.tools = [tool('render_sites', {
    type: 'object',
    properties: {
      recipe: { type: 'object', properties: { configuration: { type: 'string' } } },
      allow_execute: { type: 'boolean' },
    },
  })];
  f.state.pick = (items, options) => options.title.endsWith(': request mode')
    ? items.find(item => item.mode === 'advanced') : items[0];
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('render_sites');
  assert.deepEqual(JSON.parse(request.getText()), {
    recipe: { configuration: null },
    allow_execute: null,
  });
  assert.equal(f.state.inputs.length, 0);
  await f.host.extension.deactivate();
});

test('terminal postcondition is concise and full native receipt opens only through the explicit action', async () => {
  const f = fixture();
  f.state.notice = () => undefined;
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create();
  f.edit(request, '{}');
  assert.equal(await f.run('executeOperationRequest'), undefined);
  assert.equal(f.state.displayed.some(document => document.getText() === f.state.result.raw), false);
  assert.match(f.state.notices.at(-1).message, /completed successfully/);
  assert.equal(f.state.notices.at(-1).action, 'Open Full Receipt');
  const receipt = await f.run('showApplicationResult');
  assert.equal(receipt.getText(), f.state.result.raw);
  assert.equal(receipt.uri.scheme, 'gazette-result');
  assert.equal(receipt.languageId, 'json');
  assert.equal(receipt.isUntitled, false);
  assert.equal(receipt.isDirty, false);
  assert.equal(await f.run('showApplicationResult'), receipt, 'reopening the same receipt reuses its clean document');
  receipt.isClosed = true;
  f.host.close.fire(receipt);
  assert.equal(f.host.contentProviders.get('gazette-result').provideTextDocumentContent(receipt.uri), undefined);
  const reopened = await f.run('showApplicationResult');
  assert.equal(reopened.getText(), f.state.result.raw, 'retained receipt survives closing its view');
  await f.host.extension.deactivate();
});

test('held lifecycle success names its live terminal-result owner without running a follow-up effect', async () => {
  const f = fixture();
  f.state.tools = [
    tool('start_preview', { type: 'object' }, 'preview', {
      'io.rtepl.gazette/terminalResultOwner': 'stop_preview',
      'io.rtepl.gazette/closure': 'terminal-owner',
    }),
    tool('stop_preview', { type: 'object' }, 'preview'),
  ];
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('start_preview');
  f.edit(request, '{}');
  await f.run('executeOperationRequest');
  assert.match(f.state.notices.at(-1).message, /held lifecycle remains active.*stop_preview owns the terminal result/i);
  assert.match(f.state.notices.at(-1).message, /No follow-up effect ran automatically/);
  assert.deepEqual(f.state.sessions[0].calls, [{ name: 'start_preview', argumentsJson: '{}' }]);
  await f.host.extension.deactivate();
});

test('generic discovery narrows capability then intent, presenting common before advanced operations', async () => {
  const f = fixture();
  f.state.tools = [
    tool('check_workspace', { type: 'object' }, 'workspace', {
      'io.rtepl.gazette/intent': 'inspect', 'io.rtepl.gazette/effect': 'read',
      'io.rtepl.gazette/potentialEffects': [],
    }),
    tool('render_sites', { type: 'object' }, 'build', { 'io.rtepl.gazette/intent': 'act' }),
    tool('probe_rendered_site', { type: 'object' }, 'build', {
      'io.rtepl.gazette/intent': 'inspect', 'io.rtepl.gazette/effect': 'read',
      'io.rtepl.gazette/potentialEffects': [],
    }),
    tool('qualify_sites', { type: 'object' }, 'build', {
      'io.rtepl.gazette/intent': 'verify', 'io.rtepl.gazette/effect': 'local',
      'io.rtepl.gazette/potentialEffects': ['process'],
      'io.rtepl.gazette/prominence': 'advanced',
    }),
  ];
  f.state.pick = (items, options) => {
    if (options.title === 'Gazette capability') return items.find(item => item.name === 'build');
    if (options.title === 'Gazette build: purpose') return items.find(item => item.intent === 'verify');
    return items[0];
  };
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create();
  assert.deepEqual(f.state.pickers.map(picker => picker.options.title), [
    'Gazette capability',
    'Gazette build: purpose',
    'build · qualify_sites: request mode',
  ]);
  const purpose = f.state.pickers.find(picker => picker.options.title === 'Gazette build: purpose');
  assert.deepEqual(purpose.items.map(item => item.intent), ['inspect', 'act', 'verify'],
    'intents containing common operations precede advanced-only intent groups');
  f.edit(request, '{}');
  await f.run('executeOperationRequest');
  assert.deepEqual(f.state.sessions[0].calls, [{ name: 'qualify_sites', argumentsJson: '{}' }]);
  await f.host.extension.deactivate();
});

test('successful operations offer canonical relations as inert request preparation without dispatch', async () => {
  const f = fixture();
  f.state.tools = [
    tool('render_sites', { type: 'object' }, 'build', {
      'io.rtepl.gazette/relations': [
        { kind: 'verify', operation: 'qualify_sites' },
        { kind: 'contrast', operation: 'probe_rendered_site' },
      ],
      'io.rtepl.gazette/closure': 'related',
    }),
    tool('qualify_sites', { type: 'object' }, 'build', {
      'io.rtepl.gazette/intent': 'verify', 'io.rtepl.gazette/effect': 'local',
      'io.rtepl.gazette/potentialEffects': ['process'],
    }),
    tool('probe_rendered_site', { type: 'object' }, 'build', {
      'io.rtepl.gazette/intent': 'inspect', 'io.rtepl.gazette/effect': 'read',
      'io.rtepl.gazette/potentialEffects': [],
    }),
  ];
  f.state.notice = actions => actions.find(action => action === 'Prepare Canonical Follow-up…');
  f.state.pick = (items, options) => {
    if (options.title.endsWith(': canonical follow-up')) {
      return items.find(item => item.followup?.tool.name === 'qualify_sites');
    }
    return items[0];
  };
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('render_sites');
  f.edit(request, '{}');
  const related = await f.run('executeOperationRequest');
  const relatedDocument = f.host.vscode.workspace.textDocuments
    .find(document => document.uri.toString() === related.toString());
  assert.equal(relatedDocument.getText(), '{}\n');
  assert.deepEqual(f.state.sessions[0].calls, [{ name: 'render_sites', argumentsJson: '{}' }],
    'choosing a relation prepares but never dispatches its operation');
  assert.deepEqual(f.state.notices.at(-1).actions,
    ['Open Full Receipt', 'Open Build Input…', 'Prepare Canonical Follow-up…']);
  assert.match(f.state.notices.at(-1).message,
    /completed this operation successfully.*Related lifecycle operations remain separately selectable/);
  const picker = f.state.pickers.find(candidate => candidate.options.title.endsWith(': canonical follow-up'));
  assert.deepEqual(picker.items.map(item => [item.followup.kind, item.followup.tool.name]), [
    ['verify', 'qualify_sites'], ['contrast', 'probe_rendered_site'],
  ]);
  assert.ok(f.state.displayed.some(document => document.getText() === f.state.result.raw),
    'native evidence is delivered before preparing the related request');
  await f.host.extension.deactivate();
});

test('completed failures expose only evidence and recovery relations without claiming safety', async () => {
  const f = fixture();
  f.state.tools = [
    tool('render_sites', { type: 'object' }, 'build', {
      'io.rtepl.gazette/relations': [
        { kind: 'inspect', operation: 'probe_rendered_site' },
        { kind: 'verify', operation: 'qualify_sites' },
        { kind: 'recover', operation: 'recover_render' },
      ],
      'io.rtepl.gazette/closure': 'related',
    }),
    tool('probe_rendered_site', { type: 'object' }, 'build', {
      'io.rtepl.gazette/intent': 'inspect', 'io.rtepl.gazette/effect': 'read',
      'io.rtepl.gazette/potentialEffects': [],
    }),
    tool('qualify_sites', { type: 'object' }, 'build', {
      'io.rtepl.gazette/intent': 'verify', 'io.rtepl.gazette/effect': 'local',
      'io.rtepl.gazette/potentialEffects': ['process'],
    }),
    tool('recover_render', { type: 'object' }, 'recovery', {
      'io.rtepl.gazette/intent': 'recover', 'io.rtepl.gazette/effect': 'local',
      'io.rtepl.gazette/potentialEffects': ['memory-state'],
    }),
  ];
  f.state.result = { raw: '{"jsonrpc":"2.0","id":2,"result":{"isError":true}}', isError: true };
  f.state.notice = actions => actions.find(action => action === 'Prepare Evidence/Recovery Follow-up…');
  f.state.pick = (items, options) => options.title.endsWith(': evidence/recovery follow-up')
    ? items.find(item => item.followup?.tool.name === 'recover_render')
    : items[0];
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('render_sites');
  f.edit(request, '{}');
  const related = await f.run('executeOperationRequest');
  const relatedDocument = f.host.vscode.workspace.textDocuments
    .find(document => document.uri.toString() === related.toString());
  assert.equal(relatedDocument.getText(), '{}\n');
  assert.deepEqual(f.state.sessions[0].calls, [{ name: 'render_sites', argumentsJson: '{}' }]);
  assert.deepEqual(f.state.dialogs.at(-1).actions,
    ['Open Full Receipt', 'Prepare Evidence/Recovery Follow-up…']);
  assert.match(f.state.dialogs.at(-1).message, /do not make them safe or authorized/);
  const picker = f.state.pickers.find(candidate => candidate.options.title.endsWith(': evidence/recovery follow-up'));
  assert.deepEqual(picker.items.map(item => [item.followup.kind, item.followup.tool.name]), [
    ['inspect', 'probe_rendered_site'], ['recover', 'recover_render'],
  ]);
  await f.host.extension.deactivate();
});

test('visible failures preserve dispatch certainty without disclosing private details', async () => {
  for (const [effect, expected] of [
    ['not-dispatched', /did not dispatch/],
    ['unknown', /cannot prove.*Do not retry/],
  ]) {
    const f = fixture();
    await f.host.extension.activate({ subscriptions: [] });
    const request = await f.create();
    f.edit(request, '{}');
    f.state.callError = new f.ApplicationError('private failure detail', effect);
    await assert.rejects(f.run('executeOperationRequest'));
    assert.match(f.state.errors.at(-1), expected);
    assert.doesNotMatch(f.state.errors.at(-1), /private failure detail/);
    await f.host.extension.deactivate();
  }
});

test('every CLI family has one registered command and resolves only its live MCP routes', async () => {
  const f = fixture();
  const names = [...new Set(applicationCommandCatalog.flatMap(route => route.tools))];
  f.state.tools = names.map(name => tool(name));
  f.state.catalog = JSON.stringify({ jsonrpc: '2.0', id: 1, result: { tools: f.state.tools } });
  await f.host.extension.activate({ subscriptions: [] });
  const visible = new Set(extensionManifest.contributes.commands.map(command => command.command));
  assert.ok(visible.has('gazette.runCommand'));
  assert.ok(visible.has('gazette.createOperationRequest'));
  assert.equal([...visible].some(command => command.startsWith('gazette.application.')), false,
    'specialist routes stay registered without flooding the Command Palette');
  assert.equal(new Set(applicationCommandCatalog.map(route => route.cli)).size, applicationCommandCatalog.length);
  assert.ok(extensionManifest.contributes.languages.some(language => language.id === 'gazette'),
    'the contributed Gazette language supplies automatic language activation');
  const editorVersion = /^\^(\d+)\.(\d+)\.(\d+)$/.exec(extensionManifest.engines.vscode);
  assert.ok(editorVersion && (Number(editorVersion[1]) > 1 || (Number(editorVersion[1]) === 1 && Number(editorVersion[2]) >= 74)),
    'the selected editor engine supports activation for contributed languages');
  assert.deepEqual(
    new Set(extensionManifest.activationEvents),
    new Set(['workspaceContains:gazette.toml', ...applicationCommandCatalog.map(route => `onCommand:gazette.application.${route.cli}`)]),
    'hidden routes retain cold activation; visible commands and Gazette language activate through their contributions',
  );
  for (const route of applicationCommandCatalog) {
    assert.ok(route.tools.length > 0, `${route.cli} has an operation`);
    assert.equal(new Set(route.tools).size, route.tools.length, `${route.cli} has no duplicate operation`);
    assert.ok(f.host.commands.has(`gazette.application.${route.cli}`), `${route.cli} command registered`);
  }
  const route = applicationCommandCatalog.find(route => route.cli === 'check');
  f.state.pick = items => items.find(item => item.binding?.tool.name === 'inspect_publication') ?? items[0];
  const uri = await f.run('application.check');
  const request = f.host.vscode.workspace.textDocuments.find(document => document.uri.toString() === uri.toString());
  assert.equal(JSON.parse(request.getText()) instanceof Array, false);
  assert.equal(f.state.sessions[0].calls.length, 0, 'a named route still requires explicit reviewed dispatch');
  assert.ok(f.state.pickers.some(picker => picker.options.title === `Gazette ${route.cli} operation`));
  await f.host.extension.deactivate();
});

test('Run Command offers every CLI family and prepares without dispatching', async () => {
  const f = fixture();
  f.state.tools = [...new Set(applicationCommandCatalog.flatMap(route => route.tools))].map(name => tool(name));
  f.state.catalog = JSON.stringify({ jsonrpc: '2.0', id: 1, result: { tools: f.state.tools } });
  await f.host.extension.activate({ subscriptions: [] });
  f.state.pick = items => items.find(item => item.route?.cli === 'check') ?? items[0];
  const uri = await f.run('runCommand');
  assert.ok(uri, 'choosing a command opens its request');
  assert.deepEqual(f.state.pickers[0].items.map(item => item.route.cli), applicationCommandCatalog.map(route => route.cli));
  assert.equal(f.state.sessions[0].calls.length, 0, 'selection alone never executes an operation');
  await f.host.extension.deactivate();
});

test('cancelling Run Command starts no application session', async () => {
  const f = fixture();
  await f.host.extension.activate({ subscriptions: [] });
  f.state.pick = () => undefined;
  assert.equal(await f.run('runCommand'), undefined);
  assert.equal(f.state.sessions.length, 0);
  await f.host.extension.deactivate();
});

test('Run Command accepts a CLI family for keybindings and rejects unknown names before opening a session', async () => {
  const f = fixture();
  const route = applicationCommandCatalog.find(route => route.cli === 'schema');
  f.state.tools = route.tools.map(name => tool(name));
  f.state.catalog = JSON.stringify({ jsonrpc: '2.0', id: 1, result: { tools: f.state.tools } });
  await f.host.extension.activate({ subscriptions: [] });
  await assert.rejects(f.host.commands.get('gazette.runCommand')('missing-command'), /Choose a command/);
  assert.equal(f.state.sessions.length, 0);
  assert.ok(await f.host.commands.get('gazette.runCommand')('schema'));
  assert.equal(f.state.sessions[0].calls.length, 0);
  assert.equal(f.state.pickers.some(picker => picker.options.title === 'Gazette: Run Command'), false);
  await f.host.extension.deactivate();
});

test('schema projection is bounded and refuses to invent unsupported or recursive values', async () => {
  const f = fixture();
  f.state.tools = [tool('native_future_operation', {
    type: 'object',
    properties: {
      external: { $ref: 'https://example.invalid/schema' },
      recursive: { $ref: '#/$defs/node' },
      choice: { oneOf: [{ const: 'a' }, { const: 'b' }] },
    },
    $defs: { node: { type: 'object', properties: { next: { $ref: '#/$defs/node' } } } },
  })];
  f.state.catalog = JSON.stringify({ jsonrpc: '2.0', id: 1, result: { tools: f.state.tools } });
  f.state.pick = (items, options) => options.title.endsWith(': request mode')
    ? items.find(item => item.mode === 'advanced') : items[0];
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('native_future_operation');
  assert.deepEqual(JSON.parse(request.getText()), { external: null, recursive: { next: null }, choice: null });
  assert.deepEqual(f.state.sessions[0].calls, []);
  await f.host.extension.deactivate();
});

test('each prepared request stays bound to its exact folder, session and discovered tool', async () => {
  const f = fixture(['/work/one', '/work/two']);
  await f.host.extension.activate({ subscriptions: [] });
  assert.equal(f.host.clients.length, 2, 'existing language-server activation survives');
  let selected = 'one';
  f.state.pick = items => items.find(item => item.folder?.name === selected) ?? items[0];
  const one = await f.create('native_future_operation');
  selected = 'two';
  f.state.tools = [tool('different_native_operation')];
  f.state.catalog = JSON.stringify({ jsonrpc: '2.0', id: 1, result: { tools: f.state.tools } });
  const two = await f.create('different_native_operation');
  f.edit(one, '{"which":"one"}');
  await f.run('executeOperationRequest');
  f.edit(two, '{"which":"two"}');
  await f.run('executeOperationRequest');
  assert.equal(f.state.sessions.length, 2);
  assert.deepEqual(f.state.sessions.map(session => [session.cwd, session.calls]), [
    ['/work/one', [{ name: 'native_future_operation', argumentsJson: '{"which":"one"}' }]],
    ['/work/two', [{ name: 'different_native_operation', argumentsJson: '{"which":"two"}' }]],
  ]);
  const foreign = await f.host.vscode.workspace.openTextDocument({ language: 'json', content: two.getText() });
  f.host.vscode.window.activeTextEditor = { document: foreign };
  await assert.rejects(f.run('executeOperationRequest'));
  await assert.rejects(f.create('absent_from_native_catalog'));
  assert.equal(f.state.sessions.length, 2);
  await f.host.extension.deactivate();
});

test('cancellation and changed buffer, folder or executable after dialogs prevent dispatch', async () => {
  for (const mutation of ['cancel', 'text', 'version', 'closed', 'active', 'removed', 'executable']) {
    const f = fixture();
    await f.host.extension.activate({ subscriptions: [] });
    const request = await f.create();
    f.edit(request, '{"private":"canary"}');
    f.state.confirm = async () => {
      if (mutation === 'cancel') return undefined;
      if (mutation === 'text') request.text += ' ';
      if (mutation === 'version') request.version++;
      if (mutation === 'closed') request.isClosed = true;
      if (mutation === 'active') f.host.vscode.window.activeTextEditor = undefined;
      if (mutation === 'removed') f.host.vscode.workspace.workspaceFolders = [];
      if (mutation === 'executable') f.host.settings.set(f.host.vscode.workspace.workspaceFolders[0].uri.toString(), { executable: { globalValue: '/other/gazette' } });
      return 'Run';
    };
    if (mutation === 'cancel') assert.equal(await f.run('executeOperationRequest'), undefined);
    else await assert.rejects(f.run('executeOperationRequest'));
    assert.deepEqual(f.state.sessions[0].calls, [], mutation);
    assert.ok(f.state.errors.every(message => !message.includes('canary') && !message.includes('/work')));
    await f.host.extension.deactivate();
  }
});

test('late catalog cannot create a usable request after its folder or executable changes', async () => {
  for (const change of ['folder', 'executable']) {
    const f = fixture();
    await f.host.extension.activate({ subscriptions: [] });
    f.state.discoverGate = deferred();
    const pending = f.create('native_future_operation');
    await f.host.flush();
    const folder = f.host.vscode.workspace.workspaceFolders[0];
    if (change === 'folder') f.host.vscode.workspace.workspaceFolders = [];
    else f.host.settings.set(folder.uri.toString(), { executable: { globalValue: '/changed/gazette' } });
    f.state.discoverGate.resolve();
    await assert.rejects(pending);
    assert.equal(f.state.displayed.length, 0);
    assert.deepEqual(f.state.sessions[0].calls, []);
    await f.host.extension.deactivate();
  }
});

test('native failure and post-dispatch folder change still deliver raw evidence without retry', async () => {
  const f = fixture();
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create();
  f.edit(request, '{"allow_read":false}');
  f.state.result = { raw: '{"jsonrpc":"2.0","id":7,"result":{"content":[{"type":"text","text":"private-native-failure"}],"isError":true}}', isError: true };
  f.state.callGate = deferred();
  const operation = f.run('executeOperationRequest');
  await f.host.flush();
  f.host.vscode.workspace.workspaceFolders = [];
  request.isClosed = true;
  f.state.callGate.resolve();
  const result = await operation;
  assert.equal(result.getText(), f.state.result.raw);
  assert.equal(f.state.sessions[0].calls.length, 1);
  assert.ok(!f.state.errors.some(message => message.includes('private-native-failure')));
  const repeated = await f.run('showApplicationResult');
  assert.equal(repeated.getText(), result.getText());
  assert.equal(f.state.sessions[0].calls.length, 1);
  await f.host.extension.deactivate();
});

test('unknown RPC outcomes retain evidence, forbid retry and name no invented recovery', async () => {
  const f = fixture();
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create();
  f.edit(request, '{}');
  const raw = '{"jsonrpc":"2.0","id":4,"error":{"code":-32602,"message":"private-rpc-canary"}}';
  f.state.callError = new f.ApplicationError('unknown outcome', 'unknown', raw);
  f.state.failDisplay = true;
  await assert.rejects(f.run('executeOperationRequest'));
  assert.equal(f.state.sessions[0].calls.length, 1);
  assert.match(f.state.dialogs.at(-1).message, /unknown outcome.*Do not retry/);
  assert.match(f.state.dialogs.at(-1).message, /No canonical evidence or recovery-related follow-up/);
  assert.deepEqual(f.state.dialogs.at(-1).actions, ['Open Full Receipt']);
  await assert.rejects(f.run('executeOperationRequest'));
  await assert.rejects(f.run('stopApplicationSession'));
  assert.equal(f.state.sessions[0].calls.length, 1);
  assert.equal(f.state.sessions[0].stops, 0);
  f.state.failDisplay = false;
  const result = await f.run('showApplicationResult');
  assert.equal(result.getText(), raw);
  assert.ok(f.state.errors.every(message => !message.includes('private-rpc-canary')));
  await f.run('stopApplicationSession');
  assert.equal(f.state.sessions[0].closed, true);
  f.edit(request, '{}');
  await assert.rejects(f.run('executeOperationRequest'), 'retired request URI cannot migrate to a new process');
  await f.host.extension.deactivate();
});

test('active or uncertain cleanup keeps process custody; clean explicit stop alone admits replacement', async () => {
  const f = fixture();
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create();
  f.edit(request, '{}');
  f.state.callGate = deferred();
  const active = f.run('executeOperationRequest');
  await f.host.flush();
  await assert.rejects(f.run('stopApplicationSession'));
  assert.equal(f.state.sessions[0].closed, false);
  f.state.callGate.resolve();
  await active;
  f.state.failStop = true;
  await assert.rejects(f.run('stopApplicationSession'));
  await f.create();
  assert.equal(f.state.sessions.length, 1, 'cleanup failure never creates a replacement');
  f.state.failStop = false;
  await f.run('stopApplicationSession');
  await f.create();
  assert.equal(f.state.sessions.length, 2);
  assert.equal(f.state.sessions[0].closed, true);
  f.edit(request, '{}');
  await assert.rejects(f.run('executeOperationRequest'));
  assert.equal(f.state.sessions[1].calls.length, 0);
  await f.host.extension.deactivate();
});

test('trust and machine settings guard spawning, and configuration changes require explicit retirement', async () => {
  for (const setting of ['untrusted', 'workspace']) {
    const f = fixture();
    await f.host.extension.activate({ subscriptions: [] });
    const folder = f.host.vscode.workspace.workspaceFolders[0];
    if (setting === 'untrusted') f.host.vscode.workspace.isTrusted = false;
    else f.host.settings.set(folder.uri.toString(), { executable: { globalValue: '/exact/gazette', workspaceValue: '/private/override' } });
    await assert.rejects(f.create());
    assert.equal(f.state.sessions.length, 0);
    await f.host.extension.deactivate();
  }
  const f = fixture();
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create();
  const folder = f.host.vscode.workspace.workspaceFolders[0];
  f.host.settings.set(folder.uri.toString(), { executable: { globalValue: '/new/gazette' } });
  f.host.configuration.fire({ affectsConfiguration: name => name === 'gazette.server.path' });
  await f.host.flush();
  await assert.rejects(f.create());
  f.edit(request, '{}');
  await assert.rejects(f.run('executeOperationRequest'));
  assert.equal(f.state.sessions.length, 1);
  await f.run('stopApplicationSession');
  await f.create();
  assert.equal(f.state.sessions[1].executable, '/new/gazette');
  await f.host.extension.deactivate();
});


test('guided structured input reaches the request editor without numeric or key normalization', async () => {
  const f = fixture();
  f.state.tools = [tool('render_sites', { type: 'object', required: ['payload'], properties: { payload: { type: 'object' } } })];
  const exact = '{"large":9007199254740993,"overflow":1e999,"duplicate":1,"duplicate":2}';
  f.state.pick = (items, options) => options.title.endsWith(': request mode') ? items.find(item => item.mode === 'guided') : items[0];
  f.state.input = () => exact;
  await f.host.extension.activate({ subscriptions: [] });
  const request = await f.create('render_sites');
  assert.equal(request.getText().trim(), `{"payload":${exact}}`);
  assert.equal(f.state.sessions[0].calls.length, 0);
  await f.host.extension.deactivate();
});

test('cancelling an in-flight editor import closes its handle without querying or opening a result tab', async () => {
  const {host, state} = fixture();
  const {ApplicationCommands} = require('../dist/applicationCommands.js');
  const commands = new ApplicationCommands(host.vscode, {subscriptions: []});
  const folder = host.vscode.workspace.workspaceFolders[0];
  const id = 'file:///work/one/page.md';
  const admitted = deferred();
  const imported = deferred();
  const cancellation = {isCancellationRequested:false};
  state.callResult = async name => {
    if (name === 'authoring_import_editor_context') {
      admitted.resolve();
      await imported.promise;
    }
    return {raw: JSON.stringify({result:{structuredContent:{session:'temporary',generation:2}}}), isError:false};
  };
  const inspected = commands.inspectEditor(folder, {
    generation:2,
    manifest:{generation:2,sources:[{id,bytes:4,version:1,source_sha256:'proof',role:{kind:'document'}}]},
    start:{}, overlays:[], current:async()=>true, nativeSelection:{root:'/work/one'},
  }, id, 0, 'authoring_diagnostics', cancellation);
  await admitted.promise;
  const arguments_ = JSON.parse(state.sessions[0].calls[0].argumentsJson);
  assert.equal(arguments_.allow_native_reads, true);
  assert.equal(arguments_.context.format, 'gazette-editor-context/2');
  assert.equal(arguments_.context.allow_native_reads, undefined);
  cancellation.isCancellationRequested = true;
  imported.resolve();
  assert.equal(await inspected, undefined);
  assert.deepEqual(state.sessions[0].calls.map(call=>call.name),
    ['authoring_import_editor_context','authoring_close_session']);
  assert.equal(state.displayed.length, 0);
  await commands.shutdown();
});
