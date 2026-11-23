const assert = require('node:assert/strict');
const { test } = require('node:test');
const { executable, sourceSelection, sourcePath, documentSelector, serverOptions } = require('../dist/settings.js');
const manifest = require('../package.json');
const applicationCommandCatalog = require('../src/applicationCommandCatalog.json');
const {readFileSync} = require('node:fs');
const {dirname, join} = require('node:path');
const {runInNewContext} = require('node:vm');

const folder = { uri: { scheme: 'file', fsPath: '/work', toString: () => 'file:///work' } };
function workspace(setting = { globalValue: '/exact/gazette' }, values = {}) {
  return { isTrusted: true, workspaceFolders: [folder], getConfiguration: (_name, uri) => {
    assert.equal(uri, folder.uri);
    return { inspect: () => setting, get: (name, fallback) => values[name] ?? fallback };
  } };
}

test('only explicit user absolute executable and trusted native folder are admitted', () => {
  assert.equal(executable(workspace(), folder), '/exact/gazette');
  for (const value of ['gazette', './gazette', '/exact/gazette\0']) {
    assert.throws(() => executable(workspace({ globalValue: value }), folder), /absolute/);
  }
  for (const field of ['workspaceValue', 'workspaceFolderValue', 'workspaceLanguageValue', 'workspaceFolderLanguageValue']) {
    assert.throws(() => executable(workspace({ globalValue: '/exact/gazette', [field]: '/other' }), folder), /workspace/);
  }
  assert.throws(() => executable({ ...workspace(), isTrusted: false }, folder), /trusted/);
  assert.throws(() => executable({ ...workspace(), workspaceFolders: [] }, folder), /local file/);
  assert.throws(() => executable(workspace(), { uri: { scheme: 'remote' } }), /local file/);
});

test('an unset User override uses only the installed executable, shared by LSP and MCP', () => {
  const filename = require.resolve('../dist/settings.js');
  const expected = join(dirname(filename), '..', 'server', 'gazette');
  const observed = [];
  let runnable = true;
  const exports = {};
  runInNewContext(readFileSync(filename, 'utf8'), {
    exports, __dirname: dirname(filename), process: {platform: 'darwin'},
    require: name => name === 'node:fs' ? {
      constants: {X_OK: 1},
      statSync: path => { assert.equal(path, expected); return {isFile: () => runnable}; },
      accessSync: (path, mode) => observed.push([path, mode]),
    } : require(name),
  });
  for (const value of [undefined, '']) {
    const host = workspace({globalValue: value});
    assert.equal(exports.executable(host, folder), expected);
    assert.equal(exports.serverOptions(host, folder).command, expected);
  }
  assert.equal(observed.length, 4);
  assert.equal(exports.executable(workspace(), folder), '/exact/gazette');
  for (const field of ['workspaceValue', 'workspaceFolderValue', 'workspaceLanguageValue', 'workspaceFolderLanguageValue']) {
    assert.throws(() => exports.executable(workspace({[field]: '/repository/program'}), folder), /workspace/);
  }
  runnable = false;
  assert.throws(() => exports.executable(workspace({}), folder), /no runnable bundled/);
  assert.equal(exports.executable(workspace(), folder), '/exact/gazette');
});

test('explicit rooted selectors preserve spelling and manual native source keys', () => {
  const root={id:'workspace',uri:folder.uri.toString()};
  const local=path=>({root:'workspace',file:path,path});
  assert.deepEqual(sourceSelection(workspace(),folder),{roots:[root],configuration:local('gazette.toml'),configurationDocuments:[],documents:[],manuals:[],pagePackageInputs:[],packageBaselines:[]});
  const selected=sourceSelection(workspace(undefined,{
    'workspace.roots':[{id:'owner',uri:'file:///owner'}],
    'workspace.configurationDocuments':['site.toml'],
    'workspace.documents':[{root:'owner',file:'café%20[one].md',path:'content/one.md'}],
    'workspace.manuals':[{source:'source:manual',file:'docs/start.md'}],
    'workspace.packageBaselines':[{kind:'theme',package:'theme:main',root:'owner',file:'theme.toml',path:'packages/main.toml'}],
  }),folder);
  assert.deepEqual(selected.documents,[{root:'owner',file:'café%20[one].md',path:'content/one.md'}]);
  assert.deepEqual(selected.manuals,[{source:'source:manual',...local('docs/start.md')}]);
  assert.deepEqual(selected.packageBaselines,[{kind:'theme',package:'theme:main',root:'owner',file:'theme.toml',path:'packages/main.toml'}]);
  for(const path of ['','/absolute','../outside','a/../b','./a','a//b','a/','a\\b','a\0b','a\u0085b'])assert.throws(()=>sourcePath(path));
  for(const value of ['*.md',['same.md','same.md'],['gazette.toml'],['../escape'],[{root:'unknown',file:'one.md',path:'one.md'}],[{root:'workspace',file:'one.md',path:'one.md',source:'guessed'}]])assert.throws(()=>sourceSelection(workspace(undefined,{'workspace.documents':value}),folder));
  for(const value of [['manual.md'],[{file:'manual.md'}],[{source:'',file:'manual.md'}],[{source:'source:manual',file:'../outside.md'}]])assert.throws(()=>sourceSelection(workspace(undefined,{'workspace.manuals':value}),folder));
  for(const key of ['workspace.configurationDocuments','workspace.pagePackageInputs'])for(const value of ['*.md',['same.md','same.md'],['gazette.toml'],['../escape']])assert.throws(()=>sourceSelection(workspace(undefined,{[key]:value}),folder));
  for(const value of ['theme.toml',[{kind:'theme',package:'theme:main'}],[{kind:'provider',package:'theme:main',file:'theme.toml'}],[{kind:'theme',package:'theme:main',file:'theme.toml',source:'source:theme'}],[{kind:'theme',package:'theme:main',file:'one.toml'},{kind:'theme',package:'theme:main',file:'two.toml'}]])assert.throws(()=>sourceSelection(workspace(undefined,{'workspace.packageBaselines':value}),folder));
});

test('selectors escape literal glob characters and never claim a whole language', () => {
  assert.deepEqual(documentSelector([{ fsPath: '/work/[a]*?.md' }]), [{ scheme: 'file', pattern: '/work/[[]a[]][*][?].md' }]);
  assert.equal(manifest.contributes.configuration.properties['gazette.server.path'].scope, 'machine');
  for (const key of ['gazette.workspace.roots', 'gazette.workspace.configuration', 'gazette.workspace.configurationDocuments', 'gazette.workspace.documents', 'gazette.workspace.manuals', 'gazette.workspace.pagePackageInputs', 'gazette.workspace.packageBaselines']) assert.equal(manifest.contributes.configuration.properties[key].scope, 'resource');
  assert.equal(manifest.capabilities.untrustedWorkspaces.supported, false);
  assert.equal(manifest.capabilities.virtualWorkspaces.supported, false);
  assert.equal(manifest.contributes.languages[0].extensions, undefined);
  assert.deepEqual(manifest.dependencies, { 'vscode-languageclient': '9.0.1' });
});

test('native options retain gazette lsp without shell, PATH search or injected stdio', () => {
  assert.deepEqual(serverOptions(workspace(), folder), { command: '/exact/gazette', args: ['lsp'], options: { shell: false, cwd: '/work' } });
  assert.equal(serverOptions(workspace(), folder).transport, undefined);
});

test('manifest keeps the palette bounded while retaining the complete programmatic route catalog', () => {
  const application = manifest.contributes.commands
    .map(entry => entry.command)
    .filter(command => command.startsWith('gazette.application.'));
  assert.deepEqual(application, []);
  assert.equal(new Set(applicationCommandCatalog.map(route => route.cli)).size, 56);
  for (const command of [
    'gazette.createOperationRequest', 'gazette.executeOperationRequest',
    'gazette.showApplicationTools', 'gazette.showApplicationResult',
    'gazette.stopApplicationSession', 'gazette.restartLanguageServer',
  ]) assert.ok(manifest.contributes.commands.some(entry => entry.command === command));
});
