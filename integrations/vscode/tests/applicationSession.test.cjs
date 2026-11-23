const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { ApplicationSession, ApplicationError } = require('../dist/applicationSession.js');

const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const metadata = (overrides = {}) => ({
  'io.rtepl.gazette/capability': 'build',
  'io.rtepl.gazette/intent': 'act',
  'io.rtepl.gazette/effect': 'read',
  'io.rtepl.gazette/potentialEffects': [],
  'io.rtepl.gazette/prominence': 'common',
  'io.rtepl.gazette/closure': 'self-terminal',
  ...overrides,
});

// Real disposable process boundary: fixtures implement only their selected response behavior.
async function fixture(t, handler) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'gazette-application-'));
  const executable = path.join(root, 'gazette');
  await fs.writeFile(executable, `#!${process.execPath}\n
const fs = require('node:fs');
const assert = require('node:assert/strict');
assert.deepEqual(process.argv.slice(2), ['mcp', '--max-frame-bytes', '16777216', '--max-response-bytes', '8387584']);
fs.writeFileSync('pid', String(process.pid));
const lines = require('node:readline').createInterface({ input: process.stdin });
function send(id, result) { process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id, result }) + '\\n'); }
lines.on('line', line => {
  fs.appendFileSync('requests', line + '\\n');
  const request = JSON.parse(line);
  assert.deepEqual(request.params._meta, {
    'io.modelcontextprotocol/protocolVersion': '2026-07-28',
    'io.modelcontextprotocol/clientCapabilities': {},
    ...(request.method === 'tools/list' ? {
      'io.rtepl.gazette/discovery': { capabilities: [
        'workspace', 'authoring', 'build', 'preview', 'documentation',
        'store', 'release', 'observation', 'recovery',
      ] },
    } : {}),
  });
  assert.notEqual(request.method, 'initialize');
  ${handler}
});
`, { mode: 0o700 });
  const session = new ApplicationSession(executable, root);
  t.after(async () => {
    try { await session.shutdown(); }
    catch {
      // Test custody only: never apply this cleanup policy to native application effects.
      const pid = Number(await fs.readFile(path.join(root, 'pid'), 'utf8'));
      try { process.kill(pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
      for (let attempt = 0; attempt < 100; attempt++) {
        await delay(10);
        try { await session.shutdown(); break; } catch { if (attempt === 99) throw new Error('Disposable child did not exit'); }
      }
    }
    await fs.rm(root, { recursive: true, force: true });
  });
  return { session, root, requests: async () => (await fs.readFile(path.join(root, 'requests'), 'utf8')).trim().split('\n') };
}

test('discovery and calls share one native process, exact current metadata and raw evidence', async t => {
  const f = await fixture(t, `
    if (request.method === 'tools/list') {
      process.stdout.write('{"jsonrpc":"2.0","id":' + request.id + ',"result":{"resultType":"complete","ttlMs":0,"cacheScope":"private","tools":[{"name":"new_native_operation","title":"Build · New native operation","description":"Native discovery","inputSchema":{"type":"object","maximum":18446744073709551615},"outputSchema":{"type":"object"},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false},"_meta":{"io.rtepl.gazette/capability":"build","io.rtepl.gazette/intent":"act","io.rtepl.gazette/effect":"read","io.rtepl.gazette/potentialEffects":[],"io.rtepl.gazette/prominence":"common","io.rtepl.gazette/closure":"terminal-owner","io.rtepl.gazette/relations":[{"kind":"verify","operation":"close_native_operation"}],"io.rtepl.gazette/terminalResultOwner":"close_native_operation"}},{"name":"close_native_operation","title":"Build · Close native operation","inputSchema":{"type":"object"},"outputSchema":{"type":"object"},"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":false},"_meta":{"io.rtepl.gazette/capability":"build","io.rtepl.gazette/intent":"recover","io.rtepl.gazette/effect":"local","io.rtepl.gazette/potentialEffects":["memory-state"],"io.rtepl.gazette/prominence":"advanced","io.rtepl.gazette/closure":"self-terminal"}}]}}\\n');
    } else send(request.id, { content: [], isError: false, structuredContent: { pid: process.pid, arguments: request.params.arguments } });
  `);
  const catalog = await f.session.discover();
  assert.equal(catalog.tools[0].name, 'new_native_operation');
  assert.equal(catalog.tools[0].inputSchema.type, 'object');
  assert.equal(catalog.tools[0].outputSchema.type, 'object');
  assert.equal(catalog.tools[0].annotations.readOnlyHint, true);
  assert.equal(catalog.tools[0]._meta['io.rtepl.gazette/intent'], 'act');
  assert.equal(catalog.tools[0]._meta['io.rtepl.gazette/effect'], 'read');
  assert.deepEqual(catalog.tools[0]._meta['io.rtepl.gazette/potentialEffects'], []);
  assert.equal(catalog.tools[0]._meta['io.rtepl.gazette/prominence'], 'common');
  assert.equal(catalog.tools[0]._meta['io.rtepl.gazette/closure'], 'terminal-owner');
  assert.deepEqual(catalog.tools[0]._meta['io.rtepl.gazette/relations'],
    [{ kind: 'verify', operation: 'close_native_operation' }]);
  assert.equal(catalog.tools[0]._meta['io.rtepl.gazette/terminalResultOwner'], 'close_native_operation');
  assert.match(catalog.raw, /18446744073709551615/);
  const first = await f.session.call('new_native_operation', '{"read":false}');
  const second = await f.session.call('another_discovered_operation', '{}');
  assert.equal(first.isError, false);
  assert.equal(JSON.parse(first.raw).result.structuredContent.pid, JSON.parse(second.raw).result.structuredContent.pid);
  assert.deepEqual(JSON.parse(first.raw).result.structuredContent.arguments, { read: false });
  assert.deepEqual((await f.requests()).map(line => JSON.parse(line).id), [1, 2, 3]);
});

test('discovery follows bounded standard pages and resolves relations only over the full union', async t => {
  const f = await fixture(t, `
    const descriptor = (name, meta = {}) => ({
      name, title: 'Build · ' + name, inputSchema: { type: 'object' }, outputSchema: { type: 'object' },
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
      _meta: { ...${JSON.stringify(metadata())}, ...meta },
    });
    if (request.method !== 'tools/list') throw new Error('unexpected method');
    if (request.params.cursor === undefined) {
      const tools = [descriptor('open', {
        'io.rtepl.gazette/closure': 'terminal-owner',
        'io.rtepl.gazette/relations': [{ kind: 'verify', operation: 'close' }],
        'io.rtepl.gazette/terminalResultOwner': 'close',
      }), ...Array.from({ length: 7 }, (_, index) => descriptor('first_' + index))];
      send(request.id, { resultType: 'complete', ttlMs: 0, cacheScope: 'private', tools, nextCursor: 'opaque-page-2' });
    } else {
      assert.equal(request.params.cursor, 'opaque-page-2');
      send(request.id, { resultType: 'complete', ttlMs: 0, cacheScope: 'private', tools: [descriptor('close'), descriptor('last')] });
    }
  `);
  const catalog = await f.session.discover();
  assert.equal(catalog.tools.length, 10);
  assert.equal(catalog.tools[0]._meta['io.rtepl.gazette/terminalResultOwner'], 'close');
  assert.equal(JSON.parse(catalog.raw).length, 2, 'raw evidence retains both standard page responses');
  const requests = (await f.requests()).map(JSON.parse);
  assert.equal(requests.length, 2);
  assert.equal(requests[0].params.cursor, undefined);
  assert.equal(requests[1].params.cursor, 'opaque-page-2');
  assert.deepEqual(requests[0].params._meta, requests[1].params._meta,
    'every cursor retains the exact native capability selection');
});

test('multiline framing preserves number spelling, duplicate fields, string escapes and Unicode', async t => {
  const f = await fixture(t, 'send(request.id, { content: [], isError: false });');
  const argumentsJson = '{\n "large":18446744073709551615,\n "exp":1.2300e+09, "large":9007199254740993,\n "text":"{ \\"quote\\" }\\n café ☃", "escaped":"\\u0061"\n}';
  await f.session.call('exact', argumentsJson);
  const [line] = await f.requests();
  assert.ok(line.endsWith(',"arguments":{"large":18446744073709551615,"exp":1.2300e+09,"large":9007199254740993,"text":"{ \\"quote\\" }\\n café ☃","escaped":"\\u0061"}}}'));
});

test('complete multi-site receipts fit the explicit native and editor response budgets', async t => {
  const f = await fixture(t, `send(request.id, { content: [], isError: false, structuredContent: { receipt: 'x'.repeat(5 * 1024 * 1024) } });`);
  const result = await f.session.call('render_sites', '{}');
  assert.equal(result.isError, false);
  assert.equal(JSON.parse(result.raw).result.structuredContent.receipt.length, 5 * 1024 * 1024);
});

test('tool failures preserve complete incomplete-set evidence rather than returning success', async t => {
  const f = await fixture(t, `send(request.id, { content: [{ type: 'text', text: 'private' }], isError: true, structuredContent: { complete: false, sites: [{ id: 'a', ok: true }, { id: 'b', ok: false }] } });`);
  const result = await f.session.call('render_sites', '{}');
  assert.equal(result.isError, true);
  assert.equal(JSON.parse(result.raw).result.structuredContent.sites.length, 2);
});

test('classified pre-dispatch RPC refusals retain private evidence without inventing an effect', async t => {
  const f = await fixture(t, `process.stderr.write('private diagnostics\\n'); process.stdout.write(JSON.stringify({ jsonrpc:'2.0', id:request.id, error:{code:-32602,message:'private native error',data:{retryable:true,'io.rtepl.gazette/dispatch':'not-dispatched'}} }) + '\\n');`);
  await assert.rejects(f.session.call('effect', '{}'), error => {
    assert.ok(error instanceof ApplicationError);
    assert.equal(error.effect, 'not-dispatched');
    assert.doesNotMatch(error.message, /private native error/);
    assert.match(error.raw, /private native error/);
    return true;
  });
  assert.equal((await f.requests()).length, 1);
});

for (const [name, data] of [
  ['explicit unknown', {'io.rtepl.gazette/dispatch':'outcome-unknown'}],
  ['unclassified', {retryable:true}],
]) {
  test(`${name} RPC errors conservatively retain unknown-effect custody`, async t => {
    const f = await fixture(t, `process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,error:{code:-32603,message:'private',data:${JSON.stringify(data)}}})+'\\n');`);
    await assert.rejects(f.session.call('effect', '{}'), error =>
      error instanceof ApplicationError && error.effect === 'unknown' && /private/.test(error.raw));
    await assert.rejects(f.session.call('again', '{}'), /unavailable/);
    await assert.rejects(f.session.shutdown(), /active or uncertain/);
    assert.equal((await f.requests()).length, 1);
  });
}

for (const [name, response] of [
  ['unclassified RPC error', `process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,error:{code:-32603,message:'private'}})+'\\n');`],
  ['malformed response', `process.stdout.write('{bad}\\n');`],
]) {
  test(`${name} retires discovery without inventing an application effect`, async t => {
    const f = await fixture(t, response);
    await assert.rejects(f.session.discover(), error =>
      error instanceof ApplicationError && error.effect === 'not-dispatched');
    await assert.rejects(f.session.discover(), /unavailable/);
    await f.session.shutdown();
    assert.equal((await f.requests()).length, 1);
  });
}

test('single in-flight admission and shutdown never queue, cancel or kill active work', async t => {
  const f = await fixture(t, `setTimeout(() => send(request.id, { content: [], isError: false }), 100);`);
  const pending = f.session.call('effect', '{}');
  await assert.rejects(f.session.discover(), /in-flight/);
  await assert.rejects(f.session.shutdown(), /No process was terminated/);
  assert.equal((await pending).isError, false);
  assert.equal((await f.requests()).length, 1);
  await f.session.shutdown();
  await assert.rejects(f.session.call('again', '{}'), /unavailable/);
});

test('object syntax, Unicode and outgoing bounds fail before native dispatch', async t => {
  const f = await fixture(t, 'send(request.id, { content: [], isError: false });');
  for (const argumentsJson of ['[]', 'null', '{} {}', '{"x":}', '{"x":"\ud800"}', '{"x":"' + 'x'.repeat(16 * 1024 * 1024) + '"}']) {
    await assert.rejects(f.session.call('effect', argumentsJson), error => error.effect === 'not-dispatched');
  }
  await f.session.call('valid', '{}');
  assert.equal((await f.requests()).length, 1);
});

for (const [name, output] of [
  ['malformed JSON', `process.stdout.write('{bad}\\n');`],
  ['invalid UTF-8', `process.stdout.write(Buffer.from([0xff, 10]));`],
  ['wrong request identity', `send(request.id + 1, {content:[]});`],
  ['unsafe request identity', `process.stdout.write('{"jsonrpc":"2.0","id":9007199254740993,"result":{"content":[]}}\\n');`],
  ['ambiguous result/error envelope', `process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:request.id,result:{content:[]},error:{code:-1,message:'bad'}})+'\\n');`],
  ['oversized unterminated response', `process.stdout.write('x'.repeat(8 * 1024 * 1024 + 1));`],
  ['truncated response', `process.stdout.end('{"jsonrpc":');`],
  ['invalid tool outcome', `send(request.id, {content:[],isError:'no'});`],
]) {
  test(`${name} retires the connection with unknown effects and retained process custody`, async t => {
    const f = await fixture(t, `${output}\nsetTimeout(() => process.exit(0), 1000);`);
    await assert.rejects(f.session.call('effect', '{}'), error => error.effect === 'unknown');
    await assert.rejects(f.session.call('again', '{}'), /unavailable/);
    await assert.rejects(f.session.shutdown(), /active or uncertain/);
    assert.equal((await f.requests()).length, 1);
  });
}

test('fragmented UTF-8 response is decoded only after the bounded complete frame', async t => {
  const f = await fixture(t, `
    const bytes = Buffer.from(JSON.stringify({ jsonrpc:'2.0', id:request.id, result:{content:[{type:'text',text:'☃'}],isError:false} }) + '\\n');
    const split = bytes.indexOf(Buffer.from('☃')) + 1;
    process.stdout.write(bytes.subarray(0,split));
    setTimeout(() => process.stdout.write(bytes.subarray(split)), 10);
  `);
  assert.match((await f.session.call('effect', '{}')).raw, /☃/);
});

test('many tiny fragments retain one complete response without a per-fragment collection', async t => {
  const f = await fixture(t, `
    const bytes = Buffer.from(JSON.stringify({ jsonrpc:'2.0', id:request.id, result:{content:[{type:'text',text:'x'.repeat(8192)}],isError:false} }) + '\\n');
    let offset = 0;
    function fragment() {
      process.stdout.write(bytes.subarray(offset, offset + 1));
      if (++offset < bytes.length) setImmediate(fragment);
    }
    fragment();
  `);
  const result = await f.session.call('fragmented', '{}');
  assert.equal(JSON.parse(result.raw).result.content[0].text, 'x'.repeat(8192));
  assert.equal(result.isError, false);
});

test('process loss reports unknown effect and only proven exit permits cleanup', async t => {
  const f = await fixture(t, 'process.exit(7);');
  await assert.rejects(f.session.call('effect', '{}'), error => error.effect === 'unknown');
  for (let attempt = 0; attempt < 100; attempt++) {
    try { await f.session.shutdown(); return; } catch { await delay(10); }
  }
  assert.fail('Process exit was not established');
});

test('duplicate discovery names, malformed descriptors and partial nonterminal pages are rejected', async t => {
  for (const result of [
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata() }, { name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata() }] },
    { resultType: 'complete', ttlMs: 0, cacheScope: 'private', tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata() }], nextCursor: 'more' },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata() }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: 'yes', destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata() }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/closure': 'terminal-owner', 'io.rtepl.gazette/terminalResultOwner': 'missing' }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/closure': 'terminal-owner', 'io.rtepl.gazette/terminalResultOwner': 1 }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/closure': 'related', 'io.rtepl.gazette/relations': [{ kind: 'verify', operation: 'missing' }] }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/intent': 'invent' }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/closure': 'related' }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/potentialEffects': ['network', 'network'] }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/potentialEffects': ['process', 'filesystem-read'] }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/potentialEffects': ['filesystem-write'] }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: true }, _meta: metadata({ 'io.rtepl.gazette/effect': 'external' }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/effect': 'external', 'io.rtepl.gazette/potentialEffects': ['network'] }) }] },
    { tools: [{ name: 'one', title: 'One', inputSchema: {}, outputSchema: {}, annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false }, _meta: metadata({ 'io.rtepl.gazette/effect': 'local', 'io.rtepl.gazette/potentialEffects': ['memory-state'] }) }] },
  ]) {
    const f = await fixture(t, `send(request.id, ${JSON.stringify(result)});`);
    await assert.rejects(f.session.discover(), /invalid or incomplete/);
    await assert.rejects(f.session.discover(), /unavailable/);
  }
});

test('discovery rejects cursor loops and duplicates across otherwise valid pages', async t => {
  for (const duplicate of [false, true]) {
    const f = await fixture(t, `
      const descriptor = name => ({ name, title: name, inputSchema: {}, outputSchema: {},
        annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
        _meta: ${JSON.stringify(metadata())} });
      const second = request.params.cursor !== undefined;
      const tools = Array.from({ length: 8 }, (_, index) => descriptor((second && ${duplicate}) && index === 0 ? 'tool_0' : (second ? 'second_' : 'tool_') + index));
      send(request.id, { resultType: 'complete', ttlMs: 0, cacheScope: 'private', tools, nextCursor: second ? 'same' : 'same' });
    `);
    await assert.rejects(f.session.discover(), /invalid or incomplete/);
    await assert.rejects(f.session.discover(), /unavailable/);
  }
});

test('discovery bounds aggregate raw page bytes before accepting a union', async t => {
  const f = await fixture(t, `
    const page = request.params.cursor === undefined ? 0 : Number(request.params.cursor.slice(1));
    const tools = Array.from({ length: 8 }, (_, index) => ({
      name: 'tool_' + page + '_' + index, title: 'Tool', description: 'x'.repeat(300000),
      inputSchema: {}, outputSchema: {},
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
      _meta: ${JSON.stringify(metadata())},
    }));
    send(request.id, { resultType: 'complete', ttlMs: 0, cacheScope: 'private', tools, nextCursor: 'p' + (page + 1) });
  `);
  await assert.rejects(f.session.discover(), /invalid or incomplete/);
  assert.ok((await f.requests()).length >= 6, 'aggregate, not one-page, ceiling is exercised');
});

test('capability paging still rejects oversized pages and exhausts the complete page ceiling', async t => {
  for (const oversized of [true, false]) {
    const f = await fixture(t, `
      const page = request.params.cursor === undefined ? 0 : Number(request.params.cursor);
      const tools = Array.from({ length: ${oversized ? 9 : 8} }, (_, index) => ({
        name: 'tool_' + page + '_' + index, title: 'Tool', inputSchema: {}, outputSchema: {},
        annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
        _meta: ${JSON.stringify(metadata())},
      }));
      send(request.id, { resultType: 'complete', ttlMs: 0, cacheScope: 'private', tools, nextCursor: String(page + 1) });
    `);
    await assert.rejects(f.session.discover(), /invalid or incomplete/);
    assert.equal((await f.requests()).length, oversized ? 1 : 32,
      'refusal remains all-or-nothing; no oversized page or 33rd request is admitted');
    await assert.rejects(f.session.call('tool_0_0', '{}'), /unavailable/);
  }
});

test('only absolute admitted executable and workspace paths can start a session', () => {
  assert.throws(() => new ApplicationSession('gazette', '/tmp'), /absolute/);
  assert.throws(() => new ApplicationSession('/gazette', 'relative'), /absolute/);
});
