// Build the installed client and retain the notices and helper its dependencies use.
const {build} = require('esbuild');
const {copyFileSync, chmodSync, readFileSync, readdirSync, writeFileSync} = require('node:fs');
const {builtinModules} = require('node:module');
const {join, resolve} = require('node:path');

async function bundle() {
  const root = resolve(__dirname, '..');
  const result = await build({
    absWorkingDir: root,
    entryPoints: ['src/extension.ts'],
    outfile: 'bundle/extension.js',
    bundle: true,
    platform: 'node',
    format: 'cjs',
    target: 'node20',
    external: ['vscode'],
    legalComments: 'linked',
    metafile: true,
  });
  const allowed = new Set(['vscode', ...builtinModules, ...builtinModules.map(name => `node:${name}`)]);
  for (const output of Object.values(result.metafile.outputs)) {
    for (const imported of output.imports) {
      if (imported.external && !allowed.has(imported.path)) {
        throw new Error(`The editor bundle still needs an external package: ${imported.path}`);
      }
    }
  }
  const packages = new Set();
  for (const input of Object.keys(result.metafile.inputs)) {
    if (!input.startsWith('node_modules/')) continue;
    const parts = input.split('/');
    // Resolve the innermost package when npm installs a nested dependency.
    const start = parts.lastIndexOf('node_modules') + 1;
    packages.add(parts.slice(0, start + (parts[start].startsWith('@') ? 2 : 1)).join('/'));
  }
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'));
  const notices = ['Third-party software included in the Gazette editor client.\n'];
  for (const path of [...packages].sort()) {
    const manifest = JSON.parse(readFileSync(join(root, path, 'package.json'), 'utf8'));
    if (lock.packages[path]?.version !== manifest.version) {
      throw new Error(`Installed ${manifest.name} does not match the package lock; run npm ci.`);
    }
    const licenses = readdirSync(join(root, path)).filter(name => /^(license|notice)(\.[a-z]+)?$/i.test(name)).sort();
    if (!licenses.some(name => /^license/i.test(name))) throw new Error(`No license text found for ${manifest.name}.`);
    notices.push(`${manifest.name} ${manifest.version} (${manifest.license})\n${licenses.map(name => readFileSync(join(root, path, name), 'utf8')).join('\n')}`);
  }
  writeFileSync(join(root, 'bundle', 'THIRD_PARTY_NOTICES.txt'), notices.join('\n\n'));
  // Bundling moves the language client's __dirname; preserve its process-tree helper there.
  copyFileSync(join(root, 'node_modules/vscode-languageclient/lib/node/terminateProcess.sh'), join(root, 'bundle/terminateProcess.sh'));
  chmodSync(join(root, 'bundle/terminateProcess.sh'), 0o755);
}

bundle().catch(error => { console.error(error); process.exitCode = 1; });
