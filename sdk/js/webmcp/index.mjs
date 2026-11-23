// SPDX-License-Identifier: Apache-2.0
// Explicit browser registration only. Importing this module reads no document or provider.
import { WebMcpError, fail, integer, keys, snapshot } from './data.mjs';
export { WebMcpError } from './data.mjs';

const operations = Object.freeze(['current_page', 'search']);
const ceilings = Object.freeze({
  maxInputBytes: 65_536, maxOutputBytes: 16 * 1024 * 1024, maxResults: 1_000,
  maxNodes: 1_000_000, maxDepth: 64, maxConcurrent: 64, timeoutMs: 300_000,
});

/** Register only explicitly selected, read-only operations over admitted public projections. */
export async function registerReadOnlyTools(document, configuration) {
  keys(configuration, ['tools', 'limits', 'currentPage', 'search'], ['tools', 'limits']);
  keys(configuration.limits, Object.keys(ceilings));
  const limits = Object.freeze(Object.fromEntries(Object.entries(ceilings)
    .map(([name, maximum]) => [name, integer(configuration.limits[name], maximum)])));
  const selected = configuration.tools;
  if (!Array.isArray(selected) || selected.length < 1 || selected.length > operations.length
      || Reflect.ownKeys(selected).some(name => name !== 'length'
        && (typeof name !== 'string' || !['0', '1'].includes(name)
          || !Object.hasOwn(Object.getOwnPropertyDescriptor(selected, name), 'value')))) {
    fail('webmcp_options');
  }
  const names = Object.freeze(Array.from({ length: selected.length }, (_, index) =>
    Object.getOwnPropertyDescriptor(selected, String(index))?.value));
  if (names.some(name => !operations.includes(name)) || new Set(names).size !== names.length) {
    fail('webmcp_options');
  }
  const callbacks = { current_page: configuration.currentPage, search: configuration.search };
  if (names.some(name => typeof callbacks[name] !== 'function')) fail('webmcp_options');
  const context = document?.modelContext;
  if (typeof context?.registerTool !== 'function') fail('webmcp_unsupported');
  const registration = new AbortController();
  let active = 0;

  async function execute(name, input, options) {
    if (registration.signal.aborted) fail('webmcp_closed');
    const incoming = options?.signal;
    if (!incoming || typeof incoming.aborted !== 'boolean'
        || typeof incoming.addEventListener !== 'function'
        || typeof incoming.removeEventListener !== 'function') fail('webmcp_input');
    if (incoming.aborted) fail('webmcp_cancelled');
    keys(input, name === 'search' ? ['query', 'limit'] : [], undefined, 'webmcp_input');
    const arguments_ = snapshot(input, limits, limits.maxInputBytes);
    if (name === 'search') {
      if (typeof arguments_.query !== 'string' || arguments_.query.trim() === '') fail('webmcp_input');
      integer(arguments_.limit, limits.maxResults, 'webmcp_input');
    }
    if (active >= limits.maxConcurrent) fail('webmcp_busy');
    const controller = new AbortController();
    const cancel = () => controller.abort(new WebMcpError('webmcp_cancelled'));
    const close = () => controller.abort(new WebMcpError('webmcp_closed'));
    incoming.addEventListener('abort', cancel, { once: true });
    registration.signal.addEventListener('abort', close, { once: true });
    // Account for abortion during listener registration, before invoking a callback.
    if (incoming.aborted) cancel();
    if (registration.signal.aborted) close();
    let rejectAbort;
    const aborted = new Promise((_, reject) => { rejectAbort = reject; });
    const refuse = () => rejectAbort(controller.signal.reason);
    controller.signal.addEventListener('abort', refuse, { once: true });
    const timer = setTimeout(() => controller.abort(new WebMcpError('webmcp_timeout')), limits.timeoutMs);
    if (controller.signal.aborted) refuse();
    active++;
    const task = Promise.resolve().then(() => {
      if (controller.signal.aborted) throw controller.signal.reason;
      return name === 'current_page' ? callbacks[name]({ signal: controller.signal })
        : callbacks[name](arguments_.query, { limit: arguments_.limit, signal: controller.signal });
    }).then(result => {
      if (controller.signal.aborted) throw controller.signal.reason;
      if (name === 'search' && (!Array.isArray(result) || result.length > arguments_.limit)) {
        fail('webmcp_data');
      }
      return snapshot(result, limits, limits.maxOutputBytes);
    }).catch(error => {
      if (error instanceof WebMcpError) throw error;
      fail('webmcp_callback', error);
    }).finally(() => { active--; });
    try { return await Promise.race([task, aborted]); }
    finally {
      clearTimeout(timer);
      incoming.removeEventListener('abort', cancel);
      registration.signal.removeEventListener('abort', close);
      controller.signal.removeEventListener('abort', refuse);
    }
  }

  try {
    for (const name of names) {
      await context.registerTool({
        name,
        description: name === 'current_page'
          ? 'Read the admitted public data for the current page, without navigation or modification.'
          : 'Search the admitted public search corpus, without navigation or modification.',
        inputSchema: name === 'current_page'
          ? { type: 'object', properties: {}, additionalProperties: false }
          : { type: 'object', properties: {
            query: { type: 'string', minLength: 1, maxLength: limits.maxInputBytes },
            limit: { type: 'integer', minimum: 1, maximum: limits.maxResults },
          }, required: ['query', 'limit'], additionalProperties: false },
        annotations: { readOnlyHint: true, untrustedContentHint: true, consequentialHint: false },
        execute: (input, options) => execute(name, input, options),
      }, { signal: registration.signal });
    }
  } catch (cause) {
    registration.abort();
    fail('webmcp_registration', cause);
  }
  return Object.freeze({ names, close() { registration.abort(); } });
}
