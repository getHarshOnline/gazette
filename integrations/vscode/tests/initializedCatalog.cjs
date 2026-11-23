// Actual initialized native MCP paging -> the editor's stateless bounded bridge.
// No copied tool/schema catalog and no application operation is dispatched.
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { isAbsolute } = require('node:path');
const { ApplicationSession } = require('../dist/applicationSession.js');

async function run() {
  const executable = process.env.GAZETTE_TEST_EXECUTABLE;
  assert.ok(executable && isAbsolute(executable), 'exact fresh executable required');
  const child = spawn(executable, ['mcp'], { cwd: process.cwd(), shell: false, stdio: 'pipe' });
  const closed = new Promise(resolve => child.once('close', resolve));
  child.stderr.resume();
  let pending;
  let frame = Buffer.alloc(0);
  let id = 0;
  child.stdout.on('data', chunk => {
    if (!pending) return;
    if (frame.length + chunk.length > 4 * 1024 * 1024) {
      pending.reject(new Error('native catalog frame ceiling')); return;
    }
    frame = Buffer.concat([frame, chunk]);
    const newline = frame.indexOf(10);
    if (newline < 0) return;
    try {
      assert.equal(newline, frame.length - 1, 'one exact response, no unsolicited frames');
      const response = JSON.parse(frame.subarray(0, newline).toString('utf8'));
      assert.equal(response.id, pending.id);
      assert.equal(response.error, undefined);
      frame = Buffer.alloc(0);
      const held = pending; pending = undefined;
      held.resolve(response);
    } catch (error) { pending?.reject(error); }
  });
  child.on('error', error => pending?.reject(error));
  function send(method, params) {
    assert.equal(pending, undefined, 'no overlapping test requests');
    const requestId = ++id;
    let timer;
    return new Promise((resolve, reject) => {
      timer = setTimeout(() => reject(new Error('bounded native catalog response missing')), 10000);
      pending = { id: requestId, resolve, reject };
      child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: requestId, method, params }) + '\n');
    }).finally(() => { clearTimeout(timer); });
  }
  const bridge = new ApplicationSession(executable, process.cwd());
  try {
    const initialized = await send('initialize', { protocolVersion: '2026-07-28', capabilities: {},
      clientInfo: { name: 'gazette-editor-native-catalog-test', version: '1' } });
    assert.equal(initialized.result.protocolVersion, '2026-07-28');
    child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
    const capabilities = ['workspace', 'authoring', 'build', 'preview', 'documentation', 'store', 'release', 'observation', 'recovery'];
    const tools = [];
    const cursors = new Set();
    let cursor;
    let bytes = 0;
    let pages = 0;
    do {
      assert.ok(++pages <= 32);
      const response = await send('tools/list', { _meta: { 'io.rtepl.gazette/discovery': { capabilities } },
        ...(cursor === undefined ? {} : { cursor }) });
      bytes += Buffer.byteLength(JSON.stringify(response));
      assert.ok(bytes <= 16 * 1024 * 1024);
      const page = response.result;
      assert.equal(page.resultType, 'complete');
      assert.equal(page.ttlMs, 0);
      assert.equal(page.cacheScope, 'private');
      assert.ok(page.tools.length > 0 && page.tools.length <= 8);
      tools.push(...page.tools);
      assert.ok(tools.length <= 256);
      cursor = page.nextCursor;
      if (cursor !== undefined) {
        assert.equal(page.tools.length, 8);
        assert.equal(typeof cursor, 'string');
        assert.ok(!cursors.has(cursor));
        cursors.add(cursor);
      }
    } while (cursor !== undefined);
    assert.ok(pages > 1, 'initialized actual native session paged the complete selected catalog');
    assert.deepEqual((await bridge.discover()).tools, tools,
      'editor union matches initialized native discovery, including exact schemas and relations');
    assert.deepEqual([...new Set(tools.map(tool => tool._meta['io.rtepl.gazette/capability']))].sort(), capabilities.sort());
    console.log(`Gazette initialized native catalog proof: ${tools.length} tools in ${pages} bounded pages; editor union exact.`);
  } finally {
    await bridge.shutdown();
    child.stdin.end();
    let timer;
    try {
      await Promise.race([closed, new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error('native catalog child closure unproven')), 10000);
      })]);
    } finally {
      clearTimeout(timer);
      // Exact disposable read-only test child only, never application-effect custody.
      if (child.exitCode === null && child.signalCode === null) { child.kill('SIGTERM'); await closed; }
    }
  }
}

run().catch(error => { console.error(error?.stack ?? error); process.exitCode = 1; });
