// Focused public declaration check; no addon, network, package install or provider.
// Usage: node sdk/js/tests/error-declarations.mjs /absolute/typescript/bin/tsc
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { SearchError } from '../search/index.mjs';
import { AnalyticsError } from '../analytics/index.mjs';
import { WebMcpError } from '../webmcp/index.mjs';

const [compiler] = process.argv.slice(2);
assert.equal(process.argv.length, 3, 'Supply the exact absolute tsc script path');
assert.ok(isAbsolute(compiler), 'Compiler selection is explicit, not executable search');
const root = fileURLToPath(new URL('../', import.meta.url));
const manifest = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
const fixture = await readFile(new URL('./fixtures/error-consumer.mts', import.meta.url), 'utf8');
const temporary = await mkdtemp(join(tmpdir(), 'gazette-error-declarations-'));

try {
  // Resolve real public package type exports in an isolated strict consumer.
  // The full SDK package test separately covers its complete distributable payload.
  const installed = join(temporary, 'node_modules', '@rtepl', 'gazette');
  await mkdir(installed, { recursive: true });
  await copyFile(join(root, 'package.json'), join(installed, 'package.json'));
  for (const subpath of ['.', './search', './analytics', './webmcp']) {
    const selected = manifest.exports[subpath].types;
    assert.ok(selected.startsWith('./') && !selected.includes('..')
      && !selected.includes('\\'), 'Type export stays inside the package');
    const target = join(installed, selected);
    await mkdir(dirname(target), { recursive: true });
    await copyFile(join(root, selected), target);
  }
  await writeFile(join(temporary, 'package.json'), JSON.stringify({ private: true, type: 'module' }));
  await writeFile(join(temporary, 'consumer.mts'), fixture);
  await writeFile(join(temporary, 'tsconfig.json'), JSON.stringify({
    compilerOptions: {
      strict: true, noEmit: true, skipLibCheck: false, types: [],
      exactOptionalPropertyTypes: true, target: 'ES2022', lib: ['ES2022', 'DOM'],
      module: 'NodeNext', moduleResolution: 'NodeNext',
    },
    files: ['consumer.mts'],
  }));
  const compile = spawnSync(process.execPath, [compiler, '--project', 'tsconfig.json'], {
    cwd: temporary, encoding: 'utf8', timeout: 30_000, maxBuffer: 1 << 20,
  });
  assert.ifError(compile.error);
  assert.equal(compile.status, 0, compile.stdout + compile.stderr);

  // Check the exact values the declarations permit against the unchanged runtime.
  const search = new SearchError('search_data');
  const analytics = new AnalyticsError('analytics_environment');
  const cause = { operation: 'registration' };
  const webmcp = new WebMcpError('webmcp_registration', cause);
  for (const [error, name, code] of [
    [search, 'SearchError', 'search_data'],
    [analytics, 'AnalyticsError', 'analytics_environment'],
    [webmcp, 'WebMcpError', 'webmcp_registration'],
  ]) {
    assert.ok(error instanceof Error);
    assert.equal(error.name, name);
    assert.equal(error.message, code);
    assert.equal(error.code, code);
  }
  assert.equal(webmcp.cause, cause, 'A supplied cause is retained as the exact value');
  assert.equal(new WebMcpError('webmcp_registration', 'unavailable').cause, 'unavailable');
  assert.equal(Object.hasOwn(new WebMcpError('webmcp_unsupported'), 'cause'), false);
  assert.equal(Object.hasOwn(new WebMcpError('webmcp_unsupported', undefined), 'cause'), false);
  const objectCause = { cause: 'not unwrapped as ErrorOptions' };
  assert.equal(new WebMcpError('webmcp_registration', objectCause).cause, objectCause);
  process.stdout.write(`${JSON.stringify({
    status: 'passed', node: process.version, compiler,
    negativeTypeAssertions: (fixture.match(/@ts-expect-error/g) ?? []).length,
    requiredCodes: true, noInheritedSearchOverload: true, rawCauseRetained: true,
    runtimeConstructorsChecked: ['SearchError', 'AnalyticsError', 'WebMcpError'],
  })}\n`);
} finally {
  await rm(temporary, { recursive: true, force: true });
}
