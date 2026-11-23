// Package a built host executable with the editor; never download or install it.
const {copyFileSync, cpSync, chmodSync, constants, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync} = require('node:fs');
const {execFileSync, spawnSync} = require('node:child_process');
const {isAbsolute, join, resolve} = require('node:path');
const {parseArgs} = require('node:util');

const {values} = parseArgs({options: {
  server: {type: 'string'}, out: {type: 'string'},
}});
if (!values.server || !isAbsolute(values.server) || !values.out || !isAbsolute(values.out)) {
  throw new Error('Use npm run package -- --server /absolute/gazette --out /absolute/package.vsix');
}
const targets = {
  'darwin-arm64': 'aarch64-apple-darwin', 'darwin-x64': 'x86_64-apple-darwin',
  'linux-arm64': 'aarch64-unknown-linux-gnu', 'linux-x64': 'x86_64-unknown-linux-gnu',
};
const target = `${process.platform}-${process.arch}`;
if (!targets[target]) throw new Error(`No Gazette extension package is qualified for ${target}.`);
if (!statSync(values.server).isFile()) throw new Error('The selected Gazette program must be a file.');
const version = execFileSync(values.server, ['--version'], {encoding: 'utf8', timeout: 10000}).trim();
if (!version.startsWith('gazette ') || !version.includes(`target ${targets[target]})`)) {
  throw new Error(`Build Gazette for ${targets[target]} before packaging this host.`);
}
const root = resolve(__dirname, '..');
execFileSync('npm', ['run', 'build'], {cwd: root, stdio: 'inherit'});
// VS Code reads README.md after installation, regardless of VSCE's readme-path.
const staging = mkdtempSync(join(root, 'dist', 'package-'));
try {
  for (const name of ['bundle', 'media', 'syntaxes']) {
    cpSync(join(root, name), join(staging, name), {recursive: true, errorOnExist: true, force: false});
  }
  for (const name of ['CHANGELOG.md', 'LICENSE', 'language-configuration.json']) {
    copyFileSync(join(root, name), join(staging, name), constants.COPYFILE_EXCL);
  }
  copyFileSync(join(root, 'INSTALL.md'), join(staging, 'README.md'), constants.COPYFILE_EXCL);
  const manifest = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  delete manifest.scripts;
  writeFileSync(join(staging, 'package.json'), JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
  const directory = join(staging, 'server');
  mkdirSync(directory);
  const destination = join(directory, 'gazette');
  copyFileSync(values.server, destination, constants.COPYFILE_EXCL);
  chmodSync(destination, 0o755);
  execFileSync('python3', [
    join(root, '..', '..', 'tools/generate_distribution_evidence.py'),
    '--output-root', directory,
  ], {stdio: 'inherit'});
  execFileSync('python3', [
    join(root, '..', '..', 'tools/generate_distribution_evidence.py'),
    '--check', '--output-root', directory,
  ], {stdio: 'inherit'});
  const result = spawnSync('npx', [
    '--package', '@vscode/vsce@2.32.0', 'vsce', 'package', '--target', target,
    '--allow-missing-repository', '--no-dependencies', '--out', values.out,
  ], {cwd: staging, stdio: 'inherit', shell: false});
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`VSIX packaging failed (${result.signal ?? result.status}).`);
} finally {
  rmSync(staging, {recursive: true});
}
