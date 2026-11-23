const {test} = require('node:test');
const assert = require('node:assert/strict');
const {createHash} = require('node:crypto');
const {mkdir, mkdtemp, realpath, rm, writeFile} = require('node:fs/promises');
const {tmpdir} = require('node:os');
const {join} = require('node:path');
const {createEditor, Uri} = require('./support/editor.cjs');

test('a compiled resource opens its matching input from an explicit root and refuses changed bytes', async () => {
  const root = await mkdtemp(join(await realpath(tmpdir()), 'gazette-build-input-'));
  try {
    const workspace = join(root, 'workspace');
    const theme = join(root, 'theme');
    const entry = 'styles/_navigation.scss';
    const path = join(theme, entry);
    const source = '.menu {}';
    const sha256 = createHash('sha256').update(source).digest('hex');
    await mkdir(join(theme, 'styles'), {recursive:true});
    await writeFile(path, source);
    const host = createEditor([workspace, theme]);
    const folder = host.vscode.workspace.workspaceFolders[0];
    host.settings.set(folder.uri.toString(), {'workspace.roots': [{id:'source:base', uri:new Uri(theme).toString()}]});
    host.vscode.Uri.file = path => new Uri(path);
    host.vscode.window.showQuickPick = async choices => choices[0];
    host.vscode.window.showInformationMessage = async () => undefined;
    const opened = [];
    host.vscode.commands.executeCommand = async (command, uri) => opened.push([command, uri.fsPath]);
    const result = {status:'local_set_rendered',
      compilation:{workspace:{sources:{'source:base':{location:{kind:'filesystem',root:theme}}}}},
      sources:{'source:base':{observation:{entries:[{path:entry,kind:'file',bytes:Buffer.byteLength(source),sha256}]}}},
      sites:{'site:example':{evidence:{theme:{theme:'theme:example'},resources:{'stylesheet:physics':{
        output:'assets/physics.css',url:'/assets/physics.css',output_sha256:sha256,
        resource:{owner:{kind:'theme',package:'theme:base'},resource:'stylesheet:physics'},
        selection:{scope:'workspace',field:{kind:'resource'}},recipe:{entry},
        input_origins:{[entry]:{source:'source:base',entry,sha256}},
      }}}}}};
    const raw = JSON.stringify({result:{structuredContent:result}});
    const {openBuildInput} = require('../dist/buildInputs');
    await openBuildInput(host.vscode, folder, raw, () => {});
    assert.deepEqual(opened, [['vscode.open', path]]);
    await writeFile(path, '.new! {}');
    await assert.rejects(openBuildInput(host.vscode, folder, raw, () => {}), /differs from the retained receipt/);
    assert.equal(opened.length, 1);
  } finally {
    await rm(root, {recursive:true,force:true});
  }
});
