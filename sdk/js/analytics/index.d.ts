// SPDX-License-Identifier: Apache-2.0
/** The owner-held semantic vocabulary, not automatic DOM instrumentation. */
export type AnalyticsEvent = 'content_view' | 'cross_site_navigate' | 'cta_view' | 'cta_click'
  | 'intake_start' | 'subscribe_start' | 'discussion_open';
export const events: readonly AnalyticsEvent[];

/** Already-public coordinates; bounded transport validation does not resolve identities. */
export interface AnalyticsCoordinates {
  site: string;
  content: string;
  journey?: string;
  button?: string;
}

export interface AnalyticsConfiguration {
  /** Exact explicit GA4 G- ID, at most 64 ASCII characters; not account/property certification. */
  measurementId: string;
  /** Exact resolved canonical public HTTP(S) URL, without credentials/query/fragment. */
  publicPageUrl: string;
  /** Positive module-submitted event ceiling, at most 1000; default 1000. */
  maxEvents?: number;
  /** Positive tag-load timeout, at most 30000 milliseconds; default 10000. */
  loadTimeoutMs?: number;
}

export interface AnalyticsStatus {
  readonly consent: 'withheld' | 'granted' | 'withdrawn';
  readonly tag: 'not-requested' | 'loading' | 'loaded' | 'failed';
  readonly eventsSubmitted: number;
  /** A tag was requested; no recall or remote-delivery claim. */
  readonly mayHaveEffects: boolean;
}

export type AnalyticsSubmission = { readonly submitted: true }
  | { readonly submitted: false; readonly reason: AnalyticsStatus['consent'] | AnalyticsStatus['tag'] };

export interface AnalyticsController {
  status(): AnalyticsStatus;
  /** Explicit invocation by the Theme after consent; no UI/persistence policy inferred. */
  grant(): Promise<AnalyticsStatus>;
  /** Terminal for this document; a later grant refuses with analytics_new_document_required. */
  withdraw(): AnalyticsStatus;
  event(name: AnalyticsEvent, coordinates: AnalyticsCoordinates): AnalyticsSubmission;
}

export class AnalyticsError extends Error {
  readonly code: string;
  constructor(code: string);
}

/** Import/create has no tag, timer, storage or listener effects; one owner per document. */
export function createAnalytics(configuration: AnalyticsConfiguration, browser?: Window): AnalyticsController;
