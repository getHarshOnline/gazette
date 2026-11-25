// Real retained provider and native HTTP acquisition; no alternate ranking implementation.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { createIndex, loadIndex, MAX_SEARCH_BYTES, SearchError } from '../search/index.mjs';
import elasticlunr from '../search/vendor/elasticlunr.mjs';

test('native English recommendation vocabulary matches the retained search provider', async () => {
  const data = await readFile(new URL('../../../crates/gazette-compiler/src/authored/english-stopwords.txt', import.meta.url), 'utf8');
  const native = data.split(/\r?\n/).filter(line => line && !line.startsWith('#'));
  assert.deepEqual(native, Object.keys(elasticlunr.defaultStopWords).filter(Boolean).sort());
});

const limits = {
  profile: 'lexical', maxDocuments: 20, maxBytes: 32768,
  maxQueryBytes: 100, maxResults: 10,
};
const record = (id, title, body, extra = {}) => ({
  content: { id: `content:${id}`, title, language: 'en', lifecycle: 'published' },
  route: { url: `https://example.test/mount/${id}/`, canonical_url: `https://example.test/mount/${id}/`, output: `${id}/index.html` },
  body, projects: [], ...extra,
});
const documents = () => [
  record('compiler', 'Publication compiler', 'Shared immutable graph and exact content.', {
    summary: 'Retained summary',
    projects: [{ project: { id: 'project:gazette', name: 'Gazette', lifecycle: 'active', visibility: 'public', availability: 'planned' }, relationship: { kind: 'about' } }],
    provenance: { project: 'project:gazette' },
  }),
  record('body', 'Daily notes', 'Publication compiler runs directly.'),
  record('unrelated', 'Cooking', 'Dinner recipes'),
];
const code = expected => error => error instanceof SearchError && error.code === expected
  && error.message === expected;

test('exported byte ceiling bounds both capture and acquisition without requesting the network', async () => {
  assert.throws(() => createIndex([], { ...limits, maxBytes: MAX_SEARCH_BYTES + 1 }), code('search_limit'));
  await assert.rejects(loadIndex('https://example.invalid/search.json', {
    ...limits, allowNetwork: true, maxResponseBytes: MAX_SEARCH_BYTES + 1, timeoutMs: 1000,
  }), code('search_limit'));
});

test('explicit emitted fields rank only selected plain values and retain full immutable article metadata', () => {
  const article = record('article', 'unselectedtitle', 'unselectedbody', {
    notes: { body_html: '<p>noteshtml</p>', body_text: 'commentarytoken' },
    indexed: {
      description: 'descriptiontoken', keywords: ['keywordtoken'],
      route: 'https://example.test/routeword/', notes: 'commentarytoken',
    },
  });
  article.content.description = 'descriptiontoken';
  article.content.keywords = ['keywordtoken'];
  const index = createIndex([article], limits);
  for (const [field, query] of [['description', 'descriptiontoken'], ['keywords', 'keywordtoken'], ['route', 'https://example.test/routeword/'], ['notes', 'commentarytoken']]) {
    const hits = index.search(query, { limit: 1, fields: { [field]: 1 } });
    assert.equal(hits.length, 1);
    assert.equal(hits[0].record.content.id, article.content.id);
    assert.equal(hits[0].record.route.url, article.route.url);
    assert.equal(hits[0].record.content.title, 'unselectedtitle');
    assert.equal(hits[0].record.body, 'unselectedbody');
    assert.equal(hits[0].record.notes.body_html, '<p>noteshtml</p>');
    assert.ok(Object.isFrozen(hits[0].record.indexed));
  }
  for (const query of ['unselectedtitle', 'unselectedbody', 'noteshtml']) {
    assert.equal(index.search(query, { limit: 1 }).length, 0, 'complete metadata is not unselected ranking input');
  }
  article.indexed.notes = 'changed';
  assert.equal(index.search('commentarytoken', { limit: 1 }).length, 1);
  assert.equal(index.size, 1, 'commentary has no second document identity');
});

test('emitted ranking projection is nonempty closed typed data and cannot weaken required identity metadata', () => {
  for (const indexed of [{}, null, [], { invented: 'value' }, { notes: '<valid>', keywords: 'not-array' }, { body: false }, { notes: true }, { projects: [null] }]) {
    assert.throws(() => createIndex([record('bad', 'Title', 'Body', { indexed })], limits), code('search_data'));
  }
  const missing = record('missing', 'Title', 'Body', { indexed: { notes: 'note' } });
  delete missing.content.title;
  assert.throws(() => createIndex([missing], limits), code('search_data'));
});

test('a retained body reference ranks the same text as the explicit field', () => {
  const body = 'Analysis of a stable lexical signature';
  const document = record('body-reference', 'Title', body);
  const search = indexed => createIndex([{ ...document, indexed }], limits)
    .search('signature', { limit: 1, fields: { body: 1 } });
  const [compact] = search({ body: true });
  const [explicit] = search({ body });
  assert.ok(compact);
  assert.deepEqual({ ...compact, record: { ...compact.record, indexed: { body } } },
    { ...explicit, record: { ...explicit.record, indexed: { body } } });
  assert.equal(search({ title: 'Title' }).length, 0);
});

test('authored keywords are searchable without inferred classification or mutable metadata', () => {
  const article = record('article', 'An inquiry', 'Its argument');
  article.content.keywords = ['contrafactual models', 'typed graphs'];
  article.content.schema_type = 'TechArticle';
  const index = createIndex([article, record('other', 'Other', 'Other body')], limits);
  const hit = index.search('contrafactual', { limit: 1, fields: { keywords: 2 } })[0];
  assert.equal(hit.record.content.id, article.content.id);
  assert.deepEqual([...hit.record.content.keywords], ['contrafactual models', 'typed graphs']);
  assert.equal(hit.record.content.schema_type, 'TechArticle');
  assert.equal(Object.hasOwn(hit.record.content, 'taxonomies'), false);
  article.content.keywords[0] = 'changed';
  assert.equal(index.search('contrafactual', { limit: 1 }).length, 1);
  assert.equal(index.search('changed', { limit: 1 }).length, 0);
  for (const keywords of ['not an array', [true], [''], null]) {
    const invalid = record('invalid', 'Valid', 'Body');
    invalid.content.keywords = keywords;
    assert.throws(() => createIndex([invalid], limits), code('search_data'));
  }
});

test('real provider indexes emitted field paths and returns original immutable metadata', () => {
  const input = documents();
  const before = JSON.stringify(input);
  const index = createIndex(input, limits);
  assert.equal(index.size, 3);
  assert.equal(index.profile, 'lexical');
  const hits = index.search('publication compiler', { limit: 2, fields: { title: 10, body: 1 } });
  assert.deepEqual(hits.map(hit => hit.record.content.id), ['content:compiler', 'content:body']);
  assert.ok(hits.every(hit => Number.isFinite(hit.score) && hit.score > 0));
  assert.equal(JSON.stringify(hits[0].record), JSON.stringify(input[0]));
  assert.equal(index.search('Gazette', { limit: 10, fields: { projects: 1 } })[0].record.content.id, 'content:compiler');
  assert.equal(index.search('retained', { limit: 10, fields: { summary: 1 } })[0].record.content.id, 'content:compiler');
  assert.equal(JSON.stringify(input), before);
  input[0].route.canonical_url = 'javascript:alert(1)';
  input[0].content.title = 'changed';
  assert.equal(hits[0].record.route.canonical_url, 'https://example.test/mount/compiler/');
  assert.equal(hits[0].record.content.title, 'Publication compiler');
  assert.ok(Object.isFrozen(hits) && Object.isFrozen(hits[0]) && Object.isFrozen(hits[0].record.content));
  assert.throws(() => { hits[0].record.content.title = 'mutated'; }, TypeError);
});

test('explicit profiles, language retention, query choices and stable ties', () => {
  const input = [record('b', 'Same', 'running'), record('a', 'Same', 'running')];
  delete input[0].content.language;
  const lexical = createIndex(input, limits);
  const english = createIndex(input, { ...limits, profile: 'english' });
  assert.equal(lexical.search('run', { limit: 2 }).length, 0);
  assert.equal(english.search('run', { limit: 2 }).length, 2);
  assert.deepEqual(lexical.search('same', { limit: 2 }).map(hit => hit.record.content.id), ['content:a', 'content:b']);
  assert.equal(lexical.search('same absent', { limit: 2 }).length, 0);
  assert.equal(lexical.search('same absent', { limit: 2, bool: 'OR' }).length, 2);
  assert.equal(lexical.search('runn', { limit: 2, expand: true }).length, 2);
  const hit = english.search('run', { limit: 2 }).find(hit => hit.record.content.id === 'content:b');
  assert.equal(Object.hasOwn(hit.record.content, 'language'), false);
  assert.deepEqual(lexical.search('  ', { limit: 2 }), []);
  assert.deepEqual(createIndex([], limits).search('anything', { limit: 1 }), []);
});

test('AND combines selected fields, retains profile processing and respects prefix policy', () => {
  const input = [record('split', 'Kestrel', 'Durable recovery'),
    record('title', 'Kestrel recovery', 'Other'), record('partial', 'Kestrel', 'Other')];
  for (const profile of ['lexical', 'english']) {
    const index = createIndex(input, { ...limits, profile });
    const ids = query => index.search(query, { limit: 10, fields: { title: 2, body: 1 } })
      .map(hit => hit.record.content.id).sort();
    assert.deepEqual(ids('Kestrel recovery'), ['content:split', 'content:title']);
    assert.deepEqual(ids('Kestrel recovery recovery'), ['content:split', 'content:title']);
    assert.deepEqual(index.search('Kestrel recovery', { limit: 10, fields: { title: 1 } })
      .map(hit => hit.record.content.id), ['content:title']);
    assert.equal(index.search('Kestrel absent', { limit: 10 }).length, 0);
    assert.equal(index.search('Kestrel absent', { limit: 10, bool: 'OR' }).length, 3);
    assert.equal(index.search('kest recov', { limit: 10 }).length, 0);
    assert.equal(index.search('kest recov', { limit: 10, expand: true }).length, 2);
    if (profile === 'english') {
      assert.deepEqual(ids('the Kestrel recovery'), ['content:split', 'content:title']);
      assert.deepEqual(ids('the and'), []);
    }
  }
});

test('full indexed titles precede mentions without promoting partial or unselected titles', () => {
  const titled = record('z-title', 'Kestrel', 'Kestrel long context '.repeat(50));
  const mention = record('a-mention', 'Reference', 'Kestrel');
  const index = createIndex([mention, titled], limits);
  const ranked = index.search('  KESTREL  ', { limit: 2, fields: { title: 1, body: 10 } });
  assert.equal(ranked[0].record.content.id, titled.content.id);
  assert.ok(ranked[0].score < ranked[1].score, 'full-title preference is independent of provider score');
  assert.equal(index.search('Kest', { limit: 1, expand: true, fields: { title: 1, body: 10 } })[0].record.content.id, mention.content.id);
  assert.equal(index.search('Kestrel', { limit: 1, fields: { body: 1 } })[0].record.content.id, mention.content.id);
  assert.equal(index.search('Kest', { limit: 1, expand: true, fields: { body: 1 } })[0].record.content.id, mention.content.id);
  const hidden = { ...titled, indexed: { body: titled.body } };
  const excluded = createIndex([mention, hidden], limits);
  assert.equal(excluded.search('Kestrel', { limit: 1 })[0].record.content.id, mention.content.id);
  const phrase = createIndex([record('z', 'Kestrel Recovery', 'Long context'),
    record('a', 'Other', 'Kestrel recovery')], limits);
  assert.equal(phrase.search('kestrel   RECOVERY', { limit: 1 })[0].record.content.id, 'content:z');
});

test('English prefix matching keeps every literal typing step alongside stemmed matches', () => {
  const input = [record('coordination', 'Coordinating emergencies', 'Shared responsibility'),
    record('neutral', 'Recovering observations', 'Kestrel responsibilities'),
    record('other', 'Unrelated', 'Cooking')];
  const index = createIndex(input, { ...limits, profile: 'english' });
  for (const [word, expected] of [['coordinating', 'content:coordination'],
    ['responsibility', 'content:coordination'], ['recovering', 'content:neutral']]) {
    for (let length = 3; length <= word.length; length++) {
      const query = word.slice(0, length);
      assert.ok(index.search(query, { limit: 10, expand: true }).some(hit => hit.record.content.id === expected), query);
    }
  }
  assert.equal(index.search('coordination', { limit: 10, expand: true })[0].record.content.id, 'content:coordination');
  assert.equal(index.search('coordination', { limit: 10, expand: false })[0].record.content.id, 'content:coordination');
  assert.equal(index.search('coordinati', { limit: 10, expand: false }).length, 0);
  for (const query of ['coordinatix', 'coordinationzz', 'responsibilix', 'recoveringxyz']) {
    assert.equal(index.search(query, { limit: 10, expand: true }).length, 0, query);
  }
  assert.equal(index.search('coordinati', { limit: 10, expand: true, fields: { body: 1 } }).length, 0);
  const hidden = { ...input[0], indexed: { body: input[0].body } };
  assert.equal(createIndex([hidden], { ...limits, profile: 'english' })
    .search('coordinati', { limit: 10, expand: true }).length, 0);
  const cross = { limit: 10, expand: true, fields: { title: 1, body: 1 } };
  assert.equal(index.search('the recoveri kestrel', cross)[0].record.content.id, 'content:neutral');
  assert.equal(index.search('recoveri absent', cross).length, 0);
  assert.equal(index.search('recoveri absent', { ...cross, bool: 'OR' })[0].record.content.id, 'content:neutral');
});

test('English AND keeps distinct literal prefixes even when their stems coincide', () => {
  const index = createIndex([record('busybody', 'Busybody', 'Kestrel'),
    record('business', 'Business', 'Kestrel')], { ...limits, profile: 'english' });
  for (const query of ['busy business', 'business busy', 'busy business kestrel', 'business busy kestrel']) {
    assert.deepEqual(index.search(query, { limit: 10, expand: true }).map(hit => hit.record.content.id),
      ['content:business'], query);
  }
  assert.equal(index.search('business busy', { limit: 10, expand: true, bool: 'OR' }).length, 2);
});

test('whole dataset validates before provider construction; identities and URLs cannot alias', () => {
  assert.throws(() => createIndex([...documents(), null], limits), code('search_data'));
  assert.throws(() => createIndex([documents()[0], documents()[0]], limits), code('search_duplicate'));
  const duplicate = record('different', 'Other', 'Body');
  duplicate.route.url = documents()[0].route.url;
  assert.throws(() => createIndex([documents()[0], duplicate], limits), code('search_duplicate'));
  for (const url of ['javascript:alert(1)', '/relative/', 'https://user:pass@example.test/x/',
    'https://example.test/x/#fragment', 'https://example.test/a\\b/', 'https://example.test/a%0ab/']) {
    for (const field of ['url', 'canonical_url']) {
      const invalid = record('invalid', 'Valid', 'Body');
      invalid.route[field] = url;
      assert.throws(() => createIndex([invalid], limits), code('search_url'));
    }
  }
  for (const field of ['url', 'canonical_url']) {
    const absent = record('absent', 'Valid', 'Body');
    delete absent.route[field];
    assert.throws(() => createIndex([absent], limits), code('search_url'));
  }
  const missing = record('missing', 'Valid', '');
  delete missing.body;
  assert.throws(() => createIndex([missing], limits), code('search_data'));
});

test('canonical copies rank independently by serving address and retain foreign canonical metadata', () => {
  const original = 'https://original.test/reference/shared/';
  const input = [record('b', 'Shared publication', 'Same body'), record('a', 'Shared publication', 'Same body')];
  for (const item of input) item.route.canonical_url = original;
  const index = createIndex(input, limits);
  assert.equal(index.size, 2, 'shared canon does not collapse distinct serving pages');
  const hits = index.search('shared publication', { limit: 2, fields: { title: 1 } });
  assert.deepEqual(hits.map(hit => hit.record.route.url), [
    'https://example.test/mount/a/', 'https://example.test/mount/b/',
  ]);
  assert.ok(hits.every(hit => hit.record.route.canonical_url === original));
  assert.equal(hits[0].score, hits[1].score, 'serving address resolves an actual provider ranking tie');
  input[1].route.url = 'https://changed.test/';
  input[1].route.canonical_url = 'https://changed-canon.test/';
  assert.equal(hits[0].record.route.url, 'https://example.test/mount/a/');
  assert.equal(hits[0].record.route.canonical_url, original);
});

test('independent dataset, nested JSON, query and result limits fail explicitly', () => {
  assert.throws(() => createIndex(documents(), { ...limits, maxDocuments: 2 }), code('search_limit'));
  assert.throws(() => createIndex(documents(), { ...limits, maxBytes: 100 }), code('search_limit'));
  const nested = record('nested', 'Nested', 'body');
  let cursor = nested;
  for (let i = 0; i < 66; i++) { cursor.next = {}; cursor = cursor.next; }
  assert.throws(() => createIndex([nested], limits), code('search_limit'));
  const index = createIndex(documents(), { ...limits, maxQueryBytes: 4 });
  assert.throws(() => index.search('🙂🙂', { limit: 1 }), code('search_limit'));
  assert.throws(() => index.search('a', { limit: 11 }), code('search_limit'));
  assert.throws(() => index.search('a', { limit: 1, fields: { body: 0 } }), code('search_options'));
  assert.throws(() => index.search('a', { limit: 1, fields: null }), code('search_options'));
  assert.throws(() => index.search('a', { limit: 1, bool: 'XOR' }), code('search_options'));
  assert.throws(() => createIndex([], { ...limits, profile: undefined }), code('search_profile'));
  assert.throws(() => createIndex([], { ...limits, typo: 1 }), code('search_options'));
});

test('JSON capture does not execute accessors or toJSON and does not permit cycles', () => {
  let called = false;
  const accessor = record('accessor', 'Accessor', 'body');
  Object.defineProperty(accessor, 'secret', { enumerable: true, get() { called = true; return 'no'; } });
  assert.throws(() => createIndex([accessor], limits), code('search_data'));
  assert.equal(called, false);
  const custom = record('custom', 'Custom', 'body');
  custom.toJSON = () => { called = true; return {}; };
  assert.throws(() => createIndex([custom], limits), code('search_data'));
  assert.equal(called, false);
  const circular = record('circle', 'Circle', 'body');
  circular.self = circular;
  assert.throws(() => createIndex([circular], limits), code('search_data'));
});

test('strict ESM provider keeps globals untouched and retains exact algorithm provenance', async () => {
  assert.equal(Object.hasOwn(globalThis, 'elasticlunr'), false);
  assert.equal(Object.hasOwn(globalThis, 'lunr'), false);
  const source = await readFile(new URL('../search/vendor/elasticlunr.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('/**\n * elasticlunr -');
  const end = source.lastIndexOf('}.call({ console: globalThis.console });');
  assert.ok(start > 0 && end > start);
  const original = source.slice(start, end) + '}();\n';
  assert.equal(createHash('sha256').update(original).digest('hex'), '4f648b9e42abdef9c436e68776eca79c23596c798a9ef1400cf538d8a218b8cd');
  for (const notice of ['@license', 'Copyright (C) 2017 Oliver Nightingale', 'Copyright (C) 2017 Wei Song',
    'Permission is hereby granted', 'THE SOFTWARE IS PROVIDED "AS IS"']) assert.ok(source.includes(notice));
});

test('native HTTP loads once from selected mount and never requests linked metadata', async t => {
  const calls = [];
  const server = createServer((request, response) => {
    calls.push(request.url);
    response.writeHead(200, { 'content-type': 'application/json; charset=utf-8' });
    response.end(JSON.stringify(documents()));
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => { server.closeAllConnections(); server.close(); });
  const exact = `http://127.0.0.1:${server.address().port}/mount/search.json`;
  const index = await loadIndex(exact, { ...limits, allowNetwork: true, maxResponseBytes: 32768, timeoutMs: 2000 });
  assert.equal(index.search('compiler', { limit: 2 }).length, 2);
  assert.deepEqual(calls, ['/mount/search.json']);
  await assert.rejects(loadIndex(exact, { ...limits, allowNetwork: false, maxResponseBytes: 32768, timeoutMs: 2000 }), code('search_permission'));
  assert.deepEqual(calls, ['/mount/search.json']);
});

test('native HTTP refuses redirects, denied responses, bad data, overflow and timeout without fallback', async t => {
  const calls = [];
  const server = createServer((request, response) => {
    calls.push(request.url);
    if (request.url === '/redirect') { response.writeHead(302, { location: '/ok' }); response.end(); return; }
    if (request.url === '/denied') { response.writeHead(403); response.end('private body'); return; }
    if (request.url === '/not-modified') { response.writeHead(304); response.end(); return; }
    if (request.url === '/slow') return;
    response.writeHead(200, { 'content-type': request.url === '/mime' ? 'text/html' : 'application/json' });
    response.flushHeaders();
    if (request.url === '/bad') response.end('{private-invalid');
    else if (request.url === '/utf8') response.end(Buffer.from([0xff]));
    else if (request.url === '/large') response.end(' '.repeat(5000));
    else response.end(JSON.stringify(documents()));
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => { server.closeAllConnections(); server.close(); });
  const base = `http://127.0.0.1:${server.address().port}`;
  const options = { ...limits, allowNetwork: true, maxResponseBytes: 4096, timeoutMs: 1000 };
  for (const [path, expected] of [['redirect', 'search_network'], ['denied', 'search_response'],
    ['not-modified', 'search_response'],
    ['mime', 'search_response'], ['bad', 'search_data'], ['utf8', 'search_data'], ['large', 'search_limit']]) {
    await assert.rejects(loadIndex(`${base}/${path}`, options), code(expected));
  }
  await assert.rejects(loadIndex(`${base}/slow`, { ...options, timeoutMs: 30 }), code('search_timeout'));
  assert.deepEqual(calls, ['/redirect', '/denied', '/not-modified', '/mime', '/bad', '/utf8', '/large', '/slow']);
});

test('acquisition distinguishes a progressing corpus from a stalled or overlong response', async t => {
  const payload = JSON.stringify(documents());
  const server = createServer((request, response) => {
    response.writeHead(200, { 'content-type': 'application/json' });
    response.write(payload.slice(0, 1));
    if (request.url === '/stall') return;
    let offset = 1;
    const timer = setInterval(() => {
      if (request.url === '/long') { response.write(' '); return; }
      const cut = offset + Math.ceil(payload.length / 5);
      response.write(payload.slice(offset, cut));
      offset = cut;
      if (offset >= payload.length) response.end();
    }, 50);
    response.on('close', () => clearInterval(timer));
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => { server.closeAllConnections(); server.close(); });
  const base = `http://127.0.0.1:${server.address().port}`;
  const options = { ...limits, allowNetwork: true, maxResponseBytes: 32768, timeoutMs: 600, idleTimeoutMs: 180 };
  assert.equal((await loadIndex(`${base}/progress`, options)).size, documents().length);
  const stalled = performance.now();
  await assert.rejects(loadIndex(`${base}/stall`, { ...options, timeoutMs: 1200 }), code('search_timeout'));
  assert.ok(performance.now() - stalled < 900);
  await assert.rejects(loadIndex(`${base}/long`, options), code('search_timeout'));
});
