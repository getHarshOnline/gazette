// Actual addon -> shared native preparation; the helper is test-only Rust read-back, not a CLI bridge.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 4, 'supply exact addon and preparation_reference paths');
const [, , addon, referenceExecutable] = process.argv;
assert.ok(isAbsolute(referenceExecutable));
const gitExecutable = process.env.GAZETTE_TEST_GIT;
assert.ok(gitExecutable && isAbsolute(gitExecutable), 'GAZETTE_TEST_GIT must select an absolute Git executable');
const api = loadGazette(addon);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const allowed = {
  local: true, allowRead: true, createStage: true, recordPreparation: true,
  advanceWriterGeneration: true, reserveLocalCoordination: true, initializeWorktree: true,
  limitsJson: '{"read":{"allow_coordination_files":true}}', resultBytes: 8 * 1024 * 1024,
};
const native = request => JSON.parse(execFileSync(referenceExecutable, [], {
  input: JSON.stringify(request), encoding: 'utf8', maxBuffer: 32 * 1024 * 1024,
}));
function failure(call, code) {
  let caught;
  try { call(); } catch (error) { caught = error; }
  assert.ok(caught instanceof Error, 'failure cannot become partial success');
  const value = JSON.parse(caught.message);
  if (code) assert.deepEqual(value, { code });
  return value;
}

function sourceFixture(root) {
  const files = {};
  for (const site of ['one', 'other']) {
    files[`${site}/page.md`] = `+++\n[content]\nid='content:${site}'\nsite='site:${site}'\ntitle='${site}'\nlifecycle='published'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\nExact ${site}.\n`;
    files[`theme-${site}/theme.toml`] = `package='theme:${site}'\nnamespace='${site}'\ntemplates='templates'\nrender={page='page.html'}\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\n`;
    files[`theme-${site}/templates/page.html`] = '<article>{{ content.title }}{{ body_html | safe }}</article>';
  }
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  }
  const git = args => execFileSync(gitExecutable, ['-c', 'core.hooksPath=/dev/null', ...args], {
    cwd: root, encoding: 'utf8', env: { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null',
      GIT_AUTHOR_DATE: '2000-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2000-01-01T00:00:00Z' },
  });
  git(['init', '--quiet']); git(['add', '--all']);
  git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgSign=false', 'commit', '--quiet', '-m', 'exact fixture']);
  const commit = git(['rev-parse', 'HEAD']).trim();
  let workspace = "[[principals]]\nid='owner:one'\nkind='person'\n";
  for (const site of ['one', 'other']) {
    workspace += `[[sites]]\nid='site:${site}'\npublisher='owner:one'\ndeployable=false\n[sites.composition]\ntheme={package='theme:${site}'}\n`;
  }
  workspace += "[[brands]]\nid='brand:one'\nname='Private fixture'\n";
  for (const site of ['one', 'other']) workspace += `[[brands.publications]]\nsite='site:${site}'\npublic_base_url='https://${site}.example/'\nauthored=[{source='source:${site}',target='${site}'}]\n`;
  for (const key of ['one', 'other', 'theme-one', 'theme-other']) {
    workspace += `[[sources]]\nid='source:${key}'\n`;
    workspace += `location={kind='repository',root=${JSON.stringify(root)},commit='${commit}',subdirectory='${key}'}\n`;
    if (key.startsWith('theme-')) {
      const site = key.slice('theme-'.length);
      workspace += `[[theme_packages]]\npackage='theme:${site}'\nsource='source:${key}'\nmanifest='theme.toml'\n`;
    }
  }
  return workspace;
}

function recipe(workspace, journal, parent) {
  const signature = { name: 'Fixture', email: 'fixture@example.invalid', seconds: 100, offset_minutes: 330 };
  const rules = Object.fromEntries([
    ['local-path', { kind: 'path-tree', root: '/private/do-not-publish' }],
    ['private-endpoint', { kind: 'host', name: 'private.internal.invalid' }],
    ...['secret', 'generated-artifact', 'fixture-claim', 'internal-identifier'].map(name => [name, { kind: 'literal', value: `never-publish-${name}-982731` }]),
  ].map(([classification, pattern]) => [`rule:${classification}`, { class: classification, pattern }]));
  return {
    workspace: { kind: 'supplied', source: workspace },
    journal: journal.directory, expected_header: journal.header, coordinator: null,
    selection: { content: { 'content:one': 'include', 'content:other': 'include' }, assets: {} },
    disclosure: { rules }, candidate_check: null,
    sites: ['other', 'one'].map(site => ({
      render: { site: `site:${site}`,   model_output: 'model.json', graph: null },
      stage_parent: parent, stage_name: site, preparation_id: sha256(`site:${site}`),
      metadata: { parent: null, author: signature, committer: signature, message: 'Exact retained publication\n' }, parent_source: null,
    })),
  };
}

test('Node preparation preserves complete native effects and custody', async t => {
  const root = mkdtempSync(join(tmpdir(), 'gazette-node-preparation-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = join(root, 'source'); mkdirSync(source);
  const workspace = sourceFixture(source);
  let sequence = 0;
  const setup = () => {
    const directory = join(root, `journal-${sequence}`), parent = join(root, `stage-${sequence++}`);
    mkdirSync(directory, { mode: 0o700 }); mkdirSync(parent, { mode: 0o700 });
    const journal = { ...native({ operation: 'initialize', journal: directory }), directory };
    return { journal, parent, request: recipe(workspace, journal, parent) };
  };
  const prepare = (request, extra = {}) => api.prepareSites(Buffer.from(JSON.stringify(request)), { ...allowed, ...extra });
  const inspect = ({ journal, request }) => native({ operation: 'inspect', database: journal.database, source, ids: request.sites.map(site => site.preparation_id) });
  const unchanged = fixture => {
    const observed = inspect(fixture);
    assert.deepEqual(observed.header, fixture.journal.header);
    assert.deepEqual(observed.records, [null, null]);
    assert.deepEqual(readdirSync(fixture.parent), []);
  };

  await t.test('two sites reach native staging, journal and Git read-back through the addon', () => {
    const fixture = setup();
    const report = JSON.parse(prepare(fixture.request));
    assert.equal(report.status, 'local-preparations-complete-not-certified');
    assert.deepEqual(report.sites.map(site => site.site), ['site:other', 'site:one']);
    const observed = inspect(fixture);
    assert.deepEqual(observed.records, report.sites.map(site => site.preparation.record));
    for (const site of ['other', 'one']) {
      assert.equal(readFileSync(join(fixture.parent, site, 'guide/index.html'), 'utf8'), `<article>${site}<p>Exact ${site}.</p>\n</article>`);
      const model = JSON.parse(readFileSync(join(fixture.parent, site, 'model.json')));
      assert.equal(model.pages[`content:${site}`].content.title, site);
    }
    const direct = setup();
    const expected = native({ operation: 'prepare', recipe: direct.request });
    assert.equal(expected.status, report.status);
    inspect(direct);
    const identities = result => result.sites.map(site => ({ output: site.preparation.record.intent.output, commit: site.preparation.record.worktree.completion.report.commit }));
    assert.deepEqual(identities(report), identities(expected), 'native content and commits agree independently of custody paths');
  });

  await t.test('denied effects, strict recipes and impossible delivery budget do not start preparation', () => {
    const fixture = setup();
    for (const flag of ['local', 'allowRead']) failure(() => prepare(fixture.request, { [flag]: false }), 'publication_permission');
    for (const flag of ['createStage', 'recordPreparation', 'advanceWriterGeneration', 'reserveLocalCoordination', 'initializeWorktree']) {
      const result = failure(() => prepare(fixture.request, { [flag]: false }));
      assert.equal(result.status, 'not-started'); assert.equal(result.code, 'local_preparation_preflight');
    }
    const denied = failure(() => prepare(fixture.request, { limitsJson: '{}' }));
    assert.equal(denied.status, 'not-started');
    failure(() => api.prepareSites(Buffer.from(JSON.stringify(fixture.request)), { local: true, allowRead: true }));
    for (const bytes of ['[]', 'null', 'private-invalid-json', JSON.stringify({ ...fixture.request, unknown: true }), JSON.stringify(fixture.request).replace('"workspace":', '"workspace":"old","worksp\\u0061ce":')]) {
      failure(() => api.prepareSites(Buffer.from(bytes), allowed), 'local_preparation_input');
    }
    failure(() => prepare(fixture.request, { resultBytes: 1 }), 'node_result_limit');
    failure(() => prepare(fixture.request, { inputBytes: 1 }), 'node_input_limit');
    failure(() => prepare(fixture.request, { resultBytes: 0.5 }), 'node_limits');
    for (const limitsJson of ['{"read":[]}', '{"read":null}', '{"read":{"allow_coordination_files":true,"allow_coordination_files":false}}', '{"create_stage":true}']) {
      failure(() => prepare(fixture.request, { limitsJson }), 'node_query');
    }
    unchanged(fixture);
  });

  await t.test('second-site failure delivers completed prefix and newer durable intent', () => {
    const fixture = setup();
    writeFileSync(join(fixture.parent, 'one'), 'keep this sentinel');
    const report = failure(() => prepare(fixture.request));
    assert.equal(report.status, 'incomplete');
    assert.deepEqual(report.completed.map(site => site.site), ['site:other']);
    assert.equal(report.failure.site, 'site:one');
    assert.equal(report.failure.phase.kind, 'stage');
    const observed = inspect(fixture);
    assert.deepEqual(observed.records[0], report.completed[0].preparation.record);
    assert.deepEqual(observed.records[1], report.failure.phase.evidence.receipt.record);
    assert.ok(observed.header.writer_fence > report.completed[0].preparation.journal.writer_fence);
    assert.equal(readFileSync(join(fixture.parent, 'one'), 'utf8'), 'keep this sentinel');
  });

  await t.test('post-effect output overflow retains original coordinates, not invented current state', () => {
    const fixture = setup();
    // Enough for the complete bounded original-coordinate envelope, below two full native receipts.
    const report = failure(() => prepare(fixture.request, { resultBytes: 4096 }));
    assert.equal(report.status, 'reconciliation-required');
    assert.equal(report.journal, fixture.journal.directory);
    assert.deepEqual(report.input_expected_header, fixture.journal.header);
    assert.equal(report.coordinator, null);
    assert.deepEqual(report.preparations, fixture.request.sites.map(({ render, preparation_id }) => ({ site: render.site, preparation_id })));
    const observed = inspect(fixture);
    assert.ok(observed.records.every(record => record.worktree.completion));
    assert.ok(observed.header.writer_fence > report.input_expected_header.writer_fence);
    assert.ok(Buffer.byteLength(JSON.stringify(report)) <= 4096);
    assert.ok(!Object.hasOwn(report, 'sites'), 'never turn overflow into a partial successful set');
  });
});
