// Real editor commands -> persistent MCP -> native source capture and rendering.
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const { createHash } = require('node:crypto');
const { mkdir, readFile, writeFile } = require('node:fs/promises');
const { basename, dirname, isAbsolute, join } = require('node:path');
const vscode = require('vscode');
const { ApplicationCommands, bindOperationRequest } = require('../dist/applicationCommands.js');

async function run(folder) {
  assert.equal(process.env.GAZETTE_VSCODE_ISOLATED, '1');
  assert.ok(basename(folder.uri.fsPath).startsWith('gazette-vscode-proof.'));
  const root = join(folder.uri.fsPath, 'publication');
  await mkdir(root);
  const files = {
    'content/page.md': "+++\n[content]\nid='content:editor'\nsite='site:editor'\ntitle='Editor'\nlifecycle='draft'\nroute={path='/editor/'}\nstructure={kind='page',family={kind='article'}}\n+++\nRetained **editor**.\n",
    'theme/theme.toml': "package='theme:editor'\nnamespace='editor'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n",
    'theme/templates/page.html': '<h1>{{ content.title }}</h1>{{ body_html | safe }}',
  };
  for (const [path, text] of Object.entries(files)) {
    await mkdir(dirname(join(root, path)), { recursive: true });
    await writeFile(join(root, path), text, { flag: 'wx' });
  }
  const gitProgram = process.env.GAZETTE_TEST_GIT;
  assert.ok(gitProgram && isAbsolute(gitProgram),
    'GAZETTE_TEST_GIT must select an absolute Git executable');
  const git = args => execFileSync(gitProgram, ['-c', 'core.hooksPath=/dev/null', ...args], {
    cwd: root, encoding: 'utf8', timeout: 10000,
    env: { PATH: process.env.PATH, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null',
      GIT_AUTHOR_DATE: '2000-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2000-01-01T00:00:00Z' },
  });
  git(['init', '--quiet']);
  git(['add', '--', 'content', 'theme']);
  git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgSign=false', 'commit', '--quiet', '-m', 'native editor fixture']);
  const commit = git(['rev-parse', 'HEAD']).trim();
  let workspace = "[composition]\ntheme={package='theme:editor'}\n[[principals]]\nid='owner:editor'\nkind='person'\n[[sites]]\nid='site:editor'\npublisher='owner:editor'\ndeployable=false\n[[brands]]\nid='brand:editor'\nname='Editor fixture'\n[[brands.publications]]\nsite='site:editor'\npublic_base_url='https://editor.example/'\nauthored=[{source='source:content',target='content'}]\n";
  for (const source of ['content', 'theme']) {
    workspace += `[[sources]]\nid='source:${source}'\n`;
    workspace += `location={kind='repository',root=${JSON.stringify(root)},commit='${commit}',subdirectory='${source}'}\n`;
  }
  workspace += "[[theme_packages]]\npackage='theme:editor'\nsource='source:theme'\nmanifest='theme.toml'\n";
  const recipe = JSON.stringify({ format: 'gazette-local-publication-set/1',
    workspace: { kind: 'supplied', source: workspace },
    sites: [{ site: 'site:editor', model_output: 'model.json' }] });
  const testWindow = Object.create(vscode.window);
  Object.defineProperties(testWindow, {
    showWarningMessage: { value: async (_message, ...choices) => {
      if (choices.includes('Run')) return 'Run';
      if (choices.includes('Open Full Receipt')) return 'Open Full Receipt';
      if (choices.includes('Stop')) return 'Stop';
      return undefined;
    } },
    showInformationMessage: { value: async (_message, ...choices) =>
      choices.includes('Open Full Receipt') ? 'Open Full Receipt' : undefined },
  });
  const testCommands = Object.create(vscode.commands);
  Object.defineProperty(testCommands, 'registerCommand', {
    value: () => ({ dispose() {} }),
  });
  const host = Object.create(vscode);
  Object.defineProperties(host, {
    window: { value: testWindow },
    commands: { value: testCommands },
  });
  const application = new ApplicationCommands(host, { subscriptions: [] });

  const catalogDocument = await application.showTools();
  const discovery = JSON.parse(catalogDocument.getText());
  const pages = Array.isArray(discovery) ? discovery : [discovery];
  assert.ok(pages.length > 0, 'displayed discovery contains at least one native page');
  const catalog = pages.flatMap(page => {
    assert.ok(Array.isArray(page?.result?.tools), 'every displayed discovery page is complete');
    return page.result.tools;
  });
  assert.equal(new Set(catalog.map(tool => tool.name)).size, catalog.length,
    'displayed paged discovery contains no duplicate operations');
  assert.ok(catalog.some(tool => tool.name === 'render_sites'));
  assert.ok(catalog.some(tool => tool.name === 'check_workspace'));
  const binding = bindOperationRequest(catalog, 'render_sites');
  const session = [...application.sessions.values()][0];
  assert.ok(session, 'live discovery retains one selected native application session');
  const prepared = await vscode.workspace.openTextDocument({ language: 'json', content: binding.text });
  await vscode.window.showTextDocument(prepared, { preview: false });
  application.requests.set(prepared.uri.toString(), {
    session, tool: binding.tool, catalog, document: prepared,
  });
  const uri = prepared.uri;
  const request = await vscode.workspace.openTextDocument(uri);
  const skeleton = JSON.parse(request.getText());
  assert.equal(skeleton.recipe_json, null);
  assert.equal(skeleton.allow_read, null);
  function inert(value) {
    if (value === null) return true;
    if (Array.isArray(value)) return value.length === 0;
    return typeof value === 'object' && value !== null && Object.values(value).every(inert);
  }
  assert.ok(inert(skeleton), 'opening a schema-shaped request cannot select or authorize native effects');

  async function invoke(allowRead) {
    const text = JSON.stringify({ recipe_json: recipe, local: true, allow_read: allowRead }, null, 2);
    const edit = new vscode.WorkspaceEdit();
    edit.replace(uri, new vscode.Range(request.positionAt(0), request.positionAt(request.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
    await vscode.window.showTextDocument(request, { preview: false });
    const result = await application.executeRequest();
    assert.ok(result, 'the explicit local fixture request must be confirmed');
    assert.equal(request.getText(), text, 'the native operation does not rewrite its input');
    return JSON.parse(result.getText()).result;
  }
  const built = await invoke(true);
  assert.equal(built.isError, false, JSON.stringify(built));
  assert.equal(built.structuredContent.status, 'local_set_rendered');
  assert.deepEqual(Object.keys(built.structuredContent.sites), ['site:editor']);
  const expected = '<h1>Editor</h1><p>Retained <strong>editor</strong>.</p>\n';
  const entry = built.structuredContent.sites['site:editor'].inventory.entries.find(entry => entry.path === 'editor/index.html');
  assert.equal(entry.sha256, createHash('sha256').update(expected).digest('hex'));
  assert.equal(entry.bytes, Buffer.byteLength(expected));
  const denied = await invoke(false);
  assert.equal(denied.isError, true);
  assert.equal(denied.structuredContent.code, 'publication_permission');
  const shown = await application.showResult();
  assert.deepEqual(JSON.parse(shown.getText()).result, denied, 'retained evidence is displayed without rerunning');
  for (const [path, text] of Object.entries(files)) assert.equal(await readFile(join(root, path), 'utf8'), text);
  assert.equal(git(['status', '--porcelain']), '');
  await application.stopSession();
  console.log('Gazette real application proof: live native discovery, isolated-host-confirmed pinned publication build, exact output hash, denied read, retained result and unchanged sources.');
}

module.exports = { run };
