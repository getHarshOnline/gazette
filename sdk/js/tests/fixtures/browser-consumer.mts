// Strict installed browser bindings; no native root import or ambient Node types.
import {
  AnalyticsError, createAnalytics, events, type AnalyticsConfiguration,
  type AnalyticsController, type AnalyticsEvent,
} from '@rtepl/gazette/analytics';
import {
  WebMcpError, registerReadOnlyTools, type ReadOnlyTools, type WebMcpLimits,
  type WebMcpRegistration,
} from '@rtepl/gazette/webmcp';
import { createIndex } from '@rtepl/gazette/search';

const analytics = {
  measurementId: 'G-EXAMPLE123', publicPageUrl: 'https://example.test/guide/',
  maxEvents: 10, loadTimeoutMs: 1000,
} satisfies AnalyticsConfiguration;
const limits = {
  maxInputBytes: 1024, maxOutputBytes: 32768, maxResults: 10,
  maxNodes: 1000, maxDepth: 10, maxConcurrent: 1, timeoutMs: 1000,
} satisfies WebMcpLimits;
const index = createIndex([{
  content: { id: 'content:guide', title: 'Guide' },
  route: { url: 'https://example.test/guide/', canonical_url: 'https://example.test/guide/' },
  body: 'admitted content', projects: [],
}], { profile: 'lexical', maxDocuments: 10, maxBytes: 32768, maxQueryBytes: 1024, maxResults: 10 });
const tools = {
  tools: ['current_page', 'search'], limits,
  currentPage: ({ signal }) => {
    if (signal.aborted) throw new Error('Cancelled');
    return { content: 'content:guide', url: 'https://example.test/guide/' };
  },
  search: (query, { limit, signal }) => {
    if (signal.aborted) throw new Error('Cancelled');
    return index.search(query, { limit });
  },
} satisfies ReadOnlyTools;

// These functions type-check against native DOM declarations, without shims.
// They are not invoked by the provider-free Node installed-package proof.
export function selectAnalytics(window: Window): AnalyticsController {
  return createAnalytics(analytics, window);
}
export function selectWebMcp(document: Document): Promise<WebMcpRegistration> {
  return registerReadOnlyTools(document, tools);
}

export async function consumeBrowserBindings() {
  let analyticsCode;
  try { createAnalytics(analytics); }
  catch (error) {
    if (!(error instanceof AnalyticsError)) throw error;
    analyticsCode = error.code;
  }
  let webmcpCode;
  try { await registerReadOnlyTools(undefined, tools); }
  catch (error) {
    if (!(error instanceof WebMcpError)) throw error;
    webmcpCode = error.code;
  }
  return { events, analyticsCode, webmcpCode, searchSize: index.size };
}

// Native strict counterexamples, never executed.
export function rejectedBrowserTypes(controller: AnalyticsController, registration: WebMcpRegistration) {
  // @ts-expect-error There is no inferred analytics measurement selection.
  createAnalytics({ publicPageUrl: analytics.publicPageUrl });
  // @ts-expect-error Measurement identity does not infer the public page address.
  createAnalytics({ measurementId: analytics.measurementId });
  // @ts-expect-error Only the owner-held semantic event vocabulary is admitted.
  controller.event('automatic_click', { site: 'site:one', content: 'content:guide' });
  // @ts-expect-error Public event coordinates require a content identity.
  controller.event('content_view', { site: 'site:one' });
  // @ts-expect-error Analytics status is immutable.
  controller.status().consent = 'granted';
  // @ts-expect-error The exported vocabulary is immutable.
  events.push('content_view');
  // @ts-expect-error No effectful tool is part of the browser read-only contract.
  registerReadOnlyTools(undefined, { ...tools, tools: ['navigate'] });
  // @ts-expect-error All execution bounds are explicit.
  registerReadOnlyTools(undefined, { ...tools, limits: { maxOutputBytes: 32768 } });
  // @ts-expect-error Search must preserve the complete result array shape.
  registerReadOnlyTools(undefined, { ...tools, search: () => 'result' });
  // @ts-expect-error Registered tool names cannot be rewritten by a consumer.
  registration.names.push('search');
  const selected: AnalyticsEvent = 'content_view';
  return selected;
}
