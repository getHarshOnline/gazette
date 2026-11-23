// Explicit installed tools only. No installation, fake artifact, or alternate runner.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, readlinkSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const [gazette, node, typedoc, lockPath, expectedLock, git] = process.argv.slice(2);
assert.equal(process.argv.length, 8, 'supply Gazette, Node, TypeDoc bin, package-lock.json, independently retained lock SHA-256 and Git');
for (const path of [gazette, node, typedoc, lockPath, git]) {
  assert.ok(isAbsolute(path), 'all provider locators must be explicit absolute paths');
  assert.ok(lstatSync(realpathSync(path)).isFile(), 'installed regular file required');
}
assert.equal(realpathSync(process.execPath), realpathSync(node), 'launch this fixture with the selected Node');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
assert.match(expectedLock, /^[a-f0-9]{64}$/);
assert.equal(sha(readFileSync(lockPath)), expectedLock, 'independent dependency lock binding');
const install = dirname(realpathSync(lockPath));
const modules = join(install, 'node_modules');
assert.equal(realpathSync(typedoc), realpathSync(join(modules, 'typedoc/bin/typedoc')));
const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
assert.equal(lock.lockfileVersion, 3, 'this fixture qualifies one explicit npm lock format');
const requiredNode = lock.packages['']?.engines?.node;
assert.match(requiredNode, /^\d+\.\d+\.\d+$/, 'root lock must select one exact Node version');
assert.equal(process.versions.node, requiredNode, 'selected Node must exactly match root lock engines.node');
assert.equal(lock.packages['node_modules/typedoc'].version, '0.28.20');
for (const [path, selected] of Object.entries(lock.packages)) {
  if (path === '') continue;
  assert.ok(path.startsWith('node_modules/') && !path.split('/').includes('..'));
  assert.ok(!selected.link && selected.integrity, 'installed registry package lock evidence required');
  const manifest = JSON.parse(readFileSync(join(install, path, 'package.json'), 'utf8'));
  assert.equal(manifest.version, selected.version, `installed package version: ${path}`);
}

// Observe the complete installed package bytes, not merely the interpreter pin.
// This is a local identity/recheck, not an npm tarball-integrity verifier or sandbox.
function closure() {
  const rows = [];
  let bytes = 0;
  function visit(directory) {
    for (const name of readdirSync(directory).sort()) {
      const path = join(directory, name);
      const rel = relative(modules, path).split(sep).join('/');
      assert.ok(rows.length < 30_000, 'bounded tool file count');
      const stat = lstatSync(path);
      if (stat.isSymbolicLink()) {
        const target = relative(modules, realpathSync(path));
        assert.ok(target !== '..' && !target.startsWith(`..${sep}`) && !isAbsolute(target), 'tool symlink must remain inside closure');
        rows.push([rel, 'link', readlinkSync(path)]);
      } else if (stat.isDirectory()) {
        rows.push([rel, 'directory']);
        visit(path);
      } else {
        assert.ok(stat.isFile(), 'no special provider files');
        bytes += stat.size;
        assert.ok(bytes <= 256 * 1024 * 1024, 'bounded complete provider bytes');
        rows.push([rel, stat.size, sha(readFileSync(path))]);
      }
    }
  }
  visit(modules);
  return { sha256: sha(JSON.stringify(rows)), entries: rows.length, bytes };
}
const before = closure();
const root = mkdtempSync(join(tmpdir(), 'gazette-typedoc-qualification-'));
let success = false;
function invoke(program, args, cwd = root, input) {
  const result = spawnSync(program, args, {
    cwd, input, encoding: 'utf8', timeout: 90_000, maxBuffer: 16 * 1024 * 1024,
    env: { TMPDIR: root, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null',
      GIT_AUTHOR_NAME: 'Fixture', GIT_AUTHOR_EMAIL: 'fixture@example.invalid',
      GIT_COMMITTER_NAME: 'Fixture', GIT_COMMITTER_EMAIL: 'fixture@example.invalid',
      GIT_AUTHOR_DATE: '2000-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2000-01-01T00:00:00Z' },
  });
  if (result.error) throw result.error;
  assert.equal(result.signal, null, 'bounded process completed');
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return result.stdout;
}
try {
  const source = join(root, 'source');
  const sourceSelection = join(source, 'fixture');
  const jobs = join(root, 'jobs');
  const output = join(root, 'output');
  for (const path of [source, jobs, output]) mkdirSync(path);
  mkdirSync(join(sourceSelection, 'src'), { recursive: true });
  const fixtures = fileURLToPath(new URL('./fixtures/typedoc/', import.meta.url));
  const files = ['package.json', 'tsconfig.json', 'typedoc.json', 'src/model.ts', 'src/api.ts'];
  for (const file of files) writeFileSync(join(sourceSelection, file), readFileSync(join(fixtures, file)));
  invoke(git, ['init', '--quiet', source]);
  invoke(git, ['add', '--', ...files.map(file => join('fixture', file))], source);
  invoke(git, ['-c', 'commit.gpgsign=false', 'commit', '--quiet', '-m', 'owned qualification fixture'], source);
  const commit = invoke(git, ['rev-parse', 'HEAD'], source).trim();
  const quote = JSON.stringify;
  const literal = value => `{kind='literal',value=${quote(value)}}`;
  const sourceLinkTemplate = `https://github.com/example/reference/blob/${commit}/{path}#L{line}`;
  const arguments_ = [literal(typedoc), literal('--options'), "{kind='input',path='typedoc.json'}", literal('--sourceLinkTemplate'), literal(sourceLinkTemplate), literal('--json'), "{kind='output'}"];
  const executable = readFileSync(node);
  const typedocVersion = invoke(node, [typedoc, '--version']);
  const destination = join(root, 'brand-owned-docs');
  const config = `[[principals]]
id='owner:fixture'
kind='person'
[[sites]]
id='site:fixture'
publisher='owner:fixture'
deployable=false
[[sources]]
id='source:fixture'
location={kind='repository',root=${quote(source)},commit='${commit}',subdirectory='fixture'}
[[projects]]
id='project:fixture'
lifecycle='active'
visibility='public'
availability='available'
[[brands]]
id='brand:fixture'
name='Fixture publication'
[[brands.publications]]
site='site:fixture'
[brands.publications.documentation_repository]
name='fixture-domain-doc'
path=${quote(destination)}
[brands.publications.documentation_repository.build]
target={kind='mintlify',theme='mint',primary_color='#123abc'}
[[brands.publications.documentation_repository.build.modules]]
project='project:fixture'
slug='fixture'
title='Fixture library'
[[brands.publications.documentation_repository.build.modules.navigation]]
kind='reference'
slug='library'
title='API reference'
input={kind='generated',source='source:fixture',recipe={program=${quote(node)},executable={bytes=${executable.length},sha256='${sha(executable)}'},arguments=[${arguments_.join(',')}],environment={},version_arguments=[${quote(typedoc)},'--version'],expected_version_sha256='${sha(typedocVersion)}',output={kind='file',path='reference.json'},format='typedoc-json'}}
`;
  const report = JSON.parse(invoke(gazette, ['assemble-docs', '--local', '--site', 'site:fixture', '--allow-execute', '--generator-parent', jobs, '--output-parent', output, '--generator-timeout-ms', '30000', '--max-generator-jobs', '1'], root, config));
  assert.equal(report.status, 'documentation-assembled-local');
  assert.equal(report.jobs.length, 1);
  assert.equal(report.repository.name, 'fixture-domain-doc');
  assert.equal(report.repository.path, destination);
  assert.ok(!existsSync(destination), 'intended brand repository is not created');
  const job = report.jobs[0];
  assert.equal(job.evidence.version_text, typedocVersion);
  assert.match(job.evidence.version_text, /TypeDoc 0\.28\.20\b/);
  assert.equal(job.evidence.generation.exit.code, 0);
  const raw = readFileSync(join(job.output, 'reference.json'));
  assert.equal(job.content.sha256, sha(raw));
  assert.deepEqual(readFileSync(join(report.directory, 'artifacts/fixture/library.json')), raw);
  assert.equal(report.artifacts[0].content.sha256, sha(raw));
  const navigation = JSON.parse(readFileSync(join(report.directory, 'docs.json'), 'utf8'));
  assert.deepEqual(navigation.navigation.groups, [{
    group: 'Fixture library',
    pages: [{ group: 'API reference', sdk: { format: 'typedoc', source: 'artifacts/fixture/library.json', directory: 'fixture/library' } }],
  }]);
  const model = JSON.parse(raw);
  assert.equal(model.variant, 'project');
  assert.equal(model.packageVersion, '0.1.0');
  assert.ok(model.schemaVersion, 'real versioned provider output');
  const records = [];
  function walk(value) {
    if (value && typeof value === 'object') {
      if (!Array.isArray(value)) records.push(value);
      for (const child of Object.values(value)) walk(child);
    }
  }
  walk(model);
  const named = (name, kind) => {
    const matches = records.filter(row => row.name === name && row.kind === kind);
    assert.equal(matches.length, 1, `one public reflection: ${name}`);
    return matches[0];
  };
  const envelope = named('Envelope', 256);
  assert.equal(envelope.typeParameters[0].name, 'T');
  const value = envelope.children.find(row => row.name === 'value');
  assert.equal(value.type.name, 'T');
  assert.equal(value.flags.isReadonly, true);
  assert.ok(JSON.stringify(value.comment).includes('Original payload, without conversion.'));
  assert.ok(JSON.stringify(envelope.comment).includes('@example'));
  assert.ok(JSON.stringify(envelope.comment).includes('value: 42'));
  assert.equal(named('Choice', 2097152).type.types.length, 2);
  const wrap = named('wrap', 64).signatures[0];
  assert.equal(wrap.typeParameters[0].name, 'T');
  assert.equal(wrap.parameters[0].type.name, 'T');
  assert.equal(wrap.type.target, envelope.id);
  assert.ok(records.some(row => row.kind === 'inline-tag' && row.tag === '@link' && row.target === envelope.id), 'resolved comment link to public symbol');
  const overloads = named('describe', 64).signatures;
  assert.deepEqual(overloads.map(row => row.parameters[0].type.name).sort(), ['number', 'string']);
  assert.ok(overloads.every(row => row.type.name === 'string' && row.comment));
  assert.equal(model.children.filter(row => row.kind === 2).length, 2, 'explicit source modules');
  assert.ok(envelope.sources.some(row => row.fileName.endsWith('model.ts') && row.line > 0 && row.url.startsWith(`https://github.com/example/reference/blob/${commit}/`)));
  for (const file of files) {
    assert.deepEqual(readFileSync(join(sourceSelection, file)), readFileSync(join(fixtures, file)));
    assert.deepEqual(readFileSync(join(job.input, file)), readFileSync(join(fixtures, file)));
  }
  assert.equal(invoke(git, ['status', '--porcelain'], source), '');
  assert.equal(sha(readFileSync(lockPath)), expectedLock);
  assert.deepEqual(closure(), before, 'installed tool closure is unchanged');
  console.log(JSON.stringify({ status: 'native-typedoc-fixture-qualified', node: invoke(node, ['--version']).trim(), typedoc: job.evidence.version_text, lock_sha256: expectedLock, installed_closure: before, source_commit: commit, artifact_sha256: sha(raw), artifact_bytes: raw.length, schema_version: model.schemaVersion, proof: 'Actual configured source capture, TypeDoc generation, exact Mintlify artifact packaging and local navigation; not arbitrary TypeScript coverage, hosted rendering, public admission or deployment' }, null, 2));
  success = true;
} finally {
  if (success) rmSync(root, { recursive: true, force: true });
  else console.error(`Qualification failed; private native residue retained at ${root}; no automatic retry.`);
}
