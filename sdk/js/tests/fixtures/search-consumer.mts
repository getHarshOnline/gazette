// Strict public browser subpath consumption; no native root or Node types.
import {
  createIndex, loadIndex, type IndexOptions, type LoadOptions, type SearchField,
  type SearchIndex, type SearchRecord,
} from '@rtepl/gazette/search';

const bounds = {
  profile: 'lexical', maxDocuments: 20, maxBytes: 32768,
  maxQueryBytes: 1024, maxResults: 10,
} satisfies IndexOptions;
const records = [{
  content: { id: 'content:compiler', title: 'Publication compiler', lifecycle: 'published', keywords: ['contracts'] },
  route: { url: 'https://example.test/mount/compiler/', canonical_url: 'https://original.test/compiler/' },
  body: 'Shared immutable model', projects: [],
}] satisfies readonly SearchRecord[];

export function consumeSearch() {
  const index = createIndex(records, bounds);
  if (index.search('contracts', { limit: 1, fields: { keywords: 2 } }).length !== 1) {
    throw new Error('Authored keywords were not indexed');
  }
  const hit = index.search('publication', { limit: 1, fields: { title: 2 }, bool: 'AND' })[0];
  if (!hit) throw new Error('Production browser index did not return its actual document');
  if (hit.record.route.canonical_url !== 'https://original.test/compiler/') {
    throw new Error('Canonical metadata changed the serving publication record');
  }
  return {
    title: hit.record.content.title,
    destination: hit.record.route.url,
    size: index.size,
    immutable: Object.isFrozen(hit.record.content),
  };
}

export function consumeExplicitSearchFields() {
  const selected = ['description', 'route', 'notes'] satisfies readonly SearchField[];
  const record = {
    ...records[0]!,
    indexed: { description: 'declarative discovery', route: records[0]!.route.url, notes: 'subordinate commentary' },
  } satisfies SearchRecord;
  const index = createIndex([record], bounds);
  if (index.search('commentary', { limit: 1, fields: { notes: 1 } }).length !== 1
      || index.search('compiler', { limit: 1, fields: { title: 1 } }).length !== 0) {
    throw new Error('Configured fields did not control actual ranking');
  }
  return selected;
}

export function acquireSearch(url: string): Promise<SearchIndex> {
  const selection = {
    ...bounds, allowNetwork: true, maxResponseBytes: 32768, timeoutMs: 1000,
  } satisfies LoadOptions;
  return loadIndex(url, selection);
}

export function consumeBrowse() {
  const index = createIndex(records, bounds);
  const page = index.browse('', { limit: 5, offset: 0, sort: 'latest', filters: { kind: ['article'],
    classification: [{ taxonomy: 'taxonomy:subject', term: 'term:compiler' }] } });
  const count: number = page.total;
  const term: string | undefined = page.facets.classification[0]?.term;
  const keywords = index.browse('', { limit: 5, filters: { keyword: ['contracts'] } });
  const keyword: string | undefined = keywords.facets.keyword[0]?.value;
  return { count, term, keyword };
}

// Compile-time counterexamples, never executed or used as runtime fixtures.
export function rejectedSearchTypes() {
  // @ts-expect-error Canonical metadata cannot replace the required serving address.
  createIndex([{ ...records[0]!, route: { canonical_url: 'https://original.test/compiler/' } }], bounds);
  // @ts-expect-error Profile selection cannot be inferred.
  createIndex(records, { maxDocuments: 20, maxBytes: 32768, maxQueryBytes: 1024, maxResults: 10 });
  // @ts-expect-error Bounds are mandatory independently of profile selection.
  createIndex(records, { profile: 'lexical' });
  // @ts-expect-error There is no implicit or unqualified language profile.
  createIndex(records, { ...bounds, profile: 'automatic' });
  // @ts-expect-error Unknown index settings cannot change the runtime contract.
  createIndex(records, { ...bounds, provider: 'other' });
  // @ts-expect-error An acquisition needs explicit true permission.
  loadIndex('https://example.test/search.json', { ...bounds, allowNetwork: false, maxResponseBytes: 32768, timeoutMs: 1000 });
  // @ts-expect-error Resource bounds do not confer network permission.
  loadIndex('https://example.test/search.json', { ...bounds, maxResponseBytes: 32768, timeoutMs: 1000 });
  // @ts-expect-error Network permission does not replace a response ceiling.
  loadIndex('https://example.test/search.json', { ...bounds, allowNetwork: true, timeoutMs: 1000 });
  const index = createIndex(records, bounds);
  // @ts-expect-error Browse sort names are a closed, declarative vocabulary.
  index.browse('', { limit: 1, sort: 'random' });
  // @ts-expect-error A classification is scoped by its explicit taxonomy.
  index.browse('', { limit: 1, filters: { classification: ['term:compiler'] } });
  // @ts-expect-error Keywords are authored phrases, not classification objects.
  index.browse('', { limit: 1, filters: { keyword: [{ taxonomy: 'topic', term: 'compiler' }] } });
  // @ts-expect-error Each query has an explicit bounded result count.
  index.search('compiler', {});
  // @ts-expect-error No undeclared query evaluator exists.
  index.search('compiler', { limit: 1, evaluate: true });
  // @ts-expect-error Only the actual ranking fields are selectable.
  index.search('compiler', { limit: 1, fields: { invented: 1 } });
  // @ts-expect-error Index summary is readonly.
  index.size = 100;
  const hits = index.search('compiler', { limit: 1 });
  // @ts-expect-error Search returns an immutable result array.
  hits.push(hits[0]!);
  const hit = hits[0]!;
  // @ts-expect-error Result score is readonly.
  hit.score = 100;
  // @ts-expect-error Captured result metadata cannot be rewritten by a consumer.
  hit.record.content.title = 'changed';
}
