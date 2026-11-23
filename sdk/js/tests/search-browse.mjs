import test from 'node:test';
import assert from 'node:assert/strict';
import { createIndex, SearchError } from '../search/index.mjs';

const bounds = { profile: 'lexical', maxDocuments: 100, maxBytes: 100000,
  maxQueryBytes: 1024, maxResults: 5 };
const record = index => ({
  content: { id: `content:item-${index}`, title: 'Shared query', site: index < 24 ? 'site:first' : 'site:last',
    structure: { kind: 'page', family: { kind: 'article' } },
    ...(index % 2 === 0 ? { published: `2026-10-${String(index + 1).padStart(2, '0')}` } : {}) },
  route: { url: `https://example.test/${String(index).padStart(2, '0')}/`, canonical_url: `https://example.test/${index}/` },
  body: 'Shared query', projects: [{ project: { id: index === 24 ? 'project:tail' : 'project:common' } }],
  attributions: [{ role: 'rights-holder', principal: { id: 'person:not-author' } },
    { role: 'author', principal: { id: 'person:author' } }, { role: 'author', principal: { id: 'person:author' } }],
  classifications: [{ taxonomy: { id: 'taxonomy:one' }, term: { id: 'term:shared' } },
    { taxonomy: { id: 'taxonomy:two' }, term: { id: 'term:shared' } }]
});

test('native documentation category keeps manuals searchable without editorial kind facets', () => {
  const manual = record(0), task = record(1), example = record(2), editorial = record(3), legacy = record(4);
  manual.content.structure = { kind: 'collection', purpose: 'documentation' };
  task.content.structure = { kind: 'journey' };
  for (const item of [manual, task, example]) item.content.documentation = true;
  editorial.content.structure = { kind: 'collection' };
  legacy.content.structure = { kind: 'page', family: { kind: 'documentation' } };
  const index = createIndex([manual, task, example, editorial, legacy], bounds);
  const docs = index.browse('', { limit: 5, filters: { kind: ['documentation'] } });
  assert.equal(docs.total, 4);
  assert.deepEqual(docs.facets.kind, [{ value: 'documentation', count: 4 }]);
  assert.equal(index.browse('', { limit: 5, filters: { kind: ['collection'] } }).hits[0].record.content.id, editorial.content.id);
  assert.equal(index.browse('', { limit: 5, filters: { kind: ['article'] } }).total, 0);
  assert.equal(index.size, 5, 'documentation is discoverable, not suppressed');
});

test('facet labels retain only unambiguous supplied native metadata', () => {
  const first = record(0), second = record(1);
  for (const item of [first, second]) {
    item.site = { id: 'site:first', metadata: { brand: { name: 'Example publication' } } };
    item.projects[0].project.name = 'Example project';
    item.attributions[1].principal.name = 'Example author';
    item.classifications[0].taxonomy.label = 'Subject';
    item.classifications[0].term.label = 'Shared';
  }
  const facets = createIndex([first, second], bounds).browse('', { limit: 5 }).facets;
  assert.deepEqual(facets.site, [{ value: 'site:first', count: 2, label: 'Example publication' }]);
  assert.deepEqual(facets.author, [{ value: 'person:author', count: 2, label: 'Example author' }]);
  assert.deepEqual(facets.project, [{ value: 'project:common', count: 2, label: 'Example project' }]);
  assert.deepEqual(facets.classification[0], { taxonomy: 'taxonomy:one', term: 'term:shared', count: 2,
    taxonomy_label: 'Subject', term_label: 'Shared' });
  assert.deepEqual(facets.classification[1], { taxonomy: 'taxonomy:two', term: 'term:shared', count: 2 });
  second.attributions[1].principal.name = 'Conflicting author';
  second.site = { id: 'site:other', metadata: { brand: { name: 'Unrelated publication' } } };
  const conflicting = createIndex([first, second], bounds).browse('', { limit: 5 }).facets;
  assert.deepEqual(conflicting.author, [{ value: 'person:author', count: 2 }]);
  assert.deepEqual(conflicting.site, [{ value: 'site:first', count: 2, label: 'Example publication' }]);
});

test('complete matching corpus is filtered and counted before any result slice', () => {
  const index = createIndex(Array.from({ length: 25 }, (_, i) => record(i)), bounds);
  assert.equal(index.search('shared query', { limit: 5 }).length, 5);
  const tail = index.browse('shared query', { limit: 1, filters: { site: ['site:last'], project: ['project:tail'] } });
  assert.equal(tail.total, 1);
  assert.equal(tail.hits[0].record.content.id, 'content:item-24');
  assert.deepEqual(tail.facets.site, [{ value: 'site:last', count: 1 }]);
  const all = index.browse('shared query', { limit: 5 });
  assert.equal(all.total, 25);
  assert.deepEqual(all.facets.author, [{ value: 'person:author', count: 25 }]);
  assert.equal(all.facets.classification.length, 2, 'equal term keys in separate taxonomies do not merge');
  assert.equal(index.browse('shared query', { limit: 5, filters: { author: ['person:not-author'] } }).total, 0);
});

test('native kind, author and composite classification filters combine exactly', () => {
  const article = record(0), collection = record(1);
  collection.content.structure = { kind: 'collection' };
  collection.classifications = [collection.classifications[1]];
  const index = createIndex([article, collection], bounds);
  assert.equal(index.browse('', { limit: 5, filters: { kind: ['article'], author: ['person:author'],
    classification: [{ taxonomy: 'taxonomy:one', term: 'term:shared' }] } }).hits[0].record.content.id, article.content.id);
  assert.equal(index.browse('', { limit: 5, filters: { classification: [{ taxonomy: 'taxonomy:one', term: 'term:shared' }] } }).total, 1);
  assert.equal(index.browse('', { limit: 5, filters: { kind: ['Article'] } }).total, 0, 'no label/case inference');
});

test('date sorting covers all matches, places undated last and keeps deterministic pagination', () => {
  const input = Array.from({ length: 8 }, (_, i) => record(i));
  const index = createIndex(input, bounds);
  const latest = index.browse('', { limit: 3, sort: 'latest' });
  assert.equal(latest.total, 8);
  assert.deepEqual(latest.hits.map(hit => hit.record.content.id), ['content:item-6', 'content:item-4', 'content:item-2']);
  assert.deepEqual(index.browse('', { limit: 3, offset: 3, sort: 'latest' }).hits.map(hit => hit.record.content.id),
    ['content:item-0', 'content:item-1', 'content:item-3']);
  assert.equal(index.browse('', { limit: 1, sort: 'oldest' }).hits[0].record.content.id, 'content:item-0');
  const emptyPage = index.browse('', { limit: 3, offset: 80 });
  assert.equal(emptyPage.total, 8);assert.deepEqual(emptyPage.hits, []);
  assert.deepEqual(index.search('', { limit: 3 }), [], 'existing command-search empty-query contract survives');
  assert.equal(Object.hasOwn(latest.hits[0].record.content, 'modified'), false);
  assert.ok(Object.isFrozen(latest) && Object.isFrozen(latest.facets) && Object.isFrozen(latest.facets.author[0]));
});

test('browse accepts only bounded declarative choices and never invokes accessors', () => {
  const index = createIndex([record(0)], bounds);
  const invalid = value => assert.throws(() => index.browse('', { limit: 1, ...value }), SearchError);
  for (const value of [{ limit: 6 }, { offset: -1 }, { offset: 101 }, { sort: 'random' },
    { offset: null }, { filters: null }, { sort: null },
    { filters: { unknown: ['x'] } }, { filters: { site: [] } }, { filters: { site: ['x', 'x'] } },
    { filters: { site: Array(33).fill('x') } }, { filters: { classification: ['term:shared'] } }]) invalid(value);
  let invoked = false;
  const getter = [];Object.defineProperty(getter, '0', { get() { invoked = true;return 'site:first'; }, enumerable: true });
  invalid({ filters: { site: getter } });assert.equal(invoked, false);
  const options = { limit: 1 };Object.defineProperty(options, 'sort', { get() { invoked = true;return 'latest'; }, enumerable: true });
  assert.throws(() => index.browse('', options), SearchError);assert.equal(invoked, false);
});

test('impossible or nonnative date forms never acquire a normalized publication date', () => {
  const input = Array.from({ length: 4 }, (_, i) => record(i));
  for (const [index, value] of ['2026-02-30', '2026-03-01', '2026-03-02T24:00:00Z', '2026-03-02'].entries()) input[index].content.published = value;
  const index = createIndex(input, bounds);
  assert.deepEqual(index.browse('', { limit: 5, sort: 'latest' }).hits.map(hit => hit.record.content.id),
    ['content:item-3', 'content:item-1', 'content:item-0', 'content:item-2']);
});

test('mixed precision sorts complete publication days without inventing unknown times', () => {
  const values = ['2026-10-05', '2026-10-06T00:30:00+14:00', '2026-10-04T20:30:00-14:00',
    '2026-10-05T20:00:00.000000001Z', '2026-10-06', '2026-10-04', undefined, '2026-02-30'];
  const input = values.map((value, i) => {
    const item = record(i);
    if (value === undefined) delete item.content.published;
    else item.content.published = value;
    return item;
  });
  const index = createIndex(input, bounds);
  const identities = order => order.map(i => `content:item-${i}`);
  for (const [sort, order] of [['latest', [4, 3, 1, 2, 0, 5, 6, 7]], ['oldest', [5, 1, 2, 3, 0, 4, 6, 7]]]) {
    const first = index.browse('', { limit: 5, sort });
    const rest = index.browse('', { limit: 5, offset: 5, sort });
    assert.equal(first.total, values.length);
    const hits = [...first.hits, ...rest.hits];
    assert.deepEqual(hits.map(hit => hit.record.content.id), identities(order));
    assert.deepEqual(hits.map(hit => hit.record.content.published), order.map(i => values[i]),
      'ordering preserves every original date and offset, including absent or invalid values');
  }
  assert.equal(index.browse('', { limit: 5 }).total, values.length, 'relevance is unchanged');
  input[0].content.site = 'site:only-date';
  const filtered = createIndex(input, bounds).browse('', { limit: 1, sort: 'latest', filters: { site: ['site:only-date'] } });
  assert.equal(filtered.total, 1);
  assert.equal(filtered.hits[0].record.content.published, values[0]);
});

test('chronology retains native submillisecond precision and equivalent timezone instants', () => {
  const input = Array.from({ length: 4 }, (_, i) => record(i));
  for (const [index, value] of ['2026-10-01T00:00:00.000000001Z', '2026-10-01T00:00:00.000000002Z',
    '2026-10-01T05:30:00.000000002+05:30', '2026-10-01T00:00:00.0000000009Z'].entries()) input[index].content.published = value;
  const index = createIndex(input, bounds);
  assert.deepEqual(index.browse('', { limit: 5, sort: 'latest' }).hits.map(hit => hit.record.content.id),
    ['content:item-1', 'content:item-2', 'content:item-0', 'content:item-3']);
  assert.deepEqual(index.browse('', { limit: 5, sort: 'oldest' }).hits.map(hit => hit.record.content.id),
    ['content:item-3', 'content:item-0', 'content:item-1', 'content:item-2']);
});

test('authored keyword filters are exact phrases, independent of ranked text, taxonomies and projects', () => {
  const first = record(0), second = record(1), third = record(2);
  first.content.keywords = ['AI / modèles + mémoire', 'Shared query'];
  second.content.keywords = ['ai / modèles + mémoire', 'AI / modèles + mémoire'];
  third.content.keywords = ['AI / modèles + mémoire suffix'];
  const index = createIndex([first, second, third], bounds);
  const target = 'AI / modèles + mémoire';
  const initial = index.browse('', { limit: 5 });
  assert.deepEqual(initial.facets.keyword, [
    { value: target, count: 2, label: target },
    { value: `${target} suffix`, count: 1, label: `${target} suffix` },
    { value: 'Shared query', count: 1, label: 'Shared query' },
    { value: 'ai / modèles + mémoire', count: 1, label: 'ai / modèles + mémoire' },
  ]);
  const filtered = index.browse('', { limit: 1, filters: { keyword: [target] } });
  assert.equal(filtered.total, 2, 'phrase counts cover the full eligible population, not its slice');
  assert.deepEqual(filtered.hits.map(hit => hit.record.content.id), [first.content.id]);
  assert.deepEqual(index.browse('', { limit: 1, offset: 1, filters: { keyword: [target] } }).hits.map(hit => hit.record.content.id), [second.content.id]);
  assert.equal(index.browse('', { limit: 5, filters: { keyword: ['modèles'] } }).total, 0, 'ranked partial tokens are not authored keyword identities');
  assert.equal(index.browse('', { limit: 5, filters: { keyword: ['shared query'] } }).total, 0, 'case is not normalized');
  assert.equal(index.browse('', { limit: 5, filters: { keyword: ['project:common'] } }).total, 0);
  assert.equal(index.browse('', { limit: 5, filters: { keyword: ['term:shared'] } }).total, 0);
  assert.equal(index.browse('absent', { limit: 5, filters: { keyword: [target] } }).total, 0, 'free text independently narrows the exact keyword selection');
  assert.equal(index.browse('', { limit: 5, filters: { keyword: ['Shared query', 'ai / modèles + mémoire'] } }).total, 2, 'values within the keyword dimension are OR');
  assert.equal(index.browse('', { limit: 5, filters: { keyword: [target], site: ['site:last'] } }).total, 0, 'different dimensions are AND');
  const url = new URL('https://example.test/search/');url.searchParams.set('keyword', target);
  assert.equal(index.browse('', { limit: 5, filters: { keyword: [new URL(url).searchParams.get('keyword')] } }).total, 2, 'standard query encoding round-trips the exact phrase');
  assert.deepEqual(filtered.hits[0].record.content.keywords, first.content.keywords);
  assert.ok(Object.isFrozen(filtered.facets.keyword) && Object.isFrozen(filtered.facets.keyword[0]));
});

test('keyword counting deduplicates per record and does not change native ranking or source data', () => {
  const first = record(0), second = record(1);
  // Authored Gazette content rejects duplicate keywords; the SDK still counts
  // any repeated value in an independently supplied JSON snapshot only once.
  first.content.keywords = ['exact phrase', 'exact phrase'];
  second.content.keywords = ['exact phrase'];
  first.indexed = { title: 'Shared query' };second.indexed = { title: 'Shared query' };
  const original = JSON.stringify([first, second]);
  const index = createIndex([first, second], bounds);
  const ranked = index.search('shared', { limit: 5 });
  const filtered = index.browse('shared', { limit: 5, filters: { keyword: ['exact phrase'] } });
  assert.deepEqual(filtered.hits, ranked, 'filtering preserves existing eligible ranking scores and tie order');
  assert.deepEqual(filtered.facets.keyword, [{ value: 'exact phrase', count: 2, label: 'exact phrase' }]);
  assert.equal(index.search('exact phrase', { limit: 5 }).length, 0, 'filter membership does not add an unselected ranking field');
  first.content.keywords[0] = 'mutated';
  assert.equal(index.browse('', { limit: 5, filters: { keyword: ['exact phrase'] } }).total, 2);
  assert.equal(index.browse('', { limit: 5, filters: { keyword: ['mutated'] } }).total, 0);
  assert.equal(JSON.stringify(filtered.hits.map(hit => hit.record)), original);
});

test('keyword choices retain existing filter bounds and reject accessors without invocation', () => {
  const index = createIndex([record(0)], bounds);
  for (const keyword of [[], [''], ['a', 'a'], ['x'.repeat(513)], Array.from({ length: 33 }, (_, i) => String(i)), [{ taxonomy: 'a', term: 'b' }]]) {
    assert.throws(() => index.browse('', { limit: 1, filters: { keyword } }), SearchError);
  }
  let invoked = false;
  const keyword = [];
  Object.defineProperty(keyword, '0', { enumerable: true, get() { invoked = true; return 'value'; } });
  assert.throws(() => index.browse('', { limit: 1, filters: { keyword } }), SearchError);
  assert.equal(invoked, false);
  assert.deepEqual(index.browse('', { limit: 1 }).facets.keyword, [], 'absent authored keywords do not become empty invented values');
});


test('cross-field matches survive filters and pagination before the relevance slice', () => {
  const input = Array.from({ length: 25 }, (_, i) => record(i));
  for (const item of input) { item.content.title = 'Kestrel'; item.body = 'Durable recovery'; }
  const index = createIndex(input, bounds);
  const result = index.browse('kestrel recovery', { limit: 1, filters: { site: ['site:last'] } });
  assert.equal(result.total, 1);
  assert.equal(result.hits[0].record.content.id, 'content:item-24');
  assert.equal(index.browse('kestrel recovery', { limit: 1, offset: 24 }).hits[0].record.content.id,
    'content:item-24');
  assert.equal(index.browse('kest recov', { limit: 1, expand: true }).total, 25);
});


test('English literal prefixes keep complete filtered counts and pagination', () => {
  const input = Array.from({ length: 25 }, (_, i) => record(i));
  for (const item of input) { item.content.title = 'Coordinating emergencies'; item.body = 'Kestrel'; }
  const index = createIndex(input, { ...bounds, profile: 'english' });
  for (const query of ['coord', 'coordin', 'coordina', 'coordinat', 'coordinati', 'coordination']) {
    const result = index.browse(query + ' kestrel', { limit: 1, expand: true, filters: { site: ['site:last'] } });
    assert.equal(result.total, 1, query);
    assert.equal(result.hits[0].record.content.id, 'content:item-24');
    assert.equal(index.browse(query, { limit: 1, expand: true, offset: 24 }).hits[0].record.content.id, 'content:item-24');
  }
});


test('provider documentation preserves canonical identity, preview destination and missing native title', () => {
  const canonical = 'https://docs.example.test/library/reference';
  const page = {
    content: { id: canonical, title: 'library/reference', site: 'site:library', documentation: true },
    route: { url: 'http://127.0.0.1:43121/library/reference', canonical_url: canonical },
    body: 'Qualified provider wording',
    projects: [{ project: { id: 'project:library', name: 'Library' } }],
    documentation_page: { module: 'library', route: 'library/reference', title: null,
      label_source: 'reading-route', body_available: true },
    destination: { kind: 'external' },
    indexed: { title: 'library/reference', body: 'Qualified provider wording' }
  };
  const index = createIndex([page], bounds);
  const found = index.browse('provider', { limit: 1, filters: { kind: ['documentation'], project: ['project:library'] } });
  assert.equal(found.total, 1);
  assert.equal(found.hits[0].record.content.id, canonical);
  assert.equal(found.hits[0].record.route.url, page.route.url);
  assert.equal(found.hits[0].record.documentation_page.title, null);
  assert.equal(found.hits[0].record.destination.kind, 'external');
  assert.equal(Object.hasOwn(found.hits[0].record.content, 'lifecycle'), false);
  assert.deepEqual(found.facets.author, []);
  assert.deepEqual(found.facets.kind, [{ value: 'documentation', count: 1 }]);
});
