// Explicit local addon + direct Rust SDK reference; no CLI bridge or installation.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, lstatSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';
import { loadGazette } from '../index.mjs';

assert.equal(process.argv.length, 4, 'supply exact addon and publication_reference executable paths');
const [, , addon, referenceExecutable] = process.argv;
assert.ok(isAbsolute(referenceExecutable), 'reference executable selection is explicit');
const gitExecutable = process.env.GAZETTE_TEST_GIT;
assert.ok(gitExecutable && isAbsolute(gitExecutable), 'GAZETTE_TEST_GIT must select an absolute Git executable');
const api = loadGazette(addon);
const allowed = { local: true, allowRead: true };
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

function failure(call, code) {
  let caught;
  try { call(); } catch (error) { caught = error; }
  assert.ok(caught instanceof Error, 'a refusal must not return a partial success');
  const value = JSON.parse(caught.message);
  if (code !== undefined) assert.deepEqual(value, { code }, 'early errors disclose only their class');
  return value;
}

function reference(requests, withFiles = false, args = []) {
  const results = JSON.parse(execFileSync(referenceExecutable, args, {
    input: JSON.stringify(requests), encoding: 'utf8', maxBuffer: 64 * 1024 * 1024,
  }));
  return withFiles ? results : results.map(result => JSON.parse(result.report));
}

function snapshot(root) {
  const records = [];
  function visit(relative) {
    const path = join(root, relative);
    const metadata = lstatSync(path);
    assert.ok(!metadata.isSymbolicLink(), 'fixture has no ambient links');
    records.push([relative, metadata.mode, metadata.isDirectory() ? null : sha256(readFileSync(path))]);
    if (metadata.isDirectory()) {
      for (const name of readdirSync(path).sort()) visit(relative ? `${relative}/${name}` : name);
    }
  }
  visit('');
  return records;
}

function fixture(root) {
  const page = (site, title, lifecycle, related) => `+++\nrelated=['content:${related}']\n[content]\nid='content:${site}'\nsite='site:${site}'\ntitle='${title}'\nlifecycle='${lifecycle}'\nroute={path='/guide/'}\nstructure={kind='page',family={kind='article'}}\n+++\nRetained **${site}**.\n`;
  const files = {
    'one/page.md': page('one', 'One', 'draft', 'other'),
    'other/page.md': page('other', 'Other', 'archived', 'one'),
    'one/download.zip': Buffer.from([0x50, 0x4b, 0x03, 0x04, 0xff, 0, 0x80, 0x7b]),
  };
  for (const site of ['one', 'other']) {
    files[`theme-${site}/theme.toml`] = `package='theme:${site}'\nnamespace='${site}'\ntemplates='templates'\nrender={page='page.html'}\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\n`;
    files[`theme-${site}/templates/page.html`] = `<article data-theme="${site}"><h1>{{ content.title }}</h1>{{ body_html | safe }}</article>`;
  }
  for (const [relative, text] of Object.entries(files)) {
    mkdirSync(dirname(join(root, relative)), { recursive: true });
    writeFileSync(join(root, relative), text);
  }
  const git = args => execFileSync(gitExecutable, ['-c', 'core.hooksPath=/dev/null', ...args], {
    cwd: root, encoding: 'utf8',
    env: { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null',
      GIT_AUTHOR_DATE: '2000-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2000-01-01T00:00:00Z' },
  });
  git(['init', '--quiet']);
  git(['add', '--all']);
  git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
    '-c', 'commit.gpgSign=false', 'commit', '--quiet', '-m', 'synthetic retained inputs']);
  const commit = git(['rev-parse', 'HEAD']).trim();
  let workspace = `[composition]\ntheme={package='theme:one'}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=false\n[sites.composition]\ntheme={package='theme:other'}\n[[brands]]\nid='brand:one'\nname='Private fixture'\n`;
  for (const site of ['one', 'other']) {
    workspace += `[[brands.publications]]\nsite='site:${site}'\npublic_base_url='https://${site}.example/mount/'\nauthored=[{source='source:${site}',target='${site}',static_assets={route_prefix='/media/'}}]\n`;
  }
  for (const source of ['one', 'other', 'theme-one', 'theme-other']) {
    workspace += `[[sources]]\nid='source:${source}'\n`;
    workspace += `location={kind='repository',root=${JSON.stringify(root)},commit='${commit}',subdirectory='${source}'}\n`;
    if (source.startsWith('theme-')) {
      const site = source.slice('theme-'.length);
      workspace += `[[theme_packages]]\npackage='theme:${site}'\nsource='source:${source}'\nmanifest='theme.toml'\n`;
    }
  }
  const recipe = (sites = ['one', 'other']) => ({
    format: 'gazette-local-publication-set/1', workspace: { kind: 'supplied', source: workspace },
    sites: sites.map(site => ({ site: `site:${site}`,  model_output: 'model.json' })),
  });
  return { recipe, workspace, files, commit };
}

test('real Node local publication path shares the native Rust operation and refusal boundaries', async t => {
  const root = mkdtempSync(join(tmpdir(), 'gazette-node-publication-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const cwd = process.cwd();
  const { recipe, workspace, files, commit } = fixture(root);
  const original = snapshot(root);
  const multiple = JSON.stringify(recipe());
  const single = JSON.stringify(recipe(['one']));
  const other = JSON.stringify(recipe(['other']));
  const reversed = JSON.stringify(recipe(['other', 'one']));
  const missing = recipe();
  missing.sites[0].model_output = 'guide/index.html';
  const absentTemplate = JSON.stringify(missing);
  const nativeFiles = reference([multiple, single, other, reversed, absentTemplate], true);
  const native = nativeFiles.map(result => JSON.parse(result.report));
  const render = (request, options = {}) => api.renderSites(Buffer.from(request), { ...allowed, ...options });
  const renderFiles = (request, options = {}) => api.renderSiteFiles(Buffer.from(request), { ...allowed, ...options });
  const review = (site = 'site:one', options = {}, source = workspace) =>
    api.reviewPublication(Buffer.from(source), site, { allowRead: true, ...options });

  await t.test('private review matches exact Rust projections without rendering or granting approval', () => {
    const expected = reference([single, other], true, ['--review']);
    for (const [index, site] of ['one', 'other'].entries()) {
      const text = review(`site:${site}`);
      assert.equal(text, expected[index].report, 'same exact native JSON, not a JS reconstruction');
      assert.equal(review(`site:${site}`), text);
      const value = JSON.parse(text);
      assert.equal(value.sha256, sha256(JSON.stringify(value.request)));
      assert.equal(value.request.configuration_sha256, sha256(workspace));
      assert.equal(value.request.site, `site:${site}`);
      assert.equal(value.request.selection_sha256, null, 'private review is not public admission');
      const pages = value.request.surfaces.filter(surface => surface.target.kind === 'page');
      assert.equal(pages.length, 1, 'foreign relationships do not create home-site pages');
      assert.equal(pages[0].value.content.lifecycle, site === 'one' ? 'draft' : 'archived');
      assert.equal(pages[0].value.body_html, `<p>Retained <strong>${site}</strong>.</p>\n`);
      assert.ok(!pages[0].value.body_html.includes('<article'), 'Theme programs are not run');
      assert.ok(value.request.surfaces.some(surface => surface.target.kind === 'site-graph'));
      assert.equal(value.request.surfaces.filter(surface => surface.target.kind === 'asset').length, site === 'one' ? 1 : 0);
    }
    assert.deepEqual(snapshot(root), original, 'review leaves inputs, Git, and output locations untouched');
  });

  await t.test('private review permission, complete bounds and source refusals never return partial success', () => {
    failure(() => api.reviewPublication(Buffer.from([0xff]), 'bad site', {allowRead:false}), 'publication_permission');
    failure(() => api.reviewPublication(Buffer.from([0xff]), 'site:one', {allowRead:true}), 'node_source_utf8');
    failure(() => review('bad site'), 'publication_site');
    failure(() => review('site:missing'), 'workspace_site');
    const result = review();
    assert.equal(review('site:one', {inputBytes:Buffer.byteLength(workspace),resultBytes:Buffer.byteLength(result)}), result);
    failure(() => review('site:one', {inputBytes:1}), 'node_input_limit');
    failure(() => review('site:one', {resultBytes:Buffer.byteLength(result)-1}), 'node_result_limit');
    failure(() => review('site:one', {limitsJson:'{}',limitsBytes:1}), 'node_query_limit');
    for (const limitsJson of ['[]', 'null', '{"review":[]}', '{"corpus":{"document":{"markdown":null}}}',
      '{"review":{"uses":1,"us\\u0065s":2}}', '{"private-canary":true}']) {
      failure(() => review('site:one', {limitsJson}), 'node_query');
    }
    failure(() => review('site:one', {resultBytes:0.5}), 'node_limits');
    failure(() => review('site:one', {limitsJson:'{"review":{"uses":0}}'}), 'toml_limit');
    writeFileSync(join(root, 'one/page.md'), files['one/page.md'].replace('Retained', 'Changed'));
    const drifted = snapshot(root);
    failure(() => review(), 'source_content_mismatch');
    assert.deepEqual(snapshot(root), drifted, 'refusal cannot repair drift');
    writeFileSync(join(root, 'one/page.md'), files['one/page.md']);
    assert.equal(review(), result);
    assert.deepEqual(snapshot(root), original);
  });

  function checkFiles(delivered, expected) {
    assert.deepEqual(Object.keys(delivered).sort(), ['report', 'sites']);
    assert.equal(delivered.report, expected.report, 'exact native inspection bytes');
    const report = JSON.parse(delivered.report);
    assert.equal(report.status, 'local_set_rendered');
    assert.deepEqual(delivered.sites.map(site => site.site), Object.keys(report.sites));
    const projected = delivered.sites.map(site => {
      assert.deepEqual(Object.keys(site).sort(), ['files', 'site']);
      const inventory = report.sites[site.site].inventory.entries;
      const entries = inventory.filter(entry => entry.kind === 'file');
      assert.deepEqual(site.files.map(file => file.path), entries.map(entry => entry.path), 'every native file exactly once, without directories');
      assert.ok(inventory.some(entry => entry.kind === 'directory'), 'fixture exercises directory omission');
      return { site: site.site, files: site.files.map((file, index) => {
        assert.deepEqual(Object.keys(file).sort(), ['bytes', 'path']);
        assert.ok(file.bytes instanceof Uint8Array, 'binary native transport, not a text/base64 surrogate');
        assert.equal(file.bytes.byteLength, entries[index].bytes);
        assert.equal(sha256(file.bytes), entries[index].sha256);
        return { path: file.path, bytes: Array.from(file.bytes) };
      }) };
    });
    assert.deepEqual(projected, expected.sites, 'independent Rust SDK exact file bytes');
  }

  await t.test('file delivery covers single, complete multi-site and reordered sets with exact binary bytes', () => {
    for (const [index, request] of [multiple, single, other, reversed].entries()) {
      const delivered = renderFiles(request);
      checkFiles(delivered, nativeFiles[index]);
      assert.equal(delivered.report, render(request), 'report and file APIs converge on native inspection');
      checkFiles(renderFiles(request), nativeFiles[index]);
    }
    assert.deepEqual(nativeFiles[0].sites, nativeFiles[3].sites, 'request order does not rearrange file delivery');
    const delivered = renderFiles(multiple);
    const binary = delivered.sites.find(site => site.site === 'site:one').files.find(file => file.path === 'media/download.zip');
    assert.deepEqual(Buffer.from(binary.bytes), files['one/download.zip'], 'opaque non-UTF-8 bytes remain exact; no ZIP interpretation claimed');
    assert.ok(!delivered.sites.find(site => site.site === 'site:other').files.some(file => file.path === 'media/download.zip'));
  });

  await t.test('host input and returned buffers cannot mutate retained output or future operations', () => {
    const input = new Uint8Array(Buffer.from(multiple));
    const delivered = api.renderSiteFiles(input, allowed);
    input.fill(0);
    checkFiles(delivered, nativeFiles[0]);
    const separate = renderFiles(multiple);
    const victim = delivered.sites[0].files[0];
    victim.bytes.fill(0x55);
    victim.path = 'host-mutated-path';
    delivered.sites.reverse();
    delivered.report = 'host mutation';
    checkFiles(separate, nativeFiles[0]);
    checkFiles(renderFiles(multiple), nativeFiles[0]);
    assert.deepEqual(snapshot(root), original, 'delivery creates no native stage or modified input');
  });

  await t.test('file delivery refuses the entire requested set when a required sibling fails', () => {
    const failed = failure(() => renderFiles(absentTemplate));
    assert.deepEqual(failed, failure(() => render(absentTemplate)));
    assert.deepEqual(failed, native[4]);
    assert.equal(failed.status, 'local_set_incomplete');
    assert.equal(failed.sites['site:other'].status, 'rendered', 'healthy sibling inspection remains evidence, not partial file delivery');
    assert.ok(!Object.hasOwn(nativeFiles[4], 'sites'), 'direct Rust reference also withholds incomplete output');
    assert.ok(!Object.hasOwn(failed, 'files'));
  });

  await t.test('file delivery preserves permission and independent native aggregate and report bounds', () => {
    for (const permissions of [{ local: false, allowRead: true }, { local: true, allowRead: false }]) {
      failure(() => api.renderSiteFiles(Buffer.from('private-canary'), permissions), 'publication_permission');
    }
    failure(() => api.renderSiteFiles(new Uint8Array([0xff]), allowed), 'publication_set_input');
    const delivered = renderFiles(multiple);
    const totals = Object.values(native[0].sites).reduce((sum, site) => ({
      bytes: sum.bytes + site.inventory.entries.filter(entry => entry.kind === 'file').reduce((bytes, entry) => bytes + entry.bytes, 0),
      entries: sum.entries + site.inventory.entries.length,
    }), { bytes: 0, entries: 0 });
    const bounds = { max_total_output_bytes: totals.bytes, max_total_output_entries: totals.entries };
    checkFiles(renderFiles(multiple, { limitsJson: JSON.stringify(bounds), resultBytes: Buffer.byteLength(delivered.report) }), nativeFiles[0]);
    failure(() => renderFiles(multiple, { resultBytes: Buffer.byteLength(delivered.report) - 1 }), 'node_result_limit');
    failure(() => renderFiles(absentTemplate, { resultBytes: 1 }), 'node_result_limit');
    failure(() => renderFiles(multiple, { inputBytes: Buffer.byteLength(multiple) - 1 }), 'node_input_limit');
    for (const key of Object.keys(bounds)) {
      const limitsJson = JSON.stringify({ ...bounds, [key]: bounds[key] - 1 });
      const refused = failure(() => renderFiles(multiple, { limitsJson }));
      assert.deepEqual(refused, failure(() => render(multiple, { limitsJson })));
      assert.equal(refused.status, 'local_set_incomplete');
      assert.ok(Object.values(refused.sites).some(site => site.status === 'failed'));
    }
    for (const options of [{ resultBytes: -1 }, { limitsJson: '[]' }, { limitsJson: '{"output":[]}' }]) {
      assert.deepEqual(failure(() => renderFiles(multiple, options)), failure(() => render(multiple, options)));
    }
    checkFiles(renderFiles(multiple), nativeFiles[0]);
  });

  await t.test('complete single and two-site reports retain exact distinct rendered outputs', () => {
    for (const [index, request] of [multiple, single, other, reversed].entries()) {
      const text = render(request);
      assert.equal(typeof text, 'string');
      assert.deepEqual(JSON.parse(text), native[index]);
      assert.equal(render(request), text, 'repeat capture and rendering are deterministic');
    }
    const report = native[0];
    assert.equal(report.status, 'local_set_rendered');
    assert.deepEqual(Object.keys(report.sites), ['site:one', 'site:other']);
    assert.equal(report.build_selection.origin, 'invocation');
    assert.deepEqual(report.build_selection.sites, ['site:one', 'site:other']);
    assert.deepEqual(native[3].build_selection.sites, ['site:other', 'site:one']);
    assert.deepEqual(
      { ...report, build_selection: { ...report.build_selection, sites: native[3].build_selection.sites } },
      native[3],
      'only the recorded invocation order changes; all remaining receipt evidence stays exact',
    );
    assert.deepEqual(report.sites['site:one'], native[1].sites['site:one']);
    assert.deepEqual(report.sites['site:other'], native[2].sites['site:other']);
    assert.deepEqual(Object.keys(native[1].sites), ['site:one'], 'foreign relations do not auto-render a site');
    assert.deepEqual(Object.keys(report.sources), ['source:one', 'source:other', 'source:theme-one', 'source:theme-other']);
    for (const [key, source] of Object.entries(report.sources)) {
      const subtree = key.slice('source:'.length);
      assert.equal(source.kind, 'repository');
      assert.equal(source.verification.commit, commit);
      assert.equal(source.verification.subdirectory, subtree);
      const capturedFiles = source.verification.inventory.entries.filter(entry => entry.kind === 'file');
      const expectedFiles = Object.entries(files)
        .filter(([path]) => path.startsWith(`${subtree}/`))
        .map(([path, text]) => ({ path: path.slice(subtree.length + 1), kind: 'file', bytes: Buffer.byteLength(text), sha256: sha256(text) }))
        .sort((left, right) => left.path.localeCompare(right.path));
      assert.deepEqual(capturedFiles, expectedFiles, 'source receipts cover exact independently supplied bytes');
    }
    for (const [site, title] of [['one', 'One'], ['other', 'Other']]) {
      const inventory = report.sites[`site:${site}`].inventory;
      const html = inventory.entries.find(entry => entry.path === 'guide/index.html');
      const expected = `<article data-theme="${site}"><h1>${title}</h1><p>Retained <strong>${site}</strong>.</p>\n</article>`;
      assert.deepEqual(html, { path: 'guide/index.html', kind: 'file', bytes: Buffer.byteLength(expected), sha256: sha256(expected) });
      assert.ok(inventory.entries.some(entry => entry.path === 'model.json' && entry.kind === 'file'));
      assert.equal(report.sites[`site:${site}`].status, 'rendered', 'private draft/archived data is not silently admitted or filtered');
    }
    const input = new Uint8Array(Buffer.from(multiple));
    const retained = api.renderSites(input, allowed);
    input.fill(0);
    assert.deepEqual(JSON.parse(retained), report, 'completed native evidence does not borrow mutable host bytes');
  });

  await t.test('first-site failure throws the full native report and preserves the healthy sibling', () => {
    const failed = failure(() => render(absentTemplate));
    assert.deepEqual(failed, native[4]);
    assert.equal(failed.status, 'local_set_incomplete');
    assert.deepEqual(Object.keys(failed.sites), ['site:one', 'site:other']);
    assert.equal(failed.sites['site:one'].status, 'failed');
    assert.ok(failed.sites['site:one'].code);
    assert.deepEqual(failed.sites['site:other'], native[0].sites['site:other']);
    assert.ok(!Object.hasOwn(failed, 'error'), 'incomplete native inspection is not wrapped');
  });

  await t.test('permission precedes parsing, while strict recipes reject private malformed input', () => {
    for (const permissions of [{ local: false, allowRead: true }, { local: true, allowRead: false }]) {
      failure(() => api.renderSites(Buffer.from('private-canary'), permissions), 'publication_permission');
    }
    const malformed = [
      'private-canary', 'null', '[]', JSON.stringify([recipe().format, recipe().workspace, recipe().sites]),
      JSON.stringify({ ...recipe(), sites: [] }),
      JSON.stringify({ ...recipe(), sites: [recipe().sites[0], recipe().sites[0]] }),
      JSON.stringify({ ...recipe(), sites: [['site:one', 'page.html', null, 'model.json', null]] }),
      JSON.stringify({ ...recipe(), sites: [{ ...recipe().sites[0], graph: ['graph.json', 'graph.json'] }] }),
      JSON.stringify({ ...recipe(), selection: null }),
      JSON.stringify({ ...recipe(), unknown: 'private-canary' }),
      multiple.replace('"workspace":', '"workspace":"private-canary","worksp\\u0061ce":'),
      multiple + ' trailing',
    ];
    const rejected = reference(malformed);
    for (const [index, request] of malformed.entries()) {
      assert.deepEqual(failure(() => render(request), 'publication_set_input'), rejected[index]);
    }
    failure(() => api.renderSites(new Uint8Array([0xff]), allowed), 'publication_set_input');
    const unknown = recipe(['missing']);
    assert.deepEqual(failure(() => render(JSON.stringify(unknown)), 'publication_set_selection'), reference([JSON.stringify(unknown)])[0]);
    assert.deepEqual(snapshot(root), original);
  });

  await t.test('host and native ceilings are independently enforced without truncated reports', () => {
    const result = render(multiple);
    assert.equal(render(multiple, { inputBytes: Buffer.byteLength(multiple), resultBytes: Buffer.byteLength(result) }), result);
    failure(() => render(multiple, { inputBytes: Buffer.byteLength(multiple) - 1 }), 'node_input_limit');
    failure(() => render(multiple, { resultBytes: Buffer.byteLength(result) - 1 }), 'node_result_limit');
    failure(() => render(absentTemplate, { resultBytes: 1 }), 'node_result_limit');
    for (const value of [-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
      for (const key of ['inputBytes', 'resultBytes', 'limitsBytes']) {
        failure(() => render(multiple, { [key]: value }), 'node_limits');
      }
    }
    assert.equal(render(multiple, { limitsJson: '{}', limitsBytes: 2 }), result);
    failure(() => render(multiple, { limitsJson: '{}', limitsBytes: 1 }), 'node_query_limit');
    for (const limitsJson of ['[]', 'null', '{"private-canary":1}', '{"syntax":[]}',
      '{"acquisition":[]}', '{"output":[]}', '{"acquisition":{"sources":[]}}',
      '{"max_total_output_bytes":-1}', '{"max_total_output_entries":0.5}',
      '{"max_total_output_entries":1,"max_total_output_entr\\u0069es":2}']) {
      failure(() => render(multiple, { limitsJson }), 'node_query');
    }
    for (const field of ['max_total_output_bytes', 'max_total_output_entries']) {
      const limited = failure(() => render(multiple, { limitsJson: JSON.stringify({ [field]: 0 }) }));
      assert.equal(limited.status, 'local_set_incomplete');
      assert.deepEqual(Object.keys(limited.sites), ['site:one', 'site:other']);
      for (const outcome of Object.values(limited.sites)) {
        assert.equal(outcome.status, 'failed');
        assert.ok(!Object.hasOwn(outcome, 'inventory'));
      }
    }
    failure(() => render(multiple, { limitsJson: '{"acquisition":{"sites":1}}' }), 'publication_set_input');
    failure(() => render(multiple, { limitsJson: '{"syntax":{"nodes":0}}' }), 'toml_limit');
    assert.equal(render(multiple), result, 'failure does not contaminate the next operation');
  });

  await t.test('source drift refuses pinned content and cannot reuse an earlier successful render', () => {
    assert.deepEqual(snapshot(root), original, 'all successful/failed operations leave source, Git state and directories unchanged');
    assert.equal(process.cwd(), cwd);
    writeFileSync(join(root, 'one/page.md'), files['one/page.md'].replace('Retained', 'Changed'));
    const drifted = snapshot(root);
    const expected = reference([multiple])[0];
    assert.deepEqual(failure(() => render(multiple), 'source_content_mismatch'), expected);
    assert.deepEqual(snapshot(root), drifted, 'refusal does not repair the source');
    writeFileSync(join(root, 'one/page.md'), files['one/page.md']);
    assert.deepEqual(JSON.parse(render(multiple)), native[0], 'fresh exact bytes restore the deterministic native result');
    const wrongPin = recipe();
    const differentCommit = `${commit[0] === '0' ? '1' : '0'}${commit.slice(1)}`;
    wrongPin.workspace.source = wrongPin.workspace.source.replaceAll(commit, differentCommit);
    const request = JSON.stringify(wrongPin);
    assert.deepEqual(failure(() => render(request), 'source_revision_mismatch'), reference([request])[0]);
    assert.deepEqual(snapshot(root), original);
    assert.equal(process.cwd(), cwd);
  });
});
