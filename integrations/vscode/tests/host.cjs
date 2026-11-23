// Invoke only through an isolated local VS Code extension-test host.
const assert = require('node:assert/strict');
const { mkdir, readFile, writeFile } = require('node:fs/promises');
const { basename, isAbsolute, join } = require('node:path');
const { createHash } = require('node:crypto');
const vscode = require('vscode');

function diagnostics(uri, predicate) {
  return new Promise((resolve, reject) => {
    let subscription;
    const timer = setTimeout(() => {
      subscription?.dispose();
      reject(new Error('Expected real Gazette diagnostics did not arrive'));
    }, 10000);
    const inspect = () => {
      const values = vscode.languages.getDiagnostics(uri);
      if (predicate(values)) {
        clearTimeout(timer);
        subscription?.dispose();
        resolve(values);
      }
    };
    subscription = vscode.languages.onDidChangeDiagnostics(inspect);
    inspect();
  });
}

function folderChange(predicate) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      subscription.dispose();
      reject(new Error('Expected isolated workspace-folder change did not arrive'));
    }, 10000);
    const subscription = vscode.workspace.onDidChangeWorkspaceFolders(event => {
      if (predicate(event)) {
        clearTimeout(timer); subscription.dispose(); resolve();
      }
    });
  });
}

async function distributed(folder) {
  const first = join(folder.uri.fsPath, 'owner-one');
  const second = join(folder.uri.fsPath, 'owner-two');
  await mkdir(first);
  await mkdir(second);
  const root = "includes=['distributed-site.toml']\r\nprincipals=[{id='owner:one',kind='person',name='é🦀'}]\r\n";
  await writeFile(join(folder.uri.fsPath, 'distributed-root.toml'), root, { flag: 'wx' });
  await writeFile(join(folder.uri.fsPath, 'distributed-site.toml'), "sites=[{id='site:one',publisher='owner:one',deployable=false}]\r\n", { flag: 'wx' });
  const source = (id, edges, structure) => `+++\r\n${edges}[content]\r\nid='${id}'\r\nsite='site:one'\r\ntitle='Guide 🦀'\r\nlifecycle='draft'\r\nstructure=${structure}\r\n+++\r\n# Héllo 😀\r\n`;
  const collection = source('collection:guide', '', "{kind='collection'}") + '\r\n# Cross-root target\r\n';
  const article = source('content:article', "collections=['collection:guide']\r\n", "{kind='page',family={kind='article'}}") + '\r\n[Guide](guide.md#cross-root-target)\r\n';
  const collectionPath = join(first, 'guide.md');
  const articlePath = join(second, 'article.md');
  await writeFile(collectionPath, collection, { flag: 'wx' });
  await writeFile(articlePath, article, { flag: 'wx' });
  const documents = [
    { root: 'one', file: 'guide.md', path: 'content/guide.md' },
    { root: 'two', file: 'article.md', path: 'content/article.md' },
  ];
  for (let index = 0; index < 198; index++) {
    const owner = index % 2 === 0 ? first : second;
    const file = `page-${index}.md`;
    let text = source(`content:page-${index}`, '', "{kind='page',family={kind='article'}}");
    if (index === 0) text += 'body '.repeat(15_000);
    await writeFile(join(owner, file), text, { flag: 'wx' });
    documents.push({ root: index % 2 === 0 ? 'one' : 'two', file, path: `content/${file}` });
  }
  const changed = folderChange(event => event.added.some(value => value.uri.toString() === vscode.Uri.file(second).toString()));
  assert.ok(vscode.workspace.updateWorkspaceFolders(vscode.workspace.workspaceFolders.length, 0,
    { uri: vscode.Uri.file(first), name: 'Gazette owner one' }, { uri: vscode.Uri.file(second), name: 'Gazette owner two' }));
  await changed;
  const settings = vscode.workspace.getConfiguration('gazette', folder.uri);
  await settings.update('workspace.configuration', 'distributed-root.toml', vscode.ConfigurationTarget.WorkspaceFolder);
  await settings.update('workspace.configurationDocuments', ['distributed-site.toml'], vscode.ConfigurationTarget.WorkspaceFolder);
  await settings.update('workspace.roots', [{ id: 'one', uri: vscode.Uri.file(first).toString() }, { id: 'two', uri: vscode.Uri.file(second).toString() }], vscode.ConfigurationTarget.WorkspaceFolder);
  await settings.update('workspace.documents', documents, vscode.ConfigurationTarget.WorkspaceFolder);
  await vscode.commands.executeCommand('gazette.restartLanguageServer');
  let page = await vscode.workspace.openTextDocument(vscode.Uri.file(articlePath));
  page = await vscode.languages.setTextDocumentLanguage(page, 'plaintext');
  await vscode.window.showTextDocument(page);
  const at = page.positionAt(page.getText().indexOf('collection:guide'));
  const definitions = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', page.uri, at);
  assert.ok(definitions?.some(value => value.uri.toString() === vscode.Uri.file(collectionPath).toString()), 'real native cross-root membership definition uses the exact owner URI');
  const nativeLink = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', page.uri, page.positionAt(page.getText().indexOf('[Guide]')));
  assert.ok(nativeLink?.some(value => value.uri.toString() === vscode.Uri.file(collectionPath).toString()), 'native Markdown links remain a separate cross-root proof');
  const invalidMembership = new vscode.WorkspaceEdit();
  const scalar = page.getText().indexOf("'collection:guide'");
  invalidMembership.replace(page.uri, new vscode.Range(page.positionAt(scalar), page.positionAt(scalar + "'collection:guide'".length)), "'collection:unknown'");
  assert.ok(await vscode.workspace.applyEdit(invalidMembership));
  const broken = await diagnostics(page.uri, values => values.some(value => value.source === 'gazette' && value.data?.target === 'collection:unknown'));
  const capturedVersion = page.version;
  const capturedDigest = createHash('sha256').update(page.getText()).digest('hex');
  assert.equal(broken.find(value => value.data?.target === 'collection:unknown').data.source_sha256, capturedDigest);
  const proposals = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', page.uri, page.positionAt(scalar));
  // The public host command reconstitutes CompletionItem's declared fields, not
  // LSP data. Raw source-bound completion data is independently proved by the
  // initialized native LSP/MCP tests; this host proof uses the actual public edit.
  const choices = proposals?.items.filter(item => item.kind === vscode.CompletionItemKind.Reference);
  assert.deepEqual(choices?.map(item => item.label), ['collection:guide'], 'article declarations cannot enter the native collection namespace');
  const choice = choices?.find(item => item.label === 'collection:guide');
  assert.ok(choice, 'real native editorial repair selects the same-site collection');
  assert.equal(page.getText(choice.range), "'collection:unknown'");
  assert.equal(page.offsetAt(choice.range.start), scalar);
  assert.equal(page.offsetAt(choice.range.end), scalar + "'collection:unknown'".length);
  assert.equal(choice.insertText, "'collection:guide'");
  assert.equal(page.version, capturedVersion);
  assert.equal(createHash('sha256').update(page.getText()).digest('hex'), capturedDigest);
  const reviewed = new vscode.WorkspaceEdit();
  reviewed.replace(page.uri, choice.range, choice.insertText);
  assert.ok(await vscode.workspace.applyEdit(reviewed));
  await diagnostics(page.uri, values => values.every(value => value.severity !== vscode.DiagnosticSeverity.Error));
  assert.equal(await readFile(articlePath, 'utf8'), article, 'reviewed editorial replacement stays in the unsaved editor buffer');
  let guide = await vscode.workspace.openTextDocument(vscode.Uri.file(collectionPath));
  guide = await vscode.languages.setTextDocumentLanguage(guide, 'plaintext');
  await vscode.window.showTextDocument(guide);
  const references = await vscode.commands.executeCommand('vscode.executeReferenceProvider', guide.uri, guide.positionAt(guide.getText().indexOf('collection:guide')));
  assert.ok(references?.some(value => value.uri.toString() === page.uri.toString()), 'real membership reverse references retain the exact foreign source URI');
  const edit = new vscode.WorkspaceEdit();
  edit.replace(guide.uri, new vscode.Range(guide.positionAt(0), guide.positionAt(guide.getText().length)), collection.replace('collection:guide', 'collection:changed').replace('Cross-root target', 'Changed target'));
  assert.ok(await vscode.workspace.applyEdit(edit));
  const invalid = await diagnostics(page.uri, values => values.some(value => value.source === 'gazette' && value.data?.target === 'collection:guide'));
  assert.ok(invalid.some(value => value.data?.source_sha256), 'native dependent diagnostic retains exact source digest');
  assert.equal(await readFile(collectionPath, 'utf8'), collection, 'foreign-root overlay never saves native source');
  await vscode.commands.executeCommand('workbench.action.revertAndCloseActiveEditor');
  await diagnostics(page.uri, values => values.every(value => value.severity !== vscode.DiagnosticSeverity.Error));
  const recovered = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', page.uri, at);
  assert.ok(recovered?.some(value => value.uri.toString() === vscode.Uri.file(collectionPath).toString()));
  const removed = folderChange(event => event.removed.some(value => value.uri.toString() === vscode.Uri.file(first).toString()));
  const owner = vscode.workspace.workspaceFolders.find(value => value.uri.toString() === vscode.Uri.file(first).toString());
  assert.ok(owner && vscode.workspace.updateWorkspaceFolders(owner.index, 1));
  await removed;
  // Empty diagnostics were already the valid baseline; they cannot prove that
  // native folder teardown and provider removal have finished. The public
  // restart command joins that actual transition and still refuses a missing
  // explicit owner root rather than recapturing a smaller composition.
  await vscode.commands.executeCommand('gazette.restartLanguageServer');
  await diagnostics(page.uri, values => values.every(value => value.source !== 'gazette'));
  const retired = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', page.uri);
  assert.ok(!retired?.length, `removing an explicitly selected root retires the whole composition, not a silent subset: ${JSON.stringify(retired)}`);
  assert.equal(await readFile(articlePath, 'utf8'), article);
  const remaining = vscode.workspace.workspaceFolders.find(value => value.uri.toString() === vscode.Uri.file(second).toString());
  const cleanup = folderChange(event => event.removed.some(value => value.uri.toString() === vscode.Uri.file(second).toString()));
  assert.ok(remaining && vscode.workspace.updateWorkspaceFolders(remaining.index, 1));
  await cleanup;
  await vscode.commands.executeCommand('gazette.restartLanguageServer');
  await settings.update('workspace.roots', [], vscode.ConfigurationTarget.WorkspaceFolder);
  await settings.update('workspace.configurationDocuments', [], vscode.ConfigurationTarget.WorkspaceFolder);
  await settings.update('workspace.configuration', 'explicit-buffer.toml', vscode.ConfigurationTarget.WorkspaceFolder);
  await settings.update('workspace.documents', ['selected-guide.md'], vscode.ConfigurationTarget.WorkspaceFolder);
  await vscode.commands.executeCommand('gazette.restartLanguageServer');
  console.log('Gazette real distributed editor proof: 200 exact sources, three selected roots, modular configuration, >64KiB authored page, cross-root definition/unsaved dependency digest/restoration and whole-composition root retirement.');
}

async function run() {
  assert.equal(process.env.GAZETTE_VSCODE_ISOLATED, '1', 'refuse a non-isolated editor fixture');
  const executable = process.env.GAZETTE_TEST_EXECUTABLE;
  assert.ok(executable && isAbsolute(executable), 'exact independently built Gazette required');
  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder && folder.uri.scheme === 'file');
  assert.ok(basename(folder.uri.fsPath).startsWith('gazette-vscode-proof.'),
    'fixture writes stay in its explicitly selected disposable workspace');
  const sentinelUri = vscode.Uri.file(join(folder.uri.fsPath, 'host-sentinel'));
  assert.ok(vscode.workspace.workspaceFile?.scheme === 'file'
    && vscode.workspace.workspaceFolders.length === 2
    && vscode.workspace.workspaceFolders[1].uri.toString() === sentinelUri.toString(),
    'launch a preselected multi-root workspace with the disposable configuration root first and its empty host-sentinel second; single-to-multi conversion restarts the extension host');
  if (!vscode.workspace.isTrusted) {
    console.log('Gazette editor proof is waiting for explicit trust of its disposable workspace.');
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        subscription.dispose();
        reject(new Error('Disposable editor workspace was not explicitly trusted within three minutes'));
      }, 180000);
      const subscription = vscode.workspace.onDidGrantWorkspaceTrust(() => {
        clearTimeout(timer);
        subscription.dispose();
        resolve();
      });
    });
  }
  assert.ok(vscode.workspace.isTrusted);
  const path = join(folder.uri.fsPath, 'explicit-buffer.toml');
  const original = "# 🦀 é\r\n[[sites]]\r\nid='site:one'\r\npublisher='owner:missing'\r\ndeployable=false\r\n";
  const contentPath = join(folder.uri.fsPath, 'selected-guide.md');
  await writeFile(path, original, { flag: 'wx' });
  await writeFile(contentPath, '# Guide\n', { flag: 'wx' });
  await vscode.workspace.getConfiguration('gazette').update('server.path', executable, vscode.ConfigurationTarget.Global);
  const settings = vscode.workspace.getConfiguration('gazette', folder.uri);
  await settings.update('workspace.configuration', 'explicit-buffer.toml', vscode.ConfigurationTarget.WorkspaceFolder);
  await settings.update('workspace.documents', ['selected-guide.md'], vscode.ConfigurationTarget.WorkspaceFolder);
  const extension = vscode.extensions.getExtension('rtepl.gazette-vscode');
  assert.ok(extension);
  await extension.activate();
  await vscode.commands.executeCommand('gazette.restartLanguageServer');

  let document = await vscode.workspace.openTextDocument(vscode.Uri.file(path));
  document = await vscode.languages.setTextDocumentLanguage(document, 'plaintext');
  await vscode.window.showTextDocument(document);
  const invalid = await diagnostics(document.uri, values => values.some(value => value.code === 'workspace_reference'));
  assert.ok(invalid.every(value => value.source === 'gazette'));

  const replacement = "[[principals]]\nid='owner:one'\nkind='organization'\n[[principals]]\nid='owner:two'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n";
  const edit = new vscode.WorkspaceEdit();
  edit.replace(document.uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), replacement);
  assert.ok(await vscode.workspace.applyEdit(edit));
  await diagnostics(document.uri, values => values.length === 0);
  assert.ok(document.isDirty, 'diagnostics consume unsaved editor bytes');
  assert.equal(await readFile(path, 'utf8'), original, 'the extension never saves or rewrites the source');

  // The editor preserves the document's CRLF policy when applying LF replacement text.
  // Query the actual unsaved buffer, not offsets in the edit request.
  const current = document.getText();
  const referenceOffset = current.lastIndexOf('owner:one');
  assert.ok(referenceOffset >= 0);
  const hoverPosition = document.positionAt(referenceOffset);
  const definitions = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', document.uri, hoverPosition);
  assert.ok(definitions?.some(definition => definition.uri.toString() === document.uri.toString()
    && document.getText(definition.range) === "'owner:one'"), 'native reference points to its current declaration');
  const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', document.uri, hoverPosition);
  const expectedHover = new vscode.MarkdownString().appendText('Resolved principal: owner:one').value;
  assert.ok(hovers?.some(hover => hover.range && document.getText(hover.range) === "'owner:one'"
    && hover.contents.some(content => content.value === expectedHover)),
    `actual selected-source hover preserves plain text and current reference range: ${JSON.stringify(hovers)}`);

  const completions = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', document.uri, hoverPosition);
  const choice = completions?.items.find(item => item.label === 'owner:two');
  assert.ok(choice, 'actual editor completion includes the other native principal');
  assert.equal(choice.insertText, "'owner:two'");
  assert.equal(document.getText(choice.range), "'owner:one'");
  const proposed = new vscode.WorkspaceEdit();
  proposed.replace(document.uri, choice.range, choice.insertText);
  assert.ok(await vscode.workspace.applyEdit(proposed));
  await diagnostics(document.uri, values => values.length === 0);
  assert.ok(document.getText().includes("publisher='owner:two'"));
  assert.equal(await readFile(path, 'utf8'), original, 'user-applied proposal remains an unsaved buffer edit');

  const broken = new vscode.WorkspaceEdit();
  broken.replace(document.uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), '[[sites]\n');
  assert.ok(await vscode.workspace.applyEdit(broken));
  await diagnostics(document.uri, values => values.some(value => value.code === 'toml_invalid'));
  await vscode.commands.executeCommand('workbench.action.revertAndCloseActiveEditor');
  await diagnostics(document.uri, values => values.some(value => value.code === 'workspace_reference'));
  assert.equal(await readFile(path, 'utf8'), original, 'closing restores captured baseline without saving');
  let content = await vscode.workspace.openTextDocument(vscode.Uri.file(contentPath));
  content = await vscode.languages.setTextDocumentLanguage(content, 'plaintext');
  await vscode.window.showTextDocument(content);
  const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', content.uri);
  assert.ok(symbols?.some(symbol => symbol.name === 'Guide'), 'selected content symbols come from Gazette despite editor language');
  // Immediate native requests must follow exact full-change delivery, not depend
  // on the pinned provider's 250ms pending-change timer or stale diagnostics.
  for (const heading of ['Rapid é😀', 'Latest 🦀']) {
    const rapid = new vscode.WorkspaceEdit();
    rapid.replace(content.uri, new vscode.Range(content.positionAt(0), content.positionAt(content.getText().length)), `# ${heading}\r\n`);
    assert.ok(await vscode.workspace.applyEdit(rapid));
    const currentSymbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', content.uri);
    assert.ok(currentSymbols?.some(symbol => symbol.name === heading), 'immediate native request observes the latest full replacement');
  }
  const pending = new vscode.WorkspaceEdit();
  pending.replace(content.uri, new vscode.Range(content.positionAt(0), content.positionAt(content.getText().length)), '# Retirement 🦀\r\n');
  assert.ok(await vscode.workspace.applyEdit(pending));
  await vscode.commands.executeCommand('gazette.restartLanguageServer');
  const restartedSymbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', content.uri);
  assert.ok(restartedSymbols?.some(symbol => symbol.name === 'Retirement 🦀'),
    'rapid retirement/reinitialization retains the current open overlay');
  const unselected = join(folder.uri.fsPath, 'unselected.md');
  await writeFile(unselected, '# Unselected\n', { flag: 'wx' });
  let ignored = await vscode.workspace.openTextDocument(vscode.Uri.file(unselected));
  ignored = await vscode.languages.setTextDocumentLanguage(ignored, 'plaintext');
  assert.ok(vscode.languages.getDiagnostics(ignored.uri).every(value => value.source !== 'gazette'));
  assert.equal(await readFile(contentPath, 'utf8'), '# Guide\n');
  console.log('Gazette real editor proof: selected config/content, native diagnostics and symbols, unsaved full replacements, baseline restoration and source preservation.');
  await distributed(folder);
  // Keep the first folder unchanged throughout. The sentinel prevents the
  // initial single-to-multi host restart; after actual owner-root retirement,
  // retire only that test-owned sentinel through the same public lifecycle.
  // This restores one application folder without suppressing its real picker.
  const sentinel = vscode.workspace.workspaceFolders.find(value => value.uri.toString() === sentinelUri.toString());
  const retiredSentinel = folderChange(event => event.removed.some(value => value.uri.toString() === sentinelUri.toString()));
  assert.ok(sentinel && vscode.workspace.updateWorkspaceFolders(sentinel.index, 1));
  await retiredSentinel;
  await vscode.commands.executeCommand('gazette.restartLanguageServer');
  assert.equal(vscode.workspace.workspaceFolders.length, 1);
  assert.equal(vscode.workspace.workspaceFolders[0].uri.toString(), folder.uri.toString());
  await require('./applicationHost.cjs').run(folder);
}

module.exports = { run: async () => {
  try { await run(); }
  catch (error) {
    console.error('Gazette host fixture failed:', error?.stack ?? error);
    await new Promise(resolve => setTimeout(resolve, 500));
    throw error;
  }
} };
