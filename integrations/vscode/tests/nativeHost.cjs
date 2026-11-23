// Run through an isolated VS Code extension-test host with the native discovery protocol.
const assert = require('node:assert/strict');
const {readFile, writeFile} = require('node:fs/promises');
const {basename, isAbsolute, join} = require('node:path');
const {execFileSync} = require('node:child_process');
const vscode = require('vscode');

async function eventually(label, inspect) {
  const deadline = Date.now() + 30000;
  do {
    const value = await inspect();
    if (value) return value;
    await new Promise(resolve => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  throw new Error(`Real native editor did not reach: ${label}`);
}

async function replace(document, text) {
  const edit = new vscode.WorkspaceEdit();
  edit.replace(document.uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text);
  assert.ok(await vscode.workspace.applyEdit(edit));
}

function hasSymbol(symbols, name) {
  return symbols?.some(symbol => symbol.name === name || hasSymbol(symbol.children, name)) ?? false;
}

function nativeProcess(executable) {
  const rows = execFileSync('ps', ['-eo', 'pid,command'], {encoding: 'utf8'})
    .split('\n').filter(line => line.includes(`${executable} lsp`));
  assert.equal(rows.length, 1, 'one independently selected native language-server process');
  return rows[0].trim().split(/\s+/)[0];
}

async function run() {
  assert.equal(process.env.GAZETTE_VSCODE_ISOLATED, '1');
  const executable = process.env.GAZETTE_TEST_EXECUTABLE;
  assert.ok(executable && isAbsolute(executable));
  const folders = vscode.workspace.workspaceFolders;
  assert.equal(folders?.length, 1, 'native discovery opens one configuration folder');
  const folder = folders[0];
  assert.equal(folder.uri.scheme, 'file');
  const starter = basename(folder.uri.fsPath) === 'native-editor-starter';
  assert.ok(starter || process.env.GAZETTE_VSCODE_READ_ONLY_WORKSPACE === folder.uri.fsPath,
    'source writes are restricted to the generated disposable starter');
  assert.ok(vscode.workspace.isTrusted, 'select trust for this isolated host before its test');
  const bundled = process.env.GAZETTE_VSCODE_BUNDLED === '1';
  if (bundled) {
    assert.equal(vscode.workspace.getConfiguration('gazette').get('server.path', ''), '',
      'first use needs no executable setting');
  } else {
    await vscode.workspace.getConfiguration('gazette').update('server.path', executable, vscode.ConfigurationTarget.Global);
  }
  const settings = vscode.workspace.getConfiguration('gazette', folder.uri);
  assert.equal(settings.get('workspace.discovery', true), true);
  for (const key of ['roots', 'configurationDocuments', 'documents', 'manuals', 'pagePackageInputs', 'packageBaselines']) {
    assert.deepEqual(settings.get(`workspace.${key}`, []), [], `no legacy ${key} list`);
  }
  const extension = vscode.extensions.getExtension('rtepl.gazette-vscode');
  assert.ok(extension);
  if (process.env.GAZETTE_VSCODE_INSTALLED_EXTENSION) {
    assert.equal(extension.extensionPath, process.env.GAZETTE_VSCODE_INSTALLED_EXTENSION,
      'the installed VSIX supplies Gazette, independently of the test harness');
  }
  await extension.activate();
  if (!bundled) await vscode.commands.executeCommand('gazette.restartLanguageServer');
  const rootPath = join(folder.uri.fsPath, settings.get('workspace.configuration', 'gazette.toml'));
  const originalRoot = await readFile(rootPath, 'utf8');
  let root = await vscode.workspace.openTextDocument(vscode.Uri.file(rootPath));
  await vscode.window.showTextDocument(root);
  // A root containing only includes and build choices has no declarations to outline.
  await eventually('root selected for Gazette authoring', () => {
    const selected = vscode.workspace.textDocuments.find(document => !document.isClosed
      && document.uri.toString() === root.uri.toString() && document.languageId === 'gazette');
    if (selected) root = selected;
    return selected;
  });
  const pid = nativeProcess(executable);
  const selectedPage = starter ? join(folder.uri.fsPath, 'content/keep-the-context/index.md') : process.env.GAZETTE_VSCODE_NATIVE_PAGE;
  assert.ok(selectedPage && isAbsolute(selectedPage));
  const originalPage = await readFile(selectedPage, 'utf8');
  let page = await vscode.workspace.openTextDocument(vscode.Uri.file(selectedPage));
  // Prove Gazette's file-based provider, without the built-in Markdown outline.
  page = await vscode.languages.setTextDocumentLanguage(page, 'plaintext');
  await vscode.window.showTextDocument(page);
  const heading = 'Native editor unsaved observation';
  await replace(page, originalPage + `\n## ${heading}\n`);
  await eventually('unsaved page symbols', async () => {
    const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', page.uri);
    return hasSymbol(symbols, heading);
  });
  if (starter) {
    const field = page.positionAt(page.getText().indexOf('collections') + 2);
    await eventually('native frontmatter hover', async () => {
      const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', page.uri, field);
      return hovers?.some(hover => hover.contents.some(content =>
        /collections/i.test(typeof content === 'string' ? content : content.value)));
    });
    const reference = page.positionAt(page.getText().indexOf('site:field-notes.example/review-a-note/') + 8);
    await eventually('native publication definition', async () => {
      const locations = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', page.uri, reference);
      return locations?.some(location => (location.targetUri ?? location.uri)?.fsPath
        === join(folder.uri.fsPath, 'content/review-a-note/index.md'));
    });
  }
  await replace(root, '[[principals\n');
  await eventually('configuration typo diagnostic', () => vscode.languages.getDiagnostics(root.uri).some(value => value.source === 'gazette' && value.code === 'toml_invalid'));
  await replace(root, originalRoot);
  await eventually('configuration repair', () => vscode.languages.getDiagnostics(root.uri).every(value => value.code !== 'toml_invalid'));
  await eventually('page overlay survives configuration repair', async () => {
    const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', page.uri);
    return hasSymbol(symbols, heading);
  });
  assert.equal(nativeProcess(executable), pid, 'configuration repair keeps the same server');
  assert.equal(await readFile(rootPath, 'utf8'), originalRoot);
  assert.equal(await readFile(selectedPage, 'utf8'), originalPage);
  if (starter) {
    const path = join(folder.uri.fsPath, 'content/new-observation.md');
    const text = "+++\ncollections=['collection:missing']\n[content]\nid='content:new-observation'\nsite='site:field-notes'\ntitle='New observation'\nlifecycle='draft'\nstructure={kind='page',family={kind='article'}}\n+++\n# New observation\n";
    await writeFile(path, text, {flag: 'wx'});
    const added = await vscode.workspace.openTextDocument(vscode.Uri.file(path));
    await vscode.window.showTextDocument(added);
    await eventually('new page diagnostics on first open', () => vscode.languages.getDiagnostics(added.uri).some(value => value.source === 'gazette' && value.data?.target === 'collection:missing'));
    const at = added.positionAt(added.getText().indexOf('collection:missing'));
    const choices = await eventually('native collection completion', async () => {
      const result = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', added.uri, at);
      return result?.items.some(item => item.label === 'content:field-notes') && result;
    });
    assert.ok(choices.items.find(item => item.label === 'content:field-notes').filterText,
      'the real completion has text the suggestion list can match');
    assert.equal(nativeProcess(executable), pid, 'new page first-open keeps the same server');
  }
  await replace(page, originalPage);
  await vscode.window.showTextDocument(page);
  await vscode.commands.executeCommand('workbench.action.revertAndCloseActiveEditor');
  await vscode.window.showTextDocument(root);
  await vscode.commands.executeCommand('workbench.action.revertAndCloseActiveEditor');
  assert.equal(await readFile(rootPath, 'utf8'), originalRoot);
  assert.equal(await readFile(selectedPage, 'utf8'), originalPage);
  console.log(`Gazette real native discovery proof: ${starter ? 'starter and first-open page' : 'selected existing workspace'}, one folder, unsaved configuration repair, preserved page overlay, unchanged source bytes and one server process.`);
}

module.exports = {run: async () => {
  try { await run(); }
  catch (error) { console.error('Gazette native host fixture failed:', error?.stack ?? error); throw error; }
}};
