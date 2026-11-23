const {test} = require('node:test');
const assert = require('node:assert/strict');
const {createEditor, Uri} = require('./support/editor.cjs');

test('Gazette status stays hidden without a selected page and disappears when leaving one', async () => {
  const host = createEditor();
  let visible = false;
  host.vscode.window.createStatusBarItem = () => ({show() {visible = true;}, hide() {visible = false;}, dispose() {}});
  const {AuthoringView} = require('../dist/authoringView');
  const view = new AuthoringView(uri => uri.path === '/work/one/page.md'
    ? {context: async () => {throw new Error('Page needs attention');}} : undefined, {});
  try {
    await view.refresh();
    assert.equal(visible, false);
    assert.equal(host.contextKeys.get('gazette.activeSource'), false);
    host.vscode.window.activeTextEditor = {document: {uri: new Uri('/work/one/page.md')}, selection: {active: {}}};
    await view.refresh();
    assert.equal(visible, true, 'a selected page can show a problem');
    assert.equal(host.contextKeys.get('gazette.activeSource'), true);
    host.vscode.window.activeTextEditor = {document: {uri: new Uri('/client/unrelated.md')}};
    host.active.fire();
    assert.equal(visible, false, 'switching away hides status immediately');
    assert.equal(host.contextKeys.get('gazette.activeSource'), false);
    const manifest = require('../package.json');
    const action = manifest.contributes.menus['editor/title'].find(item => item.command === 'gazette.showAuthoringContext');
    assert.equal(action.when, 'gazette.activeSource');
    await view.refresh();
    assert.equal(visible, false);
    assert.doesNotMatch(view.getChildren()[0].label, /error|not in|unavailable/i);
  } finally {view.dispose();}
});

test('export and inspection share one capture and cancellation releases it', async () => {
  const host = createEditor();
  host.vscode.Uri.file = value => new Uri(value);
  host.vscode.window.activeTextEditor = {document: {uri: new Uri('/work/one/guide.md')}};
  let finishDialog;
  let dialogs = 0;
  let inspections = 0;
  host.vscode.window.showSaveDialog = () => {
    dialogs++;
    return new Promise(resolve => {finishDialog = resolve;});
  };
  const {AuthoringView} = require('../dist/authoringView');
  const view = new AuthoringView(() => ({}), {chooseEditorInspection: async () => {inspections++;}});
  try {
    const first = host.commands.get('gazette.exportEditorContext')();
    await host.commands.get('gazette.exportEditorContext')();
    await host.commands.get('gazette.inspectEditorWithMcp')();
    assert.equal(dialogs, 1);
    assert.equal(inspections, 0);
    finishDialog(undefined);
    await first;
    await host.commands.get('gazette.inspectEditorWithMcp')();
    assert.equal(inspections, 1, 'a cancelled export permits another operation');
    const retry = host.commands.get('gazette.exportEditorContext')();
    assert.equal(dialogs, 2, 'a cancelled inspection also releases the guard');
    finishDialog(undefined);
    await retry;
  } finally {finishDialog?.(undefined); view.dispose();}
});

test('status-bar cancellation stops inspection before MCP dispatch and releases the operation', async () => {
  const host = createEditor();
  host.vscode.Position = class {};
  host.vscode.Range = class {};
  host.vscode.CancellationError = class extends Error {};
  host.vscode.CancellationTokenSource = class {
    token = {isCancellationRequested: false};
    cancel() {this.token.isCancellationRequested = true;}
    dispose() {}
  };
  host.vscode.ProgressLocation = {Window: 2};
  host.vscode.window.withProgress = async (options, operation) => {
    assert.equal(options.location, 2);
    return operation({report() {}});
  };
  host.vscode.window.activeTextEditor = {document: {uri: new Uri('/work/one/page.md'), getText: () => ''}, selection: {active: {}}};
  let resume;
  let queries = 0;
  let choices = 0;
  const session = {snapshot: async (_document, progress) => {
    await new Promise(resolve => {resume = resolve;});
    progress(1, 1);
  }};
  const {AuthoringView} = require('../dist/authoringView');
  const view = new AuthoringView(() => session, {
    chooseEditorInspection: async () => ++choices === 1 ? 'authoring_diagnostics' : undefined,
    inspectEditor: async () => {queries++;},
  });
  try {
    const operation = host.commands.get('gazette.inspectEditorWithMcp')();
    await host.flush();
    host.commands.get('gazette.cancelEditorInspection')();
    resume();
    await operation;
    assert.equal(queries, 0);
    assert.deepEqual(host.messages, []);
    await host.commands.get('gazette.inspectEditorWithMcp')();
    assert.equal(choices, 2);
  } finally {resume?.(); view.dispose();}
});

for (const native of [false, true]) test(`completed ${native ? 'native' : 'supplied'} exports close progress before an unattended success notification`, async () => {
  const host = createEditor();
  const {mkdtemp, readFile, rm} = require('node:fs/promises');
  const {tmpdir} = require('node:os');
  const {join} = require('node:path');
  const directory = await mkdtemp(join(tmpdir(), 'gazette-export-progress-'));
  const destination = new Uri(join(directory, 'context.json'));
  let finishToast;
  let progressOpen = false;
  let notificationOutsideProgress = false;
  host.vscode.Uri.file = value => new Uri(value);
  host.vscode.Position = class {constructor(line, character) {Object.assign(this, {line, character});}};
  host.vscode.Range = class {};
  host.vscode.ProgressLocation = {Notification: 1, Window: 2};
  const document = {uri: new Uri('/work/one/guide.md'), getText: () => ''};
  const snapshot = {generation: 4, manifest: {sources: [{id: document.uri.toString(), role: {kind: 'document'}, bytes: 0, version: 1, source_sha256: 'exact'}]}, start: {}, overlays: [], current: async () => true};
  if (native) snapshot.nativeSelection = {root: '/work/one', workspace: 'gazette.toml', manuals: [], configuration: {}};
  host.vscode.window.activeTextEditor = {document, selection: {active: {line: 0, character: 0}}};
  host.vscode.window.showSaveDialog = async () => destination;
  host.vscode.window.showErrorMessage = async message => {throw new Error(message);};
  host.vscode.window.showInformationMessage = message => {
    if (native) assert.match(message, /0 unsaved documents.*permission to read the saved workspace/);
    else assert.match(message, /selected source text and retained assets/);
    notificationOutsideProgress = !progressOpen;
    return new Promise(resolve => {finishToast = resolve;});
  };
  host.vscode.window.withProgress = async (options, operation) => {
    assert.equal(options.location, host.vscode.ProgressLocation.Window);
    progressOpen = true;
    try {return await operation({report() {}});} finally {progressOpen = false;}
  };
  const {AuthoringView} = require('../dist/authoringView');
  const view = new AuthoringView(() => ({folder: host.folder('/work/one'), snapshot: async () => snapshot}), {});
  try {
    let finished = false;
    const operation = host.commands.get('gazette.exportEditorContext')().then(value => {finished = true; return value;});
    for (let i = 0; i < 100 && !finishToast; i++) await new Promise(resolve => setImmediate(resolve));
    assert.ok(finishToast, 'success notification was presented');
    assert.equal(progressOpen, false, 'the completed write no longer reports progress');
    assert.equal(notificationOutsideProgress, true);
    assert.equal(finished, true, 'export result does not depend on dismissing a toast');
    assert.equal(await operation, destination);
    const exported = JSON.parse(await readFile(destination.fsPath, 'utf8'));
    assert.equal(exported.generation, 4);
    assert.equal(exported.format, `gazette-editor-context/${native ? 2 : 1}`);
    assert.equal(exported.allow_native_reads, undefined, 'the export never grants its own permission');
    if (native) {
      assert.equal(exported.start, undefined);
      assert.deepEqual(exported.native_selection, snapshot.nativeSelection);
    }
  } finally {finishToast?.(); view.dispose(); await rm(directory, {recursive: true, force: true});}
});

test('page context keeps ownership and navigable companion roles without manufacturing diagnostics or paths', () => {
  createEditor();
  const {pageContext} = require('../dist/authoringView');
  const owner = new Uri('/work/guide/notes.md');
  const page = pageContext({status:'available', value:{content:'content:guide',role:{kind:'companion'},
    package:{document:'selected/guide/index.md', companions:[{relative:'notes.md',path:'selected/guide/notes.md',kind:'notes',source_sha256:'exact'}]},resources:[]}}, owner);
  assert.equal(page.description, '1 companion');
  assert.equal(page.children.find(i=>i.label==='This file').value,'Companion of this page');
  assert.deepEqual(page.children.find(i=>i.label==='Page source').command.arguments,[owner,'selected/guide/index.md']);
  assert.equal(page.children.find(i=>i.label==='notes.md').description,'notes');
  assert.equal(page.children.find(i=>i.label==='notes.md').children[0].value,'exact');
  const unavailable = pageContext({status:'unavailable_for_current_source'},owner);
  assert.match(unavailable.children[0].label,/Unavailable/);
  assert.equal(unavailable.children[0].command,undefined);
});

test('editor inspection summarises clean results and links only exact diagnostic source ranges', () => {
  const host = createEditor();
  host.vscode.Range = class { constructor(start, end) { this.start = start; this.end = end; } };
  const {inspectionContext} = require('../dist/authoringView');
  const {createHash} = require('node:crypto');
  const text = 'éx';
  const digest = createHash('sha256').update(text).digest('hex');
  const uri = new Uri('/work/guide/index.md');
  const document = {uri, getText: () => text, positionAt: character => ({line:0, character})};
  const receipt = {editor_generation:7, document:uri.toString(), offset:0, tool:'authoring_diagnostics',
    sources:[{id:uri.toString(),path:'guide/index.md',source_sha256:digest}], result:{value:[]}};
  const clean = inspectionContext(receipt, document);
  assert.match(clean.label, /0 Gazette diagnostics/);
  assert.equal(clean.children[0].command.command, 'gazette.exportEditorInspection');
  const row = {message:'Unknown publisher',code:'unknown_publisher',field:'publisher',
    path:'guide/index.md',source_sha256:digest,location:{span:{start:2,end:3}}};
  receipt.result.value = [row, {...row,path:'another/index.md'}, {...row,location:{span:{start:1,end:3}}}];
  const findings = inspectionContext(receipt, document);
  assert.match(findings.label, /3 Gazette diagnostics/);
  assert.equal(findings.children[0].description, 'unknown_publisher · publisher');
  assert.equal(findings.children[0].uri, uri);
  assert.equal(findings.children[0].range.start.character, 1);
  assert.equal(findings.children[1].uri, undefined);
  assert.equal(findings.children[2].uri, undefined);
});
