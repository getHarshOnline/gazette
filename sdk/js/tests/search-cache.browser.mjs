// Qualify the real browser HTTP cache; request routing and fetch mocks would disable or replace it.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { pathToFileURL } from 'node:url';
import test from 'node:test';

const binding = process.env.GAZETTE_TEST_PLAYWRIGHT;
test('browser search revalidates cached bodies and obeys response no-store without stale fallback',
  { skip: !binding, timeout: 60_000 }, async t => {
    assert.ok(isAbsolute(binding), 'Select an installed Playwright module; never install implicitly');
    const { chromium } = await import(pathToFileURL(binding).href);
    const executablePath = process.env.GAZETTE_TEST_CHROMIUM;
    if (executablePath) assert.ok(isAbsolute(executablePath));
    const modules = new Map(await Promise.all([
      'index.mjs', 'load.mjs', 'corpus.mjs', 'browse.mjs', 'vendor/elasticlunr.mjs',
    ].map(async name => [`/sdk/${name}`, await readFile(new URL(`../search/${name}`, import.meta.url))])));
    const limits = {
      profile: 'lexical', maxDocuments: 20, maxBytes: 32768, maxQueryBytes: 100, maxResults: 10,
      allowNetwork: true, maxResponseBytes: 32768, timeoutMs: 2000,
    };
    const records = revision => [{
      content: { id: 'content:cache', title: `Revision ${revision}`, language: 'en', lifecycle: 'published' },
      route: { url: 'https://example.test/article/', canonical_url: 'https://example.test/original/', output: 'article/index.html' },
      body: `sentinel revision ${revision}`, summary: 'Complete retained summary',
      projects: [{ project: { id: 'project:cache', name: 'Cache qualification' }, relationship: { kind: 'about' } }],
      notes: { body_text: 'Retained commentary', body_html: '<p>Retained commentary</p>' },
      provenance: { source: 'native qualification', nested: { values: [1, true, null] } },
    }];
    const requests = [];
    let revision = 1;
    let failure;
    const server = createServer((request, response) => {
      if (request.url === '/') {
        response.writeHead(200, { 'content-type': 'text/html', 'cache-control': 'no-store' });
        response.end('<!doctype html><title>Search cache qualification</title>');
        return;
      }
      const module = modules.get(request.url);
      if (module) {
        response.writeHead(200, { 'content-type': 'text/javascript', 'cache-control': 'no-store' });
        response.end(module);
        return;
      }
      if (!['/search.json', '/private.json'].includes(request.url)) {
        response.writeHead(404); response.end(); return;
      }
      const row = {
        path: request.url, conditional: request.headers['if-none-match'] ?? null,
        cookie: request.headers.cookie ?? null, referrer: request.headers.referer ?? null,
        status: null, bytes: 0,
      };
      requests.push(row);
      if (failure === 'disconnect') { request.socket.destroy(); return; }
      if (failure === 'timeout') return;
      if (failure === 'status') {
        row.status = 503;
        response.writeHead(503, { 'cache-control': 'no-store' }); response.end(); return;
      }
      const privateResponse = request.url === '/private.json';
      const etag = privateResponse ? '"private"' : `"revision-${revision}"`;
      const body = Buffer.from(JSON.stringify(records(privateResponse ? 'private' : revision)));
      const headers = {
        'content-type': 'application/json; charset=utf-8', etag,
        'cache-control': privateResponse ? 'no-store' : 'public, max-age=3600, must-revalidate',
      };
      if (request.headers['if-none-match'] === etag) {
        row.status = 304;
        response.writeHead(304, headers); response.end();
      } else {
        row.status = 200; row.bytes = body.length;
        response.writeHead(200, { ...headers, 'content-length': body.length }); response.end(body);
      }
    });
    let browser;
    let context;
    t.after(async () => {
      try {
        await context?.close();
        await browser?.close();
      } finally {
        server.closeAllConnections();
        await new Promise(resolve => server.close(resolve));
      }
    });
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    const origin = `http://127.0.0.1:${server.address().port}`;
    browser = await chromium.launch({ headless: true, ...(executablePath ? { executablePath } : {}) });
    context = await browser.newContext({ serviceWorkers: 'block' });
    const page = await context.newPage();
    await page.goto(origin);
    await context.addCookies([{ name: 'must-not-send', value: 'private', url: origin }]);
    const load = (path = '/search.json', options = limits) => page.evaluate(async ({ path, options }) => {
      const { loadIndex } = await import('/sdk/index.mjs');
      try {
        const index = await loadIndex(new URL(path, location.href).href, options);
        const result = index.search('sentinel', { limit: 10 });
        return { records: result.map(hit => hit.record), frozen: result.every(hit => Object.isFrozen(hit.record.provenance.nested.values)) };
      } catch (error) { return { error: error.code }; }
    }, { path, options });
    const expected = value => ({ records: records(value), frozen: true });
    const first = await load();
    assert.deepEqual(first, expected(1));
    assert.deepEqual(await load(), first, '304 must retain the complete previously received body');
    revision = 2;
    assert.deepEqual(await load(), expected(2), 'changed ETag must replace the whole index');
    assert.deepEqual(await load(), expected(2), 'the replacement body must itself be cached and revalidated');
    assert.deepEqual(requests.map(({ conditional, status }) => ({ conditional, status })), [
      { conditional: null, status: 200 }, { conditional: '"revision-1"', status: 304 },
      { conditional: '"revision-1"', status: 200 }, { conditional: '"revision-2"', status: 304 },
    ]);
    assert.deepEqual(requests.map(row => row.bytes), [
      Buffer.byteLength(JSON.stringify(records(1))), 0, Buffer.byteLength(JSON.stringify(records(2))), 0,
    ]);
    const cacheOnly = path => page.evaluate(async path => {
      try {
        const response = await fetch(path, { cache: 'only-if-cached', mode: 'same-origin', credentials: 'omit' });
        return { status: response.status, body: response.ok ? await response.json() : null };
      } catch (error) { return { error: error.name }; }
    }, path);
    assert.deepEqual(await cacheOnly('/search.json'), { status: 200, body: records(2) });
    assert.equal(requests.length, 4, 'positive cache-only control must make no request');
    assert.deepEqual(await load('/private.json'), expected('private'));
    assert.deepEqual(await load('/private.json'), expected('private'));
    assert.deepEqual(requests.slice(4).map(({ conditional, status }) => ({ conditional, status })), [
      { conditional: null, status: 200 }, { conditional: null, status: 200 },
    ]);
    const miss = await cacheOnly('/private.json');
    assert.ok(miss.error === 'TypeError' || (miss.status === 504 && miss.body === null), JSON.stringify(miss));
    assert.equal(requests.length, 6, 'no-store cache-only miss must not reach the server');
    assert.deepEqual(await load('/search.json', { ...limits, maxResponseBytes: 32 }), { error: 'search_limit' });
    assert.equal(requests.at(-1).status, 304, 'cached-body reuse must retain the complete byte ceiling');
    failure = 'status';
    assert.deepEqual(await load(), { error: 'search_response' });
    failure = undefined;
    assert.deepEqual(await load(), expected(2));
    failure = 'disconnect';
    assert.deepEqual(await load(), { error: 'search_network' });
    failure = 'timeout';
    assert.deepEqual(await load('/search.json', { ...limits, timeoutMs: 50 }), { error: 'search_timeout' });
    assert.ok(requests.every(row => row.cookie === null && row.referrer === null));
    t.diagnostic(JSON.stringify({ browser: await browser.version(), requests, cacheOnlyNoStore: miss, server: origin }));
  });
