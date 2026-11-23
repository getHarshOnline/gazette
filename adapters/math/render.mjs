// Finite native generation only. The Rust consumer binds and checks retained MathML.
import { TextDecoder } from 'node:util';

const INPUT_BYTES = 2 * 1024 * 1024;
const TEX_BYTES = 8 * 1024;
const MATHML_BYTES = 128 * 1024;
const OUTPUT_BYTES = 32 * 1024 * 1024;

function fields(value, names) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).length === names.length
    && names.every(name => Object.hasOwn(value, name));
}

async function request() {
  let size = 0;
  const chunks = [];
  for await (const chunk of process.stdin) {
    size += chunk.length;
    if (size > INPUT_BYTES) throw new Error();
    chunks.push(chunk);
  }
  // Internal Rust-produced batch: this validates value shape, not duplicate keys.
  // Public duplicate-safe JSON admission remains at the native caller boundary.
  const value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks, size)));
  if (!fields(value, ['formulas']) || !Array.isArray(value.formulas)
      || value.formulas.length === 0 || value.formulas.length > 256) throw new Error();
  for (const formula of value.formulas) {
    if (!fields(formula, ['tex', 'display']) || typeof formula.tex !== 'string'
        || !formula.tex.isWellFormed() || !formula.tex.length
        || Buffer.byteLength(formula.tex) > TEX_BYTES
        || typeof formula.display !== 'boolean') throw new Error();
  }
  return value.formulas;
}

function formula(katex, { tex, display }) {
  let denied = false;
  const rendered = katex.renderToString(tex, {
    displayMode: display,
    output: 'mathml',
    strict: 'error',
    throwOnError: true,
    globalGroup: false,
    macros: {},
    maxExpand: 1000,
    maxSize: 20,
    // KaTeX can replace a denied command with colored text. That is not success.
    trust: () => { denied = true; return false; },
  });
  if (denied) throw new Error();
  const prefix = '<span class="katex">';
  const suffix = '</span>';
  if (!rendered.startsWith(prefix) || !rendered.endsWith(suffix)) throw new Error();
  const mathml = rendered.slice(prefix.length, -suffix.length);
  // This checks the pinned provider's wrapper, not arbitrary MathML admission.
  if (!mathml.startsWith('<math ') || !mathml.endsWith('</math>')
      || Buffer.byteLength(mathml) > MATHML_BYTES) throw new Error();
  return mathml;
}

try {
  if (process.argv.length !== 2 || Number.parseInt(process.versions.node, 10) < 20) throw new Error();
  const formulas = await request();
  const { default: katex } = await import('./vendor/katex-0.18.4/katex.mjs');
  const mathml = [];
  // Count actual JSON spelling, including escaping, before retaining each result.
  let bytes = Buffer.byteLength('{"mathml":[]}\n');
  for (const input of formulas) {
    const rendered = formula(katex, input);
    bytes += Buffer.byteLength(JSON.stringify(rendered)) + (mathml.length ? 1 : 0);
    if (bytes > OUTPUT_BYTES) throw new Error();
    mathml.push(rendered);
  }
  const output = JSON.stringify({ mathml }) + '\n';
  await new Promise((resolve, reject) => process.stdout.write(output, error => error ? reject(error) : resolve()));
} catch {
  process.stderr.write('gazette: math generation failed\n');
  process.exitCode = 1;
}
