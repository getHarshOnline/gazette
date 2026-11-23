import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { unEscapeChars } from '../provider.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const source = await readFile(new URL('./contracts.ts', import.meta.url), 'utf8');
const cli = path.join(root, 'node_modules/typedoc/bin/typedoc');
const markdown = path.join(root, 'node_modules/typedoc-plugin-markdown/dist/index.js');
const plugin = path.join(root, 'plugin.mjs');

// Read the rendered signature, independently of Markdown's inline/code-block decoration.
function signature(text) {
  return text.replace(/`/g, '').replace(/\*\*/g, '').replace(/\\([!-/:-@[-`{-~])/g, '$1')
    .replace(/\[([^\]]+)\]\([^\s)]+\)/g, '$1').replace(/\s+/g, ' ');
}

async function render(directory, patched, useCodeBlocks, presentation = {}) {
  const out = path.join(directory, `${patched ? 'contract' : 'upstream'}-${useCodeBlocks}`);
  const config = {
    name: 'Reference contract fixture', entryPoints: ['./contracts.ts'],
    tsconfig: './tsconfig.json', plugin: patched ? [markdown, plugin] : [markdown],
    theme: patched ? 'contract-markdown' : 'default', readme: 'none',
    disableGit: true, disableSources: true, treatWarningsAsErrors: true,
    fileExtension: '.md', entryFileName: 'index', router: 'member',
    expandObjects: true, expandParameters: true, useCodeBlocks, out, ...presentation,
  };
  const options = path.join(directory, 'options.json');
  await writeFile(options, JSON.stringify(config));
  const result = spawnSync(process.execPath, [cli, '--options', options], {
    cwd: directory, encoding: 'utf8', timeout: 60_000,
  });
  assert.equal(result.status, 0, result.error?.message ?? result.stdout + result.stderr);
  const raw = async name => readFile(path.join(out, name), 'utf8');
  const page = async name => signature(await raw(name));
  return {
    raw,
    map: await page('interfaces/KeyMap.md'),
    mutable: await page('interfaces/MutableMap.md'),
    contract: await page('interfaces/Contract.md'),
    variant: await page('type-aliases/Variant.md'),
    inline: await page('type-aliases/Inline.md'),
  };
}

for (const codeBlocks of [false, true]) {
  test(`actual TypeDoc keeps nested readonly contracts; code blocks ${codeBlocks}`, async () => {
    const directory = await mkdtemp(path.join(tmpdir(), 'gazette-reference-'));
    try {
      await writeFile(path.join(directory, 'contracts.ts'), source);
      await writeFile(path.join(directory, 'tsconfig.json'), JSON.stringify({
        compilerOptions: { strict: true, noEmit: true, target: 'ES2022' }, files: ['contracts.ts'],
      }));
      const before = await render(directory, false, codeBlocks);
      const after = await render(directory, true, codeBlocks);
      assert.doesNotMatch(before.map, /readonly \[key: string\]/);
      assert.doesNotMatch(before.variant, /readonly accepted/);
      assert.match(after.map, /readonly \[key: string\]: \{ value: number/);
      assert.doesNotMatch(after.mutable, /readonly/);
      assert.match(after.contract, /readonly id.*mutable: number/);
      assert.match(after.variant, /readonly accepted.*true/);
      assert.match(after.variant, /readonly accepted.*false.*readonly reason/);
      assert.match(after.variant, /payload: \{\s*readonly \[key: string\]: number/);
      assert.match(after.inline, /readonly callback\?.*value: number.*string/);
      assert.match(after.inline, /mutable: \{ value: number/);
      assert.match(after.inline, /readonly ["']?quoted\.name["']?\?: string/);
      assert.match(after.contract, /get current/);
      assert.match(after.contract, /set writable/);
      assert.doesNotMatch(after.contract, /readonly get|readonly set/);
      // The extension changes syntax, never its captured input contract.
      assert.equal(await readFile(path.join(directory, 'contracts.ts'), 'utf8'), source);
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
}

// The target may lift one plain leading H1 without throwing away title links or
// generic detail. The producer owns this normalization; declarations stay intact.
test('plain complete titles allow one target title without duplicate producer chrome', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'gazette-reference-title-'));
  try {
    await writeFile(path.join(directory, 'contracts.ts'), source);
    await writeFile(path.join(directory, 'tsconfig.json'), JSON.stringify({
      compilerOptions: { strict: true, noEmit: true, target: 'ES2022' }, files: ['contracts.ts'],
    }));
    const presentation = { hidePageHeader: true, hideBreadcrumbs: true, hidePageTitle: false };
    const before = await render(directory, false, false, presentation);
    const after = await render(directory, true, false, presentation);
    for (const name of ['index.md', 'interfaces/GenericResult.md', 'interfaces/LiteralResult.md']) {
      const original = await before.raw(name);
      const rendered = await after.raw(name);
      assert.match(rendered, /^# /);
      const oldTitle = original.split('\n')[0].slice(2);
      const newTitle = rendered.split('\n')[0].slice(2);
      assert.equal(unEscapeChars(newTitle), unEscapeChars(oldTitle));
      // Only title markup changes. All field/signature/comments/type links remain bytes.
      assert.equal(rendered.slice(rendered.indexOf('\n')), original.slice(original.indexOf('\n')));
    }
    const generic = await after.raw('interfaces/GenericResult.md');
    assert.match(generic.split('\n')[0], /GenericResult\\<R extends NamedRecord = NamedRecord\\>/);
    assert.match(generic, /\[`NamedRecord`\]\(NamedRecord\.md\)/);
    const literal = await after.raw('interfaces/LiteralResult.md');
    assert.match(unEscapeChars(literal.split('\n')[0]), /T extends "\*" \| "\[raw\]" \| item-\$\{string\} = "\*"/);
    assert.equal(await readFile(path.join(directory, 'contracts.ts'), 'utf8'), source);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
