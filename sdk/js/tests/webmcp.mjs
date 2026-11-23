// Node carrier conformance only: this does not qualify native browser WebMCP support.
import assert from 'node:assert/strict';
import test from 'node:test';
import { registerReadOnlyTools, WebMcpError } from '../webmcp/index.mjs';
import { createIndex } from '../search/index.mjs';
import { isAbsolute } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const limits = {
  maxInputBytes: 1024, maxOutputBytes: 8192, maxResults: 10,
  maxNodes: 1000, maxDepth: 16, maxConcurrent: 2, timeoutMs: 1000,
};
const code = expected => error => error instanceof WebMcpError
  && error.code === expected && error.message === expected;
const record = () => ({
  content: { id: 'content:article', title: 'Shared compiler', authors: ['person:author'] },
  route: { url: 'https://example.test/article/', canonical_url: 'https://original.test/article/' },
  body: 'Immutable graph', projects: [],
  notes: { body_text: 'Subordinate commentary' },
});
const configure = extra => ({ tools: ['current_page'], limits, currentPage: record, ...extra });
function carrier(failureAt) {
  const tools = new Map();
  const signals = [];
  let calls = 0;
  const document = { modelContext: { async registerTool(tool, { signal }) {
    signals.push(signal);
    if (++calls === failureAt || tools.has(tool.name)) throw new Error('native registration refusal');
    tools.set(tool.name, tool);
    signal.addEventListener('abort', () => tools.delete(tool.name), { once: true });
  } } };
  return {
    document, tools, signals,
    invoke(name, input = {}, signal = new AbortController().signal) {
      return tools.get(name).execute(input, { signal });
    },
  };
}

test('selected native TypeScript checks browser Document and existing search without an API shim',
  { skip: !process.env.GAZETTE_TEST_TYPESCRIPT }, async () => {
    const compiler = process.env.GAZETTE_TEST_TYPESCRIPT;
    assert.ok(isAbsolute(compiler), 'Select an exact installed compiler; no tool lookup or install');
    const { default: ts } = await import(pathToFileURL(compiler).href);
    const path = fileURLToPath(new URL('../webmcp/consumer.mts', import.meta.url));
    const source = `
      import {registerReadOnlyTools} from './index.js';
      import {createIndex} from '../search/index.js';
      const limits=${JSON.stringify(limits)};
      const index=createIndex([], {profile:'lexical',maxDocuments:10,maxBytes:8192,maxQueryBytes:1024,maxResults:10});
      registerReadOnlyTools(document, {tools:['current_page','search'],limits,
        currentPage:()=>({title:'Public page'}),search:(query,{limit})=>index.search(query,{limit})});
      // @ts-expect-error Effectful tools are not admitted.
      registerReadOnlyTools(document,{tools:['subscribe'],limits});
      // @ts-expect-error Search input and callback options remain typed.
      registerReadOnlyTools(document,{tools:['search'],limits,search:(query:number)=>[]});
    `;
    const options = {
      strict: true, noEmit: true, skipLibCheck: false, exactOptionalPropertyTypes: true,
      target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.NodeNext,
      moduleResolution: ts.ModuleResolutionKind.NodeNext, types: [],
      lib: ['lib.es2022.d.ts', 'lib.dom.d.ts'],
    };
    const host = ts.createCompilerHost(options);
    const original = host.getSourceFile.bind(host);
    host.getSourceFile = (file, ...args) => file === path
      ? ts.createSourceFile(file, source, ts.ScriptTarget.ES2022, true) : original(file, ...args);
    const diagnostics = ts.getPreEmitDiagnostics(ts.createProgram([path], options, host));
    assert.equal(diagnostics.length, 0, ts.formatDiagnostics(diagnostics, {
      getCurrentDirectory: () => process.cwd(), getCanonicalFileName: file => file, getNewLine: () => '\n',
    }));
  });

test('import has no document, navigator, provider, or registration effects', async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'document');
  Object.defineProperty(globalThis, 'document', {
    configurable: true, get() { throw new Error('import read document'); },
  });
  try { await import('../webmcp/index.mjs?explicit-import-proof'); }
  finally {
    if (previous) Object.defineProperty(globalThis, 'document', previous);
    else delete globalThis.document;
  }
});

test('explicit two-tool registration preserves the current public projection and actual search hits', async () => {
  const host = carrier();
  const page = record();
  const index = createIndex([page], {
    profile: 'lexical', maxDocuments: 10, maxBytes: 8192, maxQueryBytes: 1024, maxResults: 10,
  });
  let calls = 0;
  const registration = await registerReadOnlyTools(host.document, configure({
    tools: ['current_page', 'search'], currentPage: () => { calls++; return page; },
    search: (query, { limit, signal }) => {
      assert.equal(signal.aborted, false);
      return index.search(query, { limit });
    },
  }));
  assert.equal(calls, 0, 'registration does not acquire page data');
  assert.deepEqual(registration.names, ['current_page', 'search']);
  assert.ok(Object.isFrozen(registration.names));
  for (const tool of host.tools.values()) {
    assert.deepEqual(tool.annotations, {
      readOnlyHint: true, untrustedContentHint: true, consequentialHint: false,
    });
    assert.equal(tool.inputSchema.additionalProperties, false);
  }
  const captured = await host.invoke('current_page');
  assert.equal(JSON.stringify(captured), JSON.stringify(page));
  assert.ok(Object.isFrozen(captured.notes));
  page.content.title = 'changed after response';
  assert.equal(captured.content.title, 'Shared compiler');
  const hits = await host.invoke('search', { query: 'shared', limit: 1 });
  assert.equal(JSON.stringify(hits), JSON.stringify(index.search('shared', { limit: 1 })));
  assert.equal(hits[0].record.route.canonical_url, 'https://original.test/article/');
  const current = await host.invoke('current_page');
  assert.equal(current.content.title, 'changed after response', 'each invocation reads current supplied data');
  registration.close(); registration.close();
  assert.equal(host.tools.size, 0);
  assert.ok(host.signals.every(signal => signal.aborted));
});

test('unsupported document API has no navigator fallback or callback effects', async () => {
  let calls = 0;
  for (const document of [undefined, {}, { modelContext: {} }, { navigator: { modelContext: carrier().document.modelContext } }]) {
    await assert.rejects(registerReadOnlyTools(document, configure({ currentPage: () => calls++ })), code('webmcp_unsupported'));
  }
  assert.equal(calls, 0);
});

test('closed selections and finite complete limits fail before native registration', async () => {
  const host = carrier();
  for (const tools of [[], ['subscribe'], ['current_page', 'current_page'], ['search']]) {
    await assert.rejects(registerReadOnlyTools(host.document, configure({ tools })), code('webmcp_options'));
  }
  let getterCalls = 0;
  const tools = [];
  Object.defineProperty(tools, '0', { get() { getterCalls++; return 'current_page'; } });
  await assert.rejects(registerReadOnlyTools(host.document, configure({ tools })), code('webmcp_options'));
  assert.equal(getterCalls, 0);
  for (const value of [0, -1, Infinity, 1.5, Number.MAX_SAFE_INTEGER]) {
    await assert.rejects(registerReadOnlyTools(host.document,
      configure({ limits: { ...limits, timeoutMs: value } })), code('webmcp_limit'));
  }
  await assert.rejects(registerReadOnlyTools(host.document, configure({ invented: true })), code('webmcp_options'));
  assert.equal(host.signals.length, 0);
});

test('partial registration and duplicate collisions unregister only this ownership', async () => {
  const host = carrier(2);
  await assert.rejects(registerReadOnlyTools(host.document, configure({
    tools: ['current_page', 'search'], search: () => [],
  })), code('webmcp_registration'));
  assert.equal(host.tools.size, 0);
  assert.ok(host.signals.every(signal => signal.aborted));
  const existing = carrier();
  const first = await registerReadOnlyTools(existing.document, configure());
  await assert.rejects(registerReadOnlyTools(existing.document, configure()), code('webmcp_registration'));
  assert.equal(existing.tools.size, 1, 'failed second ownership cannot retire the first');
  first.close();
});

test('invalid inputs, accessors, unknown fields and pre-cancellation never call search', async () => {
  const host = carrier();
  let calls = 0;
  const registration = await registerReadOnlyTools(host.document, configure({
    tools: ['current_page', 'search'], search: () => { calls++; return []; },
  }));
  for (const input of [null, [], { query: 'a' }, { query: 'a', limit: 0 },
    { query: 'a', limit: 11 }, { query: ' ', limit: 1 }, { query: 7, limit: 1 },
    { query: 'a', limit: 1, navigate: true }]) {
    await assert.rejects(host.invoke('search', input), code('webmcp_input'));
  }
  let getterCalls = 0;
  await assert.rejects(host.invoke('search', {
    get query() { getterCalls++; return 'a'; }, limit: 1,
  }), code('webmcp_input'));
  assert.equal(getterCalls, 0);
  await assert.rejects(host.invoke('current_page', { path: '/private/source' }), code('webmcp_input'));
  const aborted = new AbortController(); aborted.abort();
  await assert.rejects(host.invoke('search', { query: 'a', limit: 1 }, aborted.signal), code('webmcp_cancelled'));
  assert.equal(calls, 0);
  registration.close();
});

test('exact UTF-8 input and whole JSON output byte limits do not truncate', async () => {
  const input = { query: '🙂', limit: 1 };
  const inputBytes = Buffer.byteLength(JSON.stringify(input));
  const page = { text: '🙂\n' };
  const outputBytes = Buffer.byteLength(JSON.stringify(page));
  const run = async (selected, operation, input) => {
    const host = carrier();
    const registration = await registerReadOnlyTools(host.document, configure({
      tools: ['current_page', 'search'], limits: selected, currentPage: () => page, search: () => [],
    }));
    try { return await host.invoke(operation, input); }
    finally { registration.close(); }
  };
  assert.equal(JSON.stringify(await run({ ...limits, maxOutputBytes: outputBytes }, 'current_page', {})), JSON.stringify(page));
  await assert.rejects(run({ ...limits, maxOutputBytes: outputBytes - 1 }, 'current_page', {}), code('webmcp_limit'));
  assert.equal((await run({ ...limits, maxInputBytes: inputBytes }, 'search', input)).length, 0);
  await assert.rejects(run({ ...limits, maxInputBytes: inputBytes - 1 }, 'search', input), code('webmcp_limit'));
});

test('non-JSON results, cycles, getters and oversize search results refuse complete output', async () => {
  const cycle = {}; cycle.self = cycle;
  let getterCalls = 0;
  const getter = { get secret() { getterCalls++; return 'private'; } };
  for (const page of [undefined, NaN, 1n, new Date(), cycle, getter, { toJSON() { return {}; } }, [, 1]]) {
    const host = carrier();
    const registration = await registerReadOnlyTools(host.document, configure({ currentPage: () => page }));
    await assert.rejects(host.invoke('current_page'), code('webmcp_data'));
    registration.close();
  }
  assert.equal(getterCalls, 0);
  for (const result of [null, {}, [record(), record()]]) {
    const host = carrier();
    const registration = await registerReadOnlyTools(host.document,
      configure({ tools: ['search'], search: () => result }));
    await assert.rejects(host.invoke('search', { query: 'a', limit: 1 }), code('webmcp_data'));
    registration.close();
  }
});

test('node/depth ceilings and callback failure remain typed without partial projection', async () => {
  for (const selected of [{ ...limits, maxNodes: 2 }, { ...limits, maxDepth: 1 }]) {
    const host = carrier();
    const registration = await registerReadOnlyTools(host.document,
      configure({ limits: selected, currentPage: () => ({ a: { b: true } }) }));
    await assert.rejects(host.invoke('current_page'), code('webmcp_limit'));
    registration.close();
  }
  const cause = new Error('caller failure');
  const host = carrier();
  const registration = await registerReadOnlyTools(host.document,
    configure({ currentPage: () => { throw cause; } }));
  await assert.rejects(host.invoke('current_page'), error => code('webmcp_callback')(error) && error.cause === cause);
  registration.close();
});

test('active cancellation and close signal callbacks and reject retained execution handles', async () => {
  for (const action of ['cancel', 'close']) {
    const host = carrier();
    let entered;
    const started = new Promise(resolve => { entered = resolve; });
    let observed;
    let finish;
    const registration = await registerReadOnlyTools(host.document, configure({ currentPage: ({ signal }) => {
      observed = signal; entered(); return new Promise(resolve => { finish = resolve; });
    } }));
    const tool = host.tools.get('current_page');
    const execution = new AbortController();
    const pending = host.invoke('current_page', {}, execution.signal);
    const refused = assert.rejects(pending, code(action === 'cancel' ? 'webmcp_cancelled' : 'webmcp_closed'));
    await started;
    if (action === 'cancel') execution.abort(); else registration.close();
    await refused;
    assert.equal(observed.aborted, true);
    finish({ ignored: true });
    registration.close();
    await assert.rejects(tool.execute({}, { signal: new AbortController().signal }), code('webmcp_closed'));
  }
});

test('timeout signals callback but keeps an unsettled task charged to the concurrency ceiling', async () => {
  const host = carrier();
  let observed;
  let finish;
  let calls = 0;
  const registration = await registerReadOnlyTools(host.document, configure({
    limits: { ...limits, timeoutMs: 10, maxConcurrent: 1 },
    currentPage: ({ signal }) => {
      if (++calls > 1) return record();
      observed = signal; return new Promise(resolve => { finish = resolve; });
    },
  }));
  await assert.rejects(host.invoke('current_page'), code('webmcp_timeout'));
  assert.equal(observed.aborted, true);
  await assert.rejects(host.invoke('current_page'), code('webmcp_busy'));
  finish({ late: true });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal((await host.invoke('current_page')).content.title, 'Shared compiler');
  registration.close();
});
