// Actual addon and native generation custody; owned shell fixtures are not language qualification.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, lstatSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 4, 'supply exact addon and documentation_reference executable paths');
const [, , addon, referenceExecutable] = process.argv;
assert.ok(isAbsolute(referenceExecutable));
const gitExecutable = process.env.GAZETTE_TEST_GIT;
assert.ok(gitExecutable && isAbsolute(gitExecutable), 'GAZETTE_TEST_GIT must select an absolute Git executable');
const api = loadGazette(addon);
const allowed = {local:true, allowRead:true};
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
function failure(call, code) {
  let caught;
  try { call(); } catch (error) { caught = error; }
  assert.ok(caught instanceof Error, 'no failed module can become partial success');
  const result = JSON.parse(caught.message);
  assert.deepEqual(result, {code}, 'native failures disclose only their class');
}
function snapshot(root) {
  const records = [];
  function visit(relative) {
    const path = join(root, relative);
    const stat = lstatSync(path);
    assert.ok(!stat.isSymbolicLink());
    records.push([relative, stat.mode, stat.isDirectory() ? null : sha256(readFileSync(path))]);
    if (stat.isDirectory()) for (const name of readdirSync(path).sort()) visit(relative ? `${relative}/${name}` : name);
  }
  visit('');
  return records;
}
function fixture(root) {
  const files = {
    'generated/generator.sh':'if [ "$3" = fail ]; then printf partial > "$2"; exit 7; fi\n/bin/cat "$1" > "$2"\nprintf companion > "${2}.companion"\n',
    'generated/bundle/README.md':'# Référence\r\n\r\n[Client](nested/client.md#client).\r\n',
    'generated/bundle/nested/client.md':'# Client\n\n```rust\npub fn run(value: u64) -> u64;\n```\n\n[Overview](../README.md#reference).\n',
    'generated/library.json':JSON.stringify({schemaVersion:'2.0',variant:'project',id:0,kind:1,name:'Fixture',flags:{},files:{},children:[{variant:'declaration',id:1,kind:64,name:'run',flags:{},signatures:[{variant:'signature',id:2,kind:4096,name:'run',flags:{},type:{type:'intrinsic',name:'string'}}]}]}),
    'generated/server.json':'{"openapi":"3.1.0","info":{"title":"API","version":"1.2.3"},"paths":{},"x-exact-counter":18446744073709551615}',
    'generated/.private.bin':Buffer.from([0,255,128,13,10]),
  };
  for (const [relative, bytes] of Object.entries(files)) {
    mkdirSync(dirname(join(root, relative)), {recursive:true});
    writeFileSync(join(root, relative), bytes);
  }
  const git = args => execFileSync(gitExecutable, ['-c','core.hooksPath=/dev/null',...args], {
    cwd:root, encoding:'utf8', env:{...process.env,GIT_CONFIG_NOSYSTEM:'1',GIT_CONFIG_GLOBAL:'/dev/null',GIT_AUTHOR_DATE:'2000-01-01T00:00:00Z',GIT_COMMITTER_DATE:'2000-01-01T00:00:00Z'},
  });
  git(['init','--quiet']); git(['add','--all']);
  git(['-c','user.name=Fixture','-c','user.email=fixture@example.invalid','-c','commit.gpgSign=false','commit','--quiet','-m','pinned synthetic references']);
  const commit = git(['rev-parse','HEAD']).trim();
  let source = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n";
  for (const project of ['one','two']) source += `[[projects]]\nid='project:${project}'\nlifecycle='active'\nvisibility='public'\navailability='available'\n`;
  source += `[[sources]]\nid='source:artifact'\nlocation={kind='repository',root=${JSON.stringify(root)},commit='${commit}',subdirectory='generated'}\n[[sources]]\nid='source:unavailable'\nlocation={kind='repository',root=${JSON.stringify(join(root,'absent-source'))},commit='${commit}',subdirectory='generated'}\n[[brands]]\nid='brand:one'\nname='Référence brand'\n`;
  const destination = join(root, 'not-created-domain-doc');
  source += `[[brands.publications]]\nsite='site:one'\n[brands.publications.documentation_repository]\nname='brand-selected-doc-repository'\npath=${JSON.stringify(destination)}\n[brands.publications.documentation_repository.build]\ntarget={kind='mintlify',theme='mint',primary_color='#123abc'}\n`;
  const reference = (slug, input) => `[[brands.publications.documentation_repository.build.modules.navigation]]\nkind='reference'\nslug='${slug}'\ntitle='Reference ${slug}'\ninput=${input}\n`;
  const imported = (path, format, kind='file') => `{kind='artifact',source='source:artifact',selection={kind='${kind}',path='${path}'},format='${format}'}`;
  source += "[[brands.publications.documentation_repository.build.modules]]\nproject='project:two'\nslug='second-first'\ntitle='Second first'\n";
  source += reference('manual', imported('bundle','markdown','directory'));
  source += reference('server', imported('server.json','openapi-json'));
  source += "[[brands.publications.documentation_repository.build.modules]]\nproject='project:one'\nslug='first-second'\ntitle='First second'\n";
  source += reference('library', imported('library.json','typedoc-json'));
  source += `[[brands.publications]]\nsite='site:other'\n[brands.publications.documentation_repository]\nname='other-doc-repository'\npath=${JSON.stringify(join(root,'not-created-other-doc'))}\n[brands.publications.documentation_repository.build]\ntarget={kind='mintlify',theme='mint',primary_color='#123abc'}\n[[brands.publications.documentation_repository.build.modules]]\nproject='project:one'\nslug='other'\ntitle='Other'\n`;
  source += reference('library', "{kind='artifact',source='source:unavailable',selection={kind='file',path='library.json'},format='typedoc-json'}");
  return {source, destination, imported};
}

test('real addon assembles the whole configured reference publication without generated effects', async t => {
  const root = mkdtempSync(join(tmpdir(),'gazette-node-docs-'));
  t.after(() => rmSync(root,{recursive:true,force:true}));
  const {source, destination, imported} = fixture(root);
  const original = snapshot(root);
  const missing = source.replace("path='library.json'", "path='missing.json'");
  const generated = source.replace(imported('library.json','typedoc-json'),
    `{kind='generated',source='source:artifact',recipe={program=${JSON.stringify(join(root,'never-invoked'))},executable={bytes=1,sha256='${sha256(Buffer.from('x'))}'},arguments=[{kind='input',path='library.json'},{kind='output'}],version_arguments=['--version'],expected_version_sha256='${sha256(Buffer.alloc(0))}',output={kind='file',path='reference.json'},format='typedoc-json'}}`);
  const requests = [source, missing, generated].map(source => ({source,site:'site:one'}));
  requests.push({source,site:'site:other'});
  const native = JSON.parse(execFileSync(referenceExecutable, [], {input:JSON.stringify(requests),encoding:'utf8',maxBuffer:32*1024*1024}));
  assert.ok(native[0].files, JSON.stringify(native[0]));
  assert.equal(native[2].error, 'documentation_generation_required');
  const assemble = (text=source, options={}) => api.assembleDocumentation(Buffer.from(text), 'site:one', {...allowed,...options});
  function compare(result, expected=native[0]) {
    assert.deepEqual(Object.keys(result).sort(), ['files','report']);
    assert.equal(result.report, expected.report, 'exact shared Rust report bytes');
    const report = JSON.parse(result.report);
    const inventory = report.inventory.entries.filter(entry=>entry.kind==='file');
    assert.deepEqual(result.files.map(file=>file.path), inventory.map(entry=>entry.path));
    const files = result.files.map((file,index)=>{
      assert.ok(file.bytes instanceof Uint8Array);
      assert.equal(file.bytes.byteLength, inventory[index].bytes);
      assert.equal(sha256(file.bytes), inventory[index].sha256);
      return {path:file.path,bytes:Array.from(file.bytes)};
    });
    assert.deepEqual(files, expected.files, 'every complete file matches independent native bytes');
    return report;
  }
  await t.test('private preview map reaches the shared assembly and rejects invalid targets', () => {
    const map = {'https://example.test/':'http://127.0.0.1:4100/'};
    const mapped = assemble(source, {previewLinkBasesJson:JSON.stringify(map)});
    assert.deepEqual(JSON.parse(mapped.report).preview_link_bases, map);
    const ordinary = assemble().files;
    assert.ok(!ordinary.some(file => file.path === 'llms.txt'), 'ordinary output retains hosted index generation');
    const index = mapped.files.find(file => file.path === 'llms.txt');
    assert.ok(index, 'private preview supplies its physical-page index');
    const indexText = Buffer.from(index.bytes).toString('utf8');
    assert.ok(indexText.startsWith('# Référence brand\n'));
    assert.ok(!indexText.includes('.md)'), 'index destinations use served HTML routes');
    assert.ok(!indexText.includes('/second-first/server'), 'virtual provider roots are not retained physical pages');
    assert.deepEqual(mapped.files.filter(file => file.path !== 'llms.txt'), ordinary, 'unmatched links and generated references stay unchanged');
    for (const previewLinkBasesJson of ['[]', '{}', '{"https://example.test/":"https://127.0.0.1:4100/"}', '{"https://example.test/":"http://192.0.2.1:4100/"}']) {
      failure(() => assemble(source, {previewLinkBasesJson}), 'node_query');
    }
  });
  await t.test('brand-owned destination, ordered multi-project modules, exact numbers and source capture', ()=>{
    const result = assemble();
    const report = compare(result);
    assert.equal(report.status,'documentation-assembled-local');
    assert.equal(report.repository.path,destination);
    assert.equal(report.repository.name,'brand-selected-doc-repository');
    assert.deepEqual(report.artifacts.map(artifact=>[artifact.project,artifact.module,artifact.reference]), [
      ['project:two','second-first','manual'],['project:two','second-first','server'],['project:one','first-second','library'],
    ]);
    assert.equal(report.directory,null); assert.deepEqual(report.jobs,[]);
    assert.ok(report.artifacts.every(artifact=>!Object.hasOwn(artifact,'generation')));
    assert.ok(report.inventory.entries.some(entry=>entry.kind==='directory'));
    assert.ok(result.files.every(file=>!file.path.includes('.private')));
    const apiFile = result.files.find(file=>file.path.endsWith('/server.json'));
    assert.ok(apiFile,'OpenAPI retained in its native format');
    assert.equal(Buffer.from(apiFile.bytes).toString(),readFileSync(join(root,'generated/server.json'),'utf8'));
    assert.ok(Buffer.from(apiFile.bytes).includes(Buffer.from('18446744073709551615')));
    const navigation = JSON.parse(Buffer.from(result.files.find(file=>file.path==='docs.json').bytes).toString());
    assert.ok(JSON.stringify(navigation).indexOf('Second first') < JSON.stringify(navigation).indexOf('First second'));
    assert.deepEqual(snapshot(root),original);
  });
  await t.test('returned binary buffers are isolated from siblings, later calls and source bytes', ()=>{
    const first = assemble(); const second = assemble();
    const report = first.report;
    const untouched = first.files.slice(1).map(file=>sha256(file.bytes));
    first.files[0].bytes.fill(255); first.files[0].bytes[0]=0;
    assert.equal(first.report,report);
    assert.deepEqual(first.files.slice(1).map(file=>sha256(file.bytes)),untouched);
    compare(second); compare(assemble());
    assert.deepEqual(snapshot(root),original);
  });
  await t.test('missing sibling, configured generation and inaccessible other site fail as whole operations', ()=>{
    assert.equal(native[1].error,'documentation_artifact');
    failure(()=>assemble(missing),native[1].error);
    failure(()=>assemble(generated),native[2].error);
    assert.ok(native[3].error);
    failure(()=>api.assembleDocumentation(Buffer.from(source),'site:other',allowed),native[3].error);
    compare(assemble()); assert.deepEqual(snapshot(root),original);
  });
  await t.test('permissions, malformed named limits and unsafe host integers reject before acquisition', ()=>{
    for (const flags of [{local:false},{allowRead:false}]) failure(()=>assemble('invalid',flags),'documentation_permission');
    for (const value of [-1,0.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1]) {
      failure(()=>assemble('invalid',{resultBytes:value}),'node_limits');
    }
    for (const limitsJson of ['[]','null','{"unknown":1}','{"syntax":[]}','{"output":null}','{"acquisition":[]}',
      '{"acquisition":{"per_source":[]}}','{"acquisition":{"per_source":{"content":[]}}}',
      '{"output":{"entries":1,"entr\\u0069es":2}}','{"output":{"total_bytes":18446744073709551616}}']) {
      failure(()=>assemble('invalid',{limitsJson}),'node_query');
    }
    failure(()=>assemble('invalid',{limitsJson:'{}',limitsBytes:1}),'node_query_limit');
    failure(()=>api.assembleDocumentation(Buffer.from(source),'',allowed),'documentation_site');
    failure(()=>api.assembleDocumentation(Buffer.from([255]),'site:one',allowed),'node_source_utf8');
  });
  await t.test('independent native and carrier ceilings never truncate or deliver a subset', ()=>{
    const exact = {inputBytes:Buffer.byteLength(source),resultBytes:Buffer.byteLength(native[0].report)};
    compare(assemble(source,exact));
    failure(()=>assemble(source,{...exact,inputBytes:exact.inputBytes-1}),'node_input_limit');
    failure(()=>assemble(source,{...exact,resultBytes:exact.resultBytes-1}),'node_result_limit');
    failure(()=>assemble(source,{limitsJson:'{"syntax":{"nodes":0}}'}),'toml_limit');
    failure(()=>assemble(source,{limitsJson:'{"output":{"entries":0}}'}),'source_limit');
    failure(()=>assemble(source,{limitsJson:'{"acquisition":{"sources":0,"entries":128,"bytes":1048576,"per_source":{}}}'}),'composition_source_limit');
    const large = '{"output":{"total_bytes":18446744073709551615}}';
    compare(assemble(source,{limitsJson:large,limitsBytes:Buffer.byteLength(large)}));
    assert.deepEqual(snapshot(root),original);
  });
  await t.test('hidden companion drift is rejected rather than reread into a different reference build', ()=>{
    writeFileSync(join(root,'generated/.private.bin'),Buffer.from([1,2,3]));
    const changed = snapshot(root);
    let caught;
    try { assemble(); } catch(error) { caught=error; }
    assert.ok(caught instanceof Error);
    assert.ok(JSON.parse(caught.message).code.startsWith('source_'));
    assert.deepEqual(snapshot(root),changed,'failed SDK operation does not repair the source');
  });
});

test('explicit native generation reaches complete Mintlify assembly and preserves failure custody', async t => {
  const root = mkdtempSync(join(tmpdir(),'gazette-node-generated-docs-'));
  const jobs = mkdtempSync(join(tmpdir(),'gazette-node-doc-jobs-'));
  t.after(() => { rmSync(root,{recursive:true,force:true}); rmSync(jobs,{recursive:true,force:true}); });
  const {source, destination, imported} = fixture(root);
  const original = snapshot(root);
  const executable = readFileSync('/bin/sh');
  const request = {parent:jobs,max_jobs:2,directory_bytes:256,entry_bytes:64,allow_execute:true};
  function generated(fail=false, large=false) {
    let value = source;
    for (const [path, format] of [['server.json','openapi-json'],['library.json','typedoc-json']]) {
      const version = large
        ? 'i=0; while [ "$i" -lt 4096 ]; do printf "owned-version-01234567890123456789\\n"; i=$((i+1)); done'
        : 'printf "owned-js-generator/1\\n"';
      const versionStdout = large
        ? 'owned-version-01234567890123456789\n'.repeat(4096)
        : 'owned-js-generator/1\n';
      const recipe = `{kind='generated',source='source:artifact',recipe={program='/bin/sh',executable={bytes=${executable.length},sha256='${sha256(executable)}'},arguments=[{kind='input',path='generator.sh'},{kind='input',path='${path}'},{kind='output'},{kind='literal',value='${fail && path==='library.json' ? 'fail' : 'ok'}'}],version_arguments=['-c',${JSON.stringify(version)}],expected_version_sha256='${sha256(Buffer.from(versionStdout))}',output={kind='file',path='reference.json'},format='${format}'}}`;
      value = value.replace(imported(path,format),recipe);
    }
    return value;
  }
  const run = (text=generated(),options={}) => api.assembleDocumentation(Buffer.from(text),'site:one',{
    ...allowed,allowStage:true,generationJson:JSON.stringify(request),...options,
  });
  function caught(call) {
    let error; try { call(); } catch(value) { error=value; }
    assert.ok(error instanceof Error,'failure is never successful files');
    return JSON.parse(error.message);
  }
  await t.test('denied permissions and impossible carrier capacity precede every stage and process',()=>{
    for (const options of [{allowStage:false},{allowStage:undefined},
      {generationJson:JSON.stringify({...request,allow_execute:false})},
      {generationJson:JSON.stringify({...request,allow_execute:undefined})}]) {
      failure(()=>run('invalid',options),'documentation_permission');
    }
    failure(()=>run('invalid',{resultBytes:1}),'documentation_response_limit');
    for (const generationJson of ['[]','null',JSON.stringify({...request,input:[]}),
      JSON.stringify({...request,unknown:true}),'{"parent":"/tmp","parent":"/other"}']) {
      failure(()=>run('invalid',{generationJson}),'node_query');
    }
    assert.deepEqual(readdirSync(jobs),[]);
    assert.deepEqual(snapshot(root),original);
  });
  await t.test('two native jobs feed the selected modules, exact captured bytes and navigation',()=>{
    const result = run();
    const report = JSON.parse(result.report);
    assert.equal(report.status,'documentation-assembled-local');
    assert.equal(report.repository.path,destination);
    assert.equal(report.directory,null,'no final target or configured destination is staged');
    assert.equal(report.jobs.length,2);
    assert.equal(readdirSync(jobs).length,4,'exact two retained stages per job');
    for (const [index,path] of ['server.json','library.json'].entries()) {
      const job = report.jobs[index];
      assert.equal(job.evidence.version_text,'owned-js-generator/1\n');
      assert.equal(job.evidence.generation.exit.code,0);
      assert.deepEqual(readFileSync(join(job.output,'reference.json')),readFileSync(join(root,'generated',path)));
      assert.equal(readFileSync(join(job.output,'reference.json.companion'),'utf8'),'companion');
      assert.deepEqual(readFileSync(join(job.input,path)),readFileSync(join(root,'generated',path)));
      const artifact = report.artifacts.find(artifact=>artifact.generation && artifact.content.sha256===sha256(readFileSync(join(root,'generated',path))));
      assert.ok(artifact,'captured artifact remains bound to the native original');
      assert.deepEqual(artifact.generation,job.evidence);
    }
    const inventory = report.inventory.entries.filter(entry=>entry.kind==='file');
    assert.deepEqual(result.files.map(file=>file.path),inventory.map(entry=>entry.path));
    result.files.forEach((file,index)=>assert.equal(sha256(file.bytes),inventory[index].sha256));
    const navigation = JSON.parse(Buffer.from(result.files.find(file=>file.path==='docs.json').bytes));
    assert.ok(JSON.stringify(navigation).includes('Second first'));
    assert.ok(JSON.stringify(navigation).includes('First second'));
    assert.ok(result.files.some(file=>Buffer.from(file.bytes).includes(Buffer.from('run'))));
    assert.deepEqual(snapshot(root),original);
  });
  await t.test('later failure returns complete prior job and both active retained stages',()=>{
    const before = readdirSync(jobs).length;
    const report = caught(()=>run(generated(true)));
    assert.equal(report.code,'documentation_response_unavailable');
    assert.equal(report.failure.code,'documentation_generator_exit');
    assert.equal(report.completed.length,1);
    assert.equal(report.completed[0].generation.exit.code,0);
    assert.equal(report.failure.generation.exit.code,7);
    assert.equal(report.retained_unix_bytes.length,4);
    assert.equal(readdirSync(jobs).length-before,4,'no implicit retry or cleanup');
    for (const path of report.retained_unix_bytes) assert.ok(lstatSync(Buffer.from(path)).isDirectory());
    assert.equal(readFileSync(join(Buffer.from(report.failure.output_unix_bytes).toString(),'reference.json'),'utf8'),'partial');
    assert.deepEqual(snapshot(root),original);
  });
  await t.test('post-effect full-report overflow returns native custody, never a successful file subset',()=>{
    const before = readdirSync(jobs).length;
    const report = caught(()=>run(generated(false,true),{resultBytes:64*1024}));
    assert.equal(report.code,'documentation_response_unavailable');
    assert.equal(report.completed.length,2);
    assert.equal(report.failure.code,null);
    assert.ok(report.output_identity);
    assert.equal(readdirSync(jobs).length-before,4);
    for (const job of report.completed) {
      assert.equal(job.generation.exit.code,0);
      assert.ok(job.version.stdout_bytes>64*1024);
      assert.ok(lstatSync(Buffer.from(job.output_unix_bytes)).isDirectory());
    }
    assert.deepEqual(snapshot(root),original);
  });
});
