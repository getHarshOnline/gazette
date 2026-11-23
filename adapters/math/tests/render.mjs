import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const renderer = fileURLToPath(new URL('../render.mjs', import.meta.url));
const diagnostic = 'gazette: math generation failed\n';
const item = (tex, display = false) => ({ tex, display });

function run(input, arguments_ = []) {
  const result = spawnSync(process.execPath, [renderer, ...arguments_], {
    input: Buffer.isBuffer(input) || typeof input === 'string' ? input : JSON.stringify(input),
    // Relative provider imports resolve from the Gazette script, not a repository cwd.
    cwd: tmpdir(), env: {}, timeout: 15000, maxBuffer: 33 * 1024 * 1024,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.signal, null);
  return result;
}

function rejected(input, arguments_ = []) {
  const result = run(input, arguments_);
  assert.equal(result.status, 1);
  assert.equal(result.stdout.length, 0, 'no successful prefix or partial JSON');
  assert.equal(result.stderr.toString(), diagnostic, 'fixed diagnostic discloses no source or paths');
}

test('real provider renders ordered inline/display MathML with exact TeX and deterministic bytes', () => {
  const input = { formulas: [item(String.raw`\frac{a_1}{b}<c`), item(String.raw`\sum_{i=1}^{n}i`, true)] };
  const result = run(input);
  assert.equal(result.status, 0, result.stderr.toString());
  assert.equal(result.stderr.length, 0);
  const output = JSON.parse(result.stdout);
  assert.deepEqual(Object.keys(output), ['mathml']);
  assert.equal(output.mathml.length, 2);
  for (const mathml of output.mathml) {
    assert.ok(mathml.startsWith('<math xmlns="http://www.w3.org/1998/Math/MathML"'));
    assert.ok(mathml.endsWith('</math>'));
    assert.doesNotMatch(mathml, /<(?:span|script|link)\b|\b(?:href|src)=/u);
  }
  assert.doesNotMatch(output.mathml[0], /display="block"/u);
  assert.match(output.mathml[1], /display="block"/u);
  assert.ok(output.mathml[0].includes('<annotation encoding="application/x-tex">\\frac{a_1}{b}&lt;c</annotation>'));
  assert.ok(output.mathml[1].includes('<annotation encoding="application/x-tex">\\sum_{i=1}^{n}i</annotation>'));
  assert.deepEqual(run(input).stdout, result.stdout);
});

test('a later malformed formula discards all preceding successful results', () => {
  rejected({ formulas: [item('x+1'), item(String.raw`\frac{1`)] });
});

test('real corpus operator scripts retain exact TeX and one operand per MathML script', () => {
  // The first two formulas are the exact failing real-corpus TeX, not rewritten input.
  const inputs = [
    String.raw`L_{\log}(p,y)=-y\log p-(1-y)\log(1-p).`,
    String.raw`\eta(S)=\operatorname{clamp}\!\left(\eta_{\mathrm{base}}[1+\alpha\tanh(\beta S)],\eta_{\min},\eta_{\max}\right).`,
    String.raw`x^{\log}`,
    String.raw`x_{\min}^{\max}`,
    String.raw`x_{y_{\log}}`,
    String.raw`x_{\operatorname{clamp}}^{\operatorname{max}}`,
  ];
  const captured = JSON.parse(readFileSync(new URL('./fixtures/script-operands.json', import.meta.url)));
  for (const display of [false, true]) {
    const input = { formulas: inputs.map(tex => item(tex, display)) };
    const result = run(input);
    assert.equal(result.status, 0, result.stderr.toString());
    assert.equal(result.stderr.length, 0);
    const { mathml } = JSON.parse(result.stdout);
    const selected = captured.formulas.flatMap((formula, index) => formula.display === display ? [index] : []);
    assert.deepEqual(input.formulas, selected.map(index => captured.formulas[index]));
    assert.deepEqual(mathml, selected.map(index => captured.mathml[index]), 'actual bytes shared with strict Rust formatter qualification');
    assert.equal(mathml.length, inputs.length);
    for (const [index, tex] of inputs.entries()) {
      assert.ok(mathml[index].includes(`<annotation encoding="application/x-tex">${tex}</annotation>`));
      assert.equal(mathml[index].includes('display="block"'), display);
    }
    assert.ok(mathml[0].includes('<msub><mi>L</mi><mrow><mi>log</mi><mo>⁡</mo></mrow></msub>'));
    assert.ok(mathml[1].includes('<msub><mi>η</mi><mrow><mi>min</mi><mo>⁡</mo></mrow></msub>'));
    assert.ok(mathml[1].includes('<msub><mi>η</mi><mrow><mi>max</mi><mo>⁡</mo></mrow></msub>'));
    assert.ok(mathml[2].includes('<msup><mi>x</mi><mrow><mi>log</mi><mo>⁡</mo></mrow></msup>'));
    assert.ok(mathml[3].includes('<msubsup><mi>x</mi><mrow><mi>min</mi><mo>⁡</mo></mrow><mrow><mi>max</mi><mo>⁡</mo></mrow></msubsup>'));
    assert.ok(mathml[4].includes('<msub><mi>x</mi><msub><mi>y</mi><mrow><mi>log</mi><mo>⁡</mo></mrow></msub></msub>'));
    assert.ok(mathml[5].includes('<msubsup><mi>x</mi><mrow><mi mathvariant="normal">clamp</mi><mo>⁡</mo></mrow><mrow><mi mathvariant="normal">max</mi><mo>⁡</mo></mrow></msubsup>'));
    assert.deepEqual(run(input).stdout, result.stdout);
  }
});

test('the retained local provider correction preserves exact upstream and licence lineage', () => {
  const manifest = JSON.parse(readFileSync(new URL('../vendor/manifest.json', import.meta.url)));
  const bytes = readFileSync(new URL(`../vendor/${manifest.module}`, import.meta.url));
  const digest = value => createHash('sha256').update(value).digest('hex');
  assert.equal(digest(bytes), manifest.module_sha256);
  assert.equal(digest(readFileSync(new URL(`../vendor/${manifest.license_file}`, import.meta.url))), manifest.license_sha256);
  assert.equal(manifest.local_patch.id, 'mathml-script-operand-fragments');
  const changed = `      // Gazette local correction: a script operand is exactly one MathML
      // child, even when an operator emits identifier + ApplyFunction.
      var sub = buildGroup(group.sub, options);
      children.push(sub instanceof DocumentFragment ? new MathNode("mrow", sub.children) : sub);`;
  const changedSup = `      var sup = buildGroup(group.sup, options);
      children.push(sup instanceof DocumentFragment ? new MathNode("mrow", sup.children) : sup);`;
  const source = bytes.toString('utf8');
  assert.equal(source.split(changed).length, 2, 'one exact reviewed subscript correction');
  assert.equal(source.split(changedSup).length, 2, 'one exact reviewed superscript correction');
  const upstream = source.replace(changed, '      children.push(buildGroup(group.sub, options));')
    .replace(changedSup, '      children.push(buildGroup(group.sup, options));');
  assert.equal(digest(upstream), manifest.upstream_module_sha256, 'no other vendored changes');
});

test('request and formula fields, kinds and finite batch boundaries are exact', () => {
  for (const value of [
    null, [], {}, { formulas: [] }, { formulas: 'x' },
    { formulas: [item('x')], path: '/private/not-input' },
    { formulas: [null] }, { formulas: [{ tex: 'x' }] },
    { formulas: [{ tex: 'x', display: false, macros: {} }] },
    { formulas: [item(7)] }, { formulas: [item('x', 'false')] },
    { formulas: [item('')] }, { formulas: [item('\ud800')] },
    { formulas: Array.from({ length: 257 }, () => item('x')) },
  ]) rejected(value);
  const boundary = run({ formulas: Array.from({ length: 256 }, () => item('x')) });
  assert.equal(boundary.status, 0, boundary.stderr.toString());
  assert.equal(JSON.parse(boundary.stdout).mathml.length, 256);
  rejected({ formulas: [item('x')] }, ['caller-path']);
});

test('invalid UTF-8, syntax and per-formula byte/output excess fail without partial stdout', () => {
  rejected(Buffer.from([0xff]));
  rejected('{"formulas":[');
  rejected({ formulas: [item('x'.repeat(8193))] });
  rejected({ formulas: [item('é'.repeat(4097))] });
  rejected({ formulas: [item(Array.from({ length: 800 }, () => String.raw`\boxed{x}`).join('+'), true)] });
});

test('input byte ceiling refuses an unfinished stream without waiting for EOF', async () => {
  const child = spawn(process.execPath, [renderer], { cwd: tmpdir(), env: {}, stdio: ['pipe', 'pipe', 'pipe'] });
  const stdout = [], stderr = [];
  child.stdout.on('data', bytes => stdout.push(bytes));
  child.stderr.on('data', bytes => stderr.push(bytes));
  child.stdin.on('error', error => assert.ok(['EPIPE', 'ECONNRESET'].includes(error.code)));
  const timer = setTimeout(() => child.kill('SIGKILL'), 10000);
  const completion = new Promise((resolve, reject) => {
    child.on('error', reject);
    child.on('close', (code, signal) => resolve({ code, signal }));
  });
  child.stdin.write(Buffer.alloc(2 * 1024 * 1024 + 1, 0x20));
  try {
    const result = await completion;
    assert.deepEqual(result, { code: 1, signal: null });
    assert.equal(Buffer.concat(stdout).length, 0);
    assert.equal(Buffer.concat(stderr).toString(), diagnostic);
  } finally {
    clearTimeout(timer);
    child.stdin.destroy();
  }
});

test('provider trust refusal is an error, not a successfully colored command fallback', () => {
  for (const tex of [
    String.raw`\href{https://private.example/secret}{x}`,
    String.raw`\url{https://private.example/secret}`,
    String.raw`\includegraphics{private.png}`,
    String.raw`\htmlClass{private}{x}`,
  ]) rejected({ formulas: [item('x'), item(tex)] });
});

test('fresh macro dictionaries prevent definitions escaping their formula', () => {
  const define = item(String.raw`\gdef\gazetteprivate{z}\gazetteprivate`);
  const first = run({ formulas: [define] });
  assert.equal(first.status, 0, first.stderr.toString());
  rejected({ formulas: [define, item(String.raw`\gazetteprivate`)] });
  rejected({ formulas: [item(String.raw`\def\cycle{\cycle}\cycle`)] });
});
