// Real tsc -> emitted ESM -> explicitly selected native addon. No installs or CLI bridge.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, lstat, mkdir, mkdtemp, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { constants, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const [compiler, addon] = process.argv.slice(2);
assert.equal(process.argv.length, 4, 'Supply exact absolute tsc script and current .node addon paths');
assert.ok(isAbsolute(compiler) && isAbsolute(addon), 'Tools are explicit local files, not executable search');
assert.ok(Number(process.versions.node.split('.')[0]) >= 22, 'Use the declared Node host floor or newer');
const reference = JSON.parse(readFileSync(0, 'utf8'));
const root = fileURLToPath(new URL('../', import.meta.url));
const manifest = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
assert.equal(manifest.name, '@rtepl/gazette');
assert.deepEqual(manifest.exports['.'], { types: './index.d.ts', import: './index.mjs' });
assert.deepEqual(manifest.exports['./search'], { types: './search/index.d.ts', import: './search/index.mjs' });
assert.deepEqual(manifest.exports['./analytics'], { types: './analytics/index.d.ts', import: './analytics/index.mjs' });
assert.deepEqual(manifest.exports['./webmcp'], { types: './webmcp/index.d.ts', import: './webmcp/index.mjs' });
assert.equal(manifest.types, manifest.exports['.'].types);
const temporary = await mkdtemp(join(tmpdir(), 'gazette-typescript-consumer-'));

const copied = new Set();
async function copyPayload(name, installed) {
  assert.equal(typeof name, 'string', 'Package entries must be literal relative paths');
  const components = name.split('/');
  assert.ok(!isAbsolute(name) && !name.includes('\\')
    && components.every(part => part !== '' && part !== '.' && part !== '..'),
  'Package entries cannot escape their declared root');
  assert.ok(!copied.has(name), 'Declared payload entries cannot overlap');
  // Check ancestors too: lstat of a leaf alone would follow a symlinked parent.
  for (let count = 1; count < components.length; count++) {
    const ancestor = await lstat(join(root, ...components.slice(0, count)));
    assert.ok(ancestor.isDirectory() && !ancestor.isSymbolicLink(), 'Payload ancestors must be real directories');
  }
  const source = join(root, name);
  const target = join(installed, name);
  const metadata = await lstat(source);
  assert.ok(!metadata.isSymbolicLink(), 'Do not follow package payload symlinks');
  copied.add(name);
  if (metadata.isDirectory()) {
    await mkdir(target, { recursive: true });
    for (const child of (await readdir(source)).sort()) await copyPayload(`${name}/${child}`, installed);
  } else {
    assert.ok(metadata.isFile(), 'Package payload excludes sockets, devices and other special files');
    await mkdir(dirname(target), { recursive: true });
    await copyFile(source, target, constants.COPYFILE_EXCL);
    const after = await lstat(source);
    assert.ok(after.isFile() && !after.isSymbolicLink()
      && after.dev === metadata.dev && after.ino === metadata.ino
      && after.size === metadata.size && after.mtimeMs === metadata.mtimeMs,
    'Package source must remain the inspected regular file');
    assert.equal(createHash('sha256').update(await readFile(target)).digest('hex'),
      createHash('sha256').update(await readFile(source)).digest('hex'), 'Copied package bytes remain exact');
  }
}

function runCompiler(args) {
  const result = spawnSync(process.execPath, [compiler, ...args], {
    cwd: temporary, encoding: 'utf8', timeout: 30_000, maxBuffer: 1 << 20,
  });
  assert.ifError(result.error);
  return result;
}

try {
  // Copy the declared public payload into an isolated consumer: no checkout
  // symlink, parent node_modules, global type shim, npm installation or script.
  const installed = join(temporary, 'node_modules', '@rtepl', 'gazette');
  await mkdir(installed, { recursive: true });
  for (const name of ['package.json', ...manifest.files]) {
    await copyPayload(name, installed);
  }
  await writeFile(join(temporary, 'package.json'), JSON.stringify({ private: true, type: 'module' }));
  await copyFile(new URL('./fixtures/consumer.mts', import.meta.url), join(temporary, 'consumer.mts'));
  await copyFile(new URL('./fixtures/search-consumer.mts', import.meta.url), join(temporary, 'search-consumer.mts'));
  await copyFile(new URL('./fixtures/browser-consumer.mts', import.meta.url), join(temporary, 'browser-consumer.mts'));
  await writeFile(join(temporary, 'tsconfig.json'), JSON.stringify({
    compilerOptions: {
      strict: true, noUncheckedIndexedAccess: true, exactOptionalPropertyTypes: true,
      noEmitOnError: true, skipLibCheck: false, types: [], lib: ['ES2022'],
      target: 'ES2022', module: 'NodeNext', moduleResolution: 'NodeNext', outDir: 'compiled',
    },
    files: ['consumer.mts', 'search-consumer.mts'],
  }));
  const version = runCompiler(['--version']);
  assert.equal(version.status, 0, version.stdout + version.stderr);
  const accepted = runCompiler(['--project', 'tsconfig.json']);
  assert.equal(accepted.status, 0, accepted.stdout + accepted.stderr);
  // Preserve the ES2022-only native/search proof; browser bindings additionally
  // consume the standard DOM library, not an ambient Node/browser API shim.
  const browserConfig = JSON.parse(await readFile(join(temporary, 'tsconfig.json'), 'utf8'));
  browserConfig.compilerOptions.lib = ['ES2022', 'DOM'];
  browserConfig.files = ['browser-consumer.mts'];
  await writeFile(join(temporary, 'tsconfig.browser.json'), JSON.stringify(browserConfig));
  const acceptedBrowser = runCompiler(['--project', 'tsconfig.browser.json']);
  assert.equal(acceptedBrowser.status, 0, acceptedBrowser.stdout + acceptedBrowser.stderr);

  // Execute the browser subpath with the native root entry physically unavailable.
  // Its package resolution and complete source closure must not load the NAPI bridge.
  await rename(join(installed, 'index.mjs'), join(installed, 'withheld-native.mjs'));
  let browserSearch;
  let browserBindings;
  try {
    const { consumeSearch, consumeExplicitSearchFields } = await import(pathToFileURL(join(temporary, 'compiled', 'search-consumer.mjs')).href);
    browserSearch = consumeSearch();
    assert.deepEqual(consumeExplicitSearchFields(), ['description', 'route', 'notes']);
    assert.deepEqual(browserSearch, {
      title: 'Publication compiler', destination: 'https://example.test/mount/compiler/',
      size: 1, immutable: true,
    });
    const { consumeBrowserBindings } = await import(pathToFileURL(join(temporary, 'compiled', 'browser-consumer.mjs')).href);
    browserBindings = await consumeBrowserBindings();
    assert.deepEqual(browserBindings, {
      events: ['content_view', 'cross_site_navigate', 'cta_view', 'cta_click',
        'intake_start', 'subscribe_start', 'discussion_open'],
      analyticsCode: 'analytics_environment', webmcpCode: 'webmcp_unsupported', searchSize: 1,
    });
  } finally {
    await rename(join(installed, 'withheld-native.mjs'), join(installed, 'index.mjs'));
  }

  const { consume, consumePagePackage, consumePublication, consumePublicationFiles, consumeTree, consumeDocumentation, consumeReview, consumePreparation, consumeQualification, consumeDelivery, consumeHistory } = await import(pathToFileURL(join(temporary, 'compiled', 'consumer.mjs')).href);
  assert.throws(() => consumeHistory(addon, new Uint8Array(), {allowRead:false}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'storage_permission');
  assert.throws(() => consumeDelivery(addon, new Uint8Array(), {inputBytes:1024,resultBytes:1024}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'delivery_input');
  assert.throws(() => consumeQualification(addon, new Uint8Array(), {allowRead:true,allowPolicyExecution:false}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'qualification_input');
  assert.throws(() => consumePreparation(addon, new Uint8Array(), {local:true,allowRead:false}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'publication_permission');
  assert.throws(() => consumeReview(addon, new Uint8Array(), 'site:one', {allowRead:false}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'publication_permission');
  assert.throws(() => consumeDocumentation(addon, new Uint8Array(), 'site:one', {local:true,allowRead:false}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'documentation_permission');
  for (const framed of [false, true]) {
    assert.throws(() => consumeTree(addon, '{}', {allowRead:false}, framed), error =>
      error instanceof Error && JSON.parse(error.message).code === 'markdown_read_permission');
  }
  assert.throws(() => consumePublication(addon, new Uint8Array(), {local:true,allowRead:false}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'publication_permission');
  assert.throws(() => consumePublicationFiles(addon, new Uint8Array(), {local:true,allowRead:false}), error =>
    error instanceof Error && JSON.parse(error.message).code === 'publication_permission');
  const source = Buffer.from((await readFile(new URL('./fixtures/workspace.toml', import.meta.url), 'utf8')).replaceAll('\n', '\r\n'));
  const invalid = Buffer.from(source.toString().replaceAll("publisher='owner:one'", "publisher='owner:missing'"));
  const queryValues = JSON.parse(await readFile(new URL('./fixtures/queries.json', import.meta.url), 'utf8'));
  const queries = Object.fromEntries(Object.entries(queryValues).map(([name, value]) => [name, JSON.stringify(value)]));
  const markdown = Buffer.from((await readFile(new URL('./fixtures/markdown.md', import.meta.url), 'utf8')).replaceAll('\n', '\r\n'));
  const document = Buffer.from((await readFile(new URL('./fixtures/document.md', import.meta.url), 'utf8')).replaceAll('\n', '\r\n'));
  const pagePackageWorkspace = Buffer.from(await readFile(new URL('./fixtures/page-package-workspace.toml', import.meta.url), 'utf8'));
  const pagePackage = Buffer.from(await readFile(new URL('./fixtures/page-package.md', import.meta.url), 'utf8'));
  const pagePackageStandalone = Buffer.from(await readFile(new URL('./fixtures/page-package-standalone.md', import.meta.url), 'utf8'));
  const pagePackageImage = await readFile(new URL('./fixtures/page-package-image.png', import.meta.url));
  const packageRequest = query => ({
    workspace:{path:'workspace.toml',bytes:pagePackageWorkspace},
    documents:[
      {path:'content/guide/index.md',bytes:pagePackage},
      {path:'content/standalone.md',bytes:pagePackageStandalone},
    ],
    companions:[{path:'content/guide/image.png',bytes:pagePackageImage}], query,
  });
  assert.deepEqual(JSON.parse(consumePagePackage(addon, packageRequest('content/guide/index.md'))), reference.pagePackage.document);
  assert.deepEqual(JSON.parse(consumePagePackage(addon, packageRequest('content/guide/image.png'))), reference.pagePackage.companion);
  assert.equal(consumePagePackage(addon, packageRequest('content/standalone.md')), 'null');
  const results = consume(addon, source, invalid, queries, markdown, document);
  const suppliedReference = Object.fromEntries(Object.entries(reference)
    .filter(([name]) => name !== 'pagePackage'));
  assert.deepEqual(Object.keys(results).sort(), Object.keys(suppliedReference).sort());
  for (const [name, value] of Object.entries(results)) {
    assert.equal(typeof value, 'string', `${name} preserves the native JSON string boundary`);
    assert.deepEqual(JSON.parse(value), suppliedReference[name], `${name} agrees with current Rust reference`);
  }
  assert.deepEqual(consume(addon, source, invalid, queries, markdown, document), results, 'Failed input does not poison subsequent native calls');
  assert.equal(reference.check.source_sha256, createHash('sha256').update(source).digest('hex'));
  assert.equal(source.subarray(reference.field.span.start, reference.field.span.end).toString(), "'Rêve fixture'");

  // A missing declaration must not silently resolve to any and keep the proof green.
  await rename(join(installed, 'index.d.ts'), join(installed, 'withheld.d.ts'));
  const rejected = runCompiler(['--project', 'tsconfig.json']);
  assert.notEqual(rejected.status, 0, 'Missing declaration must fail strict package consumption');
  assert.match(rejected.stdout + rejected.stderr, /TS7016|TS2307/, 'Failure must identify the unresolved package declarations');
  await rename(join(installed, 'withheld.d.ts'), join(installed, 'index.d.ts'));
  await rename(join(installed, 'search', 'index.d.ts'), join(installed, 'search', 'withheld.d.ts'));
  const rejectedSearch = runCompiler(['--project', 'tsconfig.json']);
  assert.notEqual(rejectedSearch.status, 0, 'Missing browser declarations must fail strict subpath consumption');
  assert.match(rejectedSearch.stdout + rejectedSearch.stderr, /TS7016|TS2307/,
    'Failure must identify unresolved browser declarations, not silently fall back to any');
  await rename(join(installed, 'search', 'withheld.d.ts'), join(installed, 'search', 'index.d.ts'));
  for (const subpath of ['analytics', 'webmcp']) {
    await rename(join(installed, subpath, 'index.d.ts'), join(installed, subpath, 'withheld.d.ts'));
    const rejectedBinding = runCompiler(['--project', 'tsconfig.browser.json']);
    assert.notEqual(rejectedBinding.status, 0, `Missing ${subpath} declarations must fail strict consumption`);
    assert.match(rejectedBinding.stdout + rejectedBinding.stderr, /TS7016|TS2307/,
      'Failure must identify unresolved browser declarations');
    await rename(join(installed, subpath, 'withheld.d.ts'), join(installed, subpath, 'index.d.ts'));
  }
  process.stdout.write(`${JSON.stringify({
    node: process.version, typescript: version.stdout.trim(), module: 'NodeNext ESM',
    package: manifest.name, nativeOperations: Object.keys(results),
    negativeTypeAssertions: 69, pagePackageChecked: true, historyPermissionChecked: true, deliveryInputChecked: true, qualificationInputChecked: true, reviewPermissionChecked: true, documentationPermissionChecked: true, publicationPermissionChecked: true, publicationFilesPermissionChecked: true, treePermissionChecked: true, missingDeclarationsRejected: true,
    browserSearch, browserWithoutNativeEntry: true, browserNegativeTypeAssertions: 15,
    browserMissingDeclarationsRejected: true, copiedPayloadEntries: copied.size,
    browserBindings, browserBindingNegativeTypeAssertions: 10,
    browserBindingMissingDeclarationsRejected: true,
    addonSha256: createHash('sha256').update(await readFile(addon)).digest('hex'),
  })}\n`);
} finally {
  // Only this exact fresh test directory is disposable; no supplied addon/source is removed.
  await rm(temporary, { recursive: true, force: true });
}
