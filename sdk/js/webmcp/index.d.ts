// SPDX-License-Identifier: Apache-2.0
import type { Json, SearchHit } from '../search/index.js';

/** Closed read-only browser operations, unrelated to Gazette's stdio tool catalogue. */
export type ReadOnlyTool = 'current_page' | 'search';
export interface WebMcpLimits {
  readonly maxInputBytes: number;
  readonly maxOutputBytes: number;
  readonly maxResults: number;
  readonly maxNodes: number;
  readonly maxDepth: number;
  readonly maxConcurrent: number;
  readonly timeoutMs: number;
}
/** Only the native registration subset this module consumes; no browser API polyfill. */
export interface WebMcpDocument {
  readonly modelContext?: {
    registerTool(tool: {
      name: ReadOnlyTool;
      description: string;
      inputSchema: object;
      annotations: { readOnlyHint: true; untrustedContentHint: true; consequentialHint: false };
      execute(input: object, options: { signal: AbortSignal }): Promise<Json>;
    }, options: { signal: AbortSignal }): Promise<void>;
  };
}
export interface ReadOnlyTools {
  readonly tools: readonly ReadOnlyTool[];
  readonly limits: WebMcpLimits;
  /** Already admitted public projection, not private inspection data or scraped DOM. */
  readonly currentPage?: (options: { signal: AbortSignal }) => Json | Promise<Json>;
  /** Reuse existing search; return the complete requested hits without another resolver. */
  readonly search?: (query: string, options: { limit: number; signal: AbortSignal }) => readonly (Json | SearchHit)[] | Promise<readonly (Json | SearchHit)[]>;
}
export interface WebMcpRegistration {
  readonly names: readonly ReadOnlyTool[];
  /** Unregister and signal active callbacks. This does not prove arbitrary callbacks stopped. */
  close(): void;
}
export class WebMcpError extends Error {
  readonly code: string;
  /** Retains the supplied cause value; it is not an ErrorOptions object to unwrap. */
  constructor(code: string, cause?: unknown);
}
/** Explicit registration; API absence refuses with webmcp_unsupported, without legacy fallback. */
export function registerReadOnlyTools(document: Document | WebMcpDocument | undefined, configuration: ReadOnlyTools): Promise<WebMcpRegistration>;
