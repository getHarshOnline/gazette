// Exact native catalog -> the same binding used by the VS Code controller.
const assert = require('node:assert/strict');
const { isAbsolute } = require('node:path');
const { ApplicationSession } = require('../dist/applicationSession.js');
const { bindOperationRequest } = require('../dist/applicationCommands.js');
const applicationCommandCatalog = require('../dist/applicationCommandCatalog.json');

async function run() {
  const executable = process.env.GAZETTE_TEST_EXECUTABLE;
  assert.ok(executable && isAbsolute(executable), 'exact Gazette executable required');
  const session = new ApplicationSession(executable, process.cwd());
  try {
    const catalog = await session.discover();
    const pages = JSON.parse(catalog.raw);
    assert.ok(Array.isArray(pages) && pages.length > 1, 'actual native capability discovery is paginated');
    for (const [index, page] of pages.entries()) {
      assert.ok(page.result.tools.length > 0 && page.result.tools.length <= 8);
      assert.equal(page.result.resultType, 'complete');
      if (index < pages.length - 1) {
        assert.equal(page.result.tools.length, 8);
        assert.equal(typeof page.result.nextCursor, 'string');
      } else assert.equal(page.result.nextCursor, undefined);
    }
    const names = new Set(catalog.tools.map(tool => tool.name));
    assert.equal(names.size, catalog.tools.length, 'native catalog names are unique');
    assert.ok(names.size > 0, 'native catalog is nonempty');
    for (const route of applicationCommandCatalog) {
      assert.ok(route.tools.length > 0, `${route.cli} has no native operation`);
      for (const name of route.tools) assert.ok(names.has(name), `${route.cli} maps to absent native tool ${name}`);
    }
    const review = catalog.tools.find(tool => tool.name === 'review_publication');
    assert.ok(review, 'publication review remains in the live catalog');
    assert.equal(review._meta['io.rtepl.gazette/intent'], 'inspect');
    assert.equal(review._meta['io.rtepl.gazette/effect'], 'read');
    assert.equal(review.annotations.readOnlyHint, true);
    const effectProfile = name => {
      const tool = catalog.tools.find(candidate => candidate.name === name);
      assert.ok(tool, `${name} remains in the live catalog`);
      return [
        tool._meta['io.rtepl.gazette/effect'],
        tool._meta['io.rtepl.gazette/potentialEffects'],
      ];
    };
    assert.deepEqual(effectProfile('assemble_docs'),
      ['local', ['filesystem-read', 'filesystem-write', 'sqlite', 'process']]);
    assert.deepEqual(effectProfile('load_workspace'), ['read', ['filesystem-read']]);
    assert.deepEqual(effectProfile('start_preview'),
      ['local', ['memory-state', 'filesystem-read', 'process', 'listener']]);
    assert.deepEqual(effectProfile('inspect_store'), ['local', ['filesystem-read', 'sqlite']]);
    assert.deepEqual(effectProfile('prepare_release_commit'),
      ['local', ['filesystem-read', 'filesystem-write', 'git']]);
    assert.deepEqual(effectProfile('observe_github_pages_build'),
      ['external', ['filesystem-read', 'process', 'network']]);
    assert.deepEqual(effectProfile('authoring_diagnostics'), ['read', []]);
    for (const name of names) {
      const binding = bindOperationRequest(catalog.tools, name);
      assert.equal(binding.tool.name, name);
      const request = JSON.parse(binding.text);
      assert.ok(request && typeof request === 'object' && !Array.isArray(request));
    }
    console.log(`Gazette VS Code catalog coverage: ${names.size} live tools bound without a copied catalog.`);
  } finally {
    await session.shutdown();
  }
}

run().catch(error => {
  console.error(error?.stack ?? error);
  process.exitCode = 1;
});
