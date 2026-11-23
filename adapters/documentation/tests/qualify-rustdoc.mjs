// Runs only the owned, dependency-free fixture. This is provider qualification,
// not the application's generator adapter or a source-repository runner.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [stable, nightly] = process.argv.slice(2);
assert.equal(process.argv.length, 4, 'supply exact stable and nightly rustdoc executables');
assert.ok(stable && nightly && isAbsolute(stable) && isAbsolute(nightly));
const fixture = fileURLToPath(new URL('./fixtures/rustdoc/library.rs', import.meta.url));
const output = mkdtempSync(join(tmpdir(), 'gazette-rustdoc-qualification-'));

function invoke(executable, args) {
  const result = spawnSync(executable, args, {
    cwd: output,
    encoding: 'utf8',
    timeout: 30_000,
    maxBuffer: 1024 * 1024,
    env: { PATH: process.env.PATH, TMPDIR: output },
  });
  if (result.error) throw result.error;
  assert.equal(result.signal, null, 'generator was interrupted');
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

try {
  const versions = { stable: invoke(stable, ['--version']), nightly: invoke(nightly, ['--version']) };
  const common = [fixture, '--crate-name', 'reference_fixture', '--crate-version', '0.1.0', '--edition', '2024', '-D', 'warnings'];
  invoke(stable, [...common, '-o', join(output, 'html')]);
  const recordHtml = readFileSync(join(output, 'html/reference_fixture/model/struct.Record.html'), 'utf8');
  assert.ok(recordHtml.includes('structfield.value'));
  assert.ok(recordHtml.includes('Original payload, without conversion.'));
  assert.ok(recordHtml.includes('assert_eq!'));
  const functionHtml = readFileSync(join(output, 'html/reference_fixture/fn.describe.html'), 'utf8');
  assert.ok(functionHtml.includes('model/struct.Record.html'));
  assert.ok(functionHtml.includes('0.1.0'));
  const constantHtml = readFileSync(join(output, 'html/reference_fixture/constant.MAX_RECORDS.html'), 'utf8');
  const staticHtml = readFileSync(join(output, 'html/reference_fixture/static.DEFAULT_LABEL.html'), 'utf8');
  assert.ok(constantHtml.includes('class="rustdoc constant"'));
  assert.ok(constantHtml.includes('pub const MAX_RECORDS:'));
  assert.ok(constantHtml.includes('static.DEFAULT_LABEL.html'));
  assert.ok(staticHtml.includes('class="rustdoc static"'));
  assert.ok(staticHtml.includes('pub static DEFAULT_LABEL:'));
  assert.ok(staticHtml.includes('constant.MAX_RECORDS.html'));
  invoke(nightly, [...common, '-Z', 'unstable-options', '--output-format', 'json', '-o', join(output, 'json')]);
  const model = JSON.parse(readFileSync(join(output, 'json/reference_fixture.json'), 'utf8'));
  assert.equal(model.format_version, 57, 'requalify the adapter before accepting a changed Rustdoc schema');
  assert.equal(model.crate_version, '0.1.0');
  const items = Object.values(model.index);
  const indexed = new Map(Object.entries(model.index));
  const item = (name, kind) => {
    const matches = items.filter(value => value.name === name && Object.hasOwn(value.inner, kind));
    assert.equal(matches.length, 1, `expected one ${kind} ${name}`);
    return matches[0];
  };
  const member = (owner, name, kind) => {
    const matches = owner.inner.trait.items
      .map(id => indexed.get(String(id)))
      .filter(value => value?.name === name && Object.hasOwn(value.inner, kind));
    assert.equal(matches.length, 1, `expected one ${kind} ${name} owned by ${owner.name}`);
    return matches[0];
  };
  const record = item('Record', 'struct');
  assert.equal(record.inner.struct.generics.params[0].name, 'T');
  assert.ok(record.docs.includes('assert_eq!(record.value, 3)'));
  assert.equal(record.inner.struct.kind.plain.fields.length, 1, 'complete Record field membership');
  const value = indexed.get(String(record.inner.struct.kind.plain.fields[0]));
  assert.equal(value?.name, 'value');
  assert.deepEqual(value.inner.struct_field, { generic: 'T' });
  assert.equal(record.inner.struct.kind.plain.fields[0], value.id);
  const describe = item('describe', 'function');
  assert.equal(describe.links['`model::Record`'], record.id);
  assert.equal(describe.inner.function.sig.inputs[0][0], 'record');
  assert.ok(describe.inner.function.sig.inputs[0][1].borrowed_ref);
  assert.equal(describe.inner.function.generics.params[0].name, 'T');
  assert.ok(describe.inner.function.sig.output.resolved_path);
  const selection = item('Selection', 'enum');
  assert.equal(selection.inner.enum.variants.length, 2);
  const reexport = items.find(value => value.inner.use?.name === 'PublicRecord');
  assert.ok(reexport, 'public re-export is retained');
  assert.equal(reexport.inner.use.id, record.id);
  const trait = item('Describe', 'trait');
  assert.ok(member(trait, 'Output', 'assoc_type'));
  assert.ok(member(trait, 'description', 'function'));
  const maximum = item('MAX_RECORDS', 'constant');
  const label = item('DEFAULT_LABEL', 'static');
  assert.deepEqual(maximum.inner.constant.type, { primitive: 'usize' });
  assert.equal(maximum.links['`DEFAULT_LABEL`'], label.id);
  assert.deepEqual(label.inner.static.type.borrowed_ref.type, { primitive: 'str' });
  assert.equal(label.inner.static.is_mutable, false);
  assert.equal(label.links['`MAX_RECORDS`'], maximum.id);
  assert.ok(maximum.docs.includes('Default upper bound'));
  assert.ok(label.docs.includes('retained records'));
  assert.deepEqual(model.paths[String(record.id)].path, ['reference_fixture', 'model', 'Record']);
  assert.ok(record.span.filename.endsWith('library.rs'));
  console.log(JSON.stringify({ versions, format_version: model.format_version, crate_version: model.crate_version, proof: 'HTML and JSON retain fixture modules, symbols, generics, borrowed signature, enum variants, re-export, trait members, constant/static declarations and reciprocal links, example and source location; no Markdown or hosting compatibility certified' }, null, 2));
} finally {
  rmSync(output, { recursive: true, force: true });
}
