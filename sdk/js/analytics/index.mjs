// SPDX-License-Identifier: Apache-2.0
// Basic-consent Google Analytics transport. Importing this module has no browser effects.
// Theme owns consent UI/persistence and when semantic events occur; this is not legal approval.

export const events = Object.freeze([
  'content_view', 'cross_site_navigate', 'cta_view', 'cta_click',
  'intake_start', 'subscribe_start', 'discussion_open',
]);

const owners = new WeakSet();
const layerName = 'gazetteAnalyticsData';
const own = (value, key) => Object.prototype.hasOwnProperty.call(value, key);

export class AnalyticsError extends Error {
  constructor(code) {
    super(code);
    this.name = 'AnalyticsError';
    this.code = code;
  }
}

function fail(code) { throw new AnalyticsError(code); }

function record(value, allowed, required) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)
      || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) fail('analytics_shape');
  const fields = Object.getOwnPropertyDescriptors(value);
  if (Reflect.ownKeys(fields).some(key => typeof key !== 'string' || !allowed.includes(key)
      || !own(fields[key], 'value') || !fields[key].enumerable)
      || required.some(key => !own(fields, key))) fail('analytics_shape');
}

function integer(value, maximum) {
  if (!Number.isSafeInteger(value) || value < 1 || value > maximum) fail('analytics_limit');
  return value;
}

function publicPage(value) {
  if (typeof value !== 'string' || value.length > 1000 || !/^https?:\/\//.test(value)
      || /[\s\\?#\u0000-\u001f\u007f]/u.test(value)
      || /%(?:0[0-9a-f]|1[0-9a-f]|7f|5c)/i.test(value)) fail('analytics_page_url');
  let parsed;
  try { parsed = new URL(value); } catch { fail('analytics_page_url'); }
  if (!parsed.hostname || parsed.username || parsed.password || parsed.href !== value) {
    fail('analytics_page_url');
  }
  return value; // Preserve the caller's exact resolved public route, never reconstruct a URL.
}

function coordinates(value) {
  record(value, ['site', 'content', 'journey', 'button'], ['site', 'content']);
  const result = {};
  for (const field of ['site', 'content', 'journey', 'button']) {
    if (!own(value, field)) continue;
    const selected = value[field];
    // This transport profile is a bounded ASCII subset, not another identity resolver.
    // The caller supplies already-public identities; no URL, email, label or form value is accepted.
    const pattern = field === 'button' ? /^[a-z][a-z0-9_-]{0,63}$/
      : /^[A-Za-z][A-Za-z0-9_-]*:[A-Za-z0-9][A-Za-z0-9._/-]*$/;
    if (typeof selected !== 'string' || selected.length > 128 || /[^\x21-\x7e]/.test(selected) || !pattern.test(selected)
        || (field === 'site' && !selected.startsWith('site:'))
        || (field === 'journey' && !selected.startsWith('journey:'))) fail('analytics_coordinate');
    result[field] = selected;
  }
  return Object.freeze(result);
}

/**
 * Explicitly create one controller per document. No tag, timers, storage or listeners are
 * created until grant(). publicPageUrl must come from the resolved public route, not the DOM.
 * A competing Google tag/layer is refused, never adopted or overwritten.
 */
export function createAnalytics(configuration, browser = globalThis) {
  record(configuration, ['measurementId', 'publicPageUrl', 'maxEvents', 'loadTimeoutMs'],
    ['measurementId', 'publicPageUrl']);
  const { measurementId } = configuration;
  if (typeof measurementId !== 'string' || /[^A-Z0-9-]/.test(measurementId)
      || !/^G-[A-Z0-9]{1,62}$/.test(measurementId)) {
    fail('analytics_measurement_id');
  }
  const page = publicPage(configuration.publicPageUrl);
  const maxEvents = integer(own(configuration, 'maxEvents') ? configuration.maxEvents : 1000, 1000);
  const timeout = integer(own(configuration, 'loadTimeoutMs') ? configuration.loadTimeoutMs : 10_000, 30_000);
  const document = browser?.document;
  if (!document?.head || typeof document.head.appendChild !== 'function'
      || typeof document.createElement !== 'function' || typeof document.querySelector !== 'function'
      || typeof browser.setTimeout !== 'function' || typeof browser.clearTimeout !== 'function') {
    fail('analytics_environment');
  }
  const disable = `ga-disable-${measurementId}`;
  function vacant() {
    if (['dataLayer', 'gtag', 'google_tag_manager', 'google_tag_data', layerName, disable]
      .some(key => key in browser)
      || document.querySelector('script[src*="googletagmanager.com"], script[src*="google-analytics.com"]')) {
      fail('analytics_ownership');
    }
  }
  if (owners.has(browser)) fail('analytics_ownership');
  vacant();
  owners.add(browser);

  let consent = 'withheld';
  let tag = 'not-requested';
  let submitted = 0;
  let layer;
  let pending;
  let finish;
  let timer;
  let script;
  const status = () => Object.freeze({
    consent, tag, eventsSubmitted: submitted, mayHaveEffects: tag !== 'not-requested',
  });
  function command() {
    if (consent !== 'granted' || browser[layerName] !== layer || browser[disable] !== false) {
      fail('analytics_ownership');
    }
    layer.push(arguments);
  }
  function clearLoad() {
    if (timer !== undefined) browser.clearTimeout(timer);
    if (script) { script.onload = null; script.onerror = null; }
  }
  function stop() {
    consent = 'withdrawn';
    // Google's documented opt-out switch suppresses future cookie/send attempts. It
    // cannot recall a requested tag, submitted event or already-dispatched network effect.
    browser[disable] = true;
    clearLoad();
    if (finish) { finish.resolve(status()); finish = undefined; }
    return status();
  }

  return Object.freeze({
    status,
    /** Explicit grant only; loaded means script completion, not provider readiness/delivery. */
    grant() {
      if (consent === 'withdrawn') return Promise.reject(new AnalyticsError('analytics_new_document_required'));
      if (tag === 'failed') return Promise.reject(new AnalyticsError('analytics_load'));
      if (pending) return pending;
      vacant(); // A tag introduced after creation is still not ours to adopt.
      consent = 'granted';
      layer = [];
      browser[layerName] = layer;
      browser[disable] = false;
      pending = new Promise((resolve, reject) => { finish = { resolve, reject }; });
      const failed = () => {
        if (!finish) return; // A withdrawn/completed load never revives an earlier epoch.
        tag = 'failed';
        browser[disable] = true;
        clearLoad();
        if (finish) { finish.reject(new AnalyticsError('analytics_load')); finish = undefined; }
      };
      try {
        script = document.createElement('script');
        script.async = true;
        script.referrerPolicy = 'no-referrer';
        script.src = `https://www.googletagmanager.com/gtag/js?id=${measurementId}&l=${layerName}`;
        script.onload = () => {
          if (consent !== 'granted' || !finish) return;
          if (browser[layerName] !== layer || browser[disable] !== false) { failed(); return; }
          try {
            // No commands are queued before the tag completes. Denied users never load
            // it; this does not implement advanced-consent denied measurement pings.
            command('consent', 'default', {
              analytics_storage: 'granted', ad_storage: 'denied',
              ad_user_data: 'denied', ad_personalization: 'denied',
            });
            command('js', new Date());
            command('config', measurementId, {
              send_page_view: false, allow_google_signals: false,
              allow_ad_personalization_signals: false,
              page_location: page, page_title: '', page_referrer: '', ignore_referrer: true,
            });
            tag = 'loaded';
            clearLoad();
            finish.resolve(status());
            finish = undefined;
          } catch { failed(); }
        };
        script.onerror = failed;
        timer = browser.setTimeout(failed, timeout);
        tag = 'loading';
        document.head.appendChild(script);
      } catch { failed(); }
      return pending;
    },
    /** Terminal in this document: new consent needs a fresh document, never queued replay. */
    withdraw: stop,
    /** Submit only the seven semantic events, never arbitrary parameters or form data. */
    event(name, value) {
      if (!events.includes(name)) fail('analytics_event');
      const selected = coordinates(value);
      if (consent !== 'granted') return Object.freeze({ submitted: false, reason: consent });
      if (tag !== 'loaded') return Object.freeze({ submitted: false, reason: tag });
      if (browser[layerName] !== layer || browser[disable] !== false) {
        stop();
        fail('analytics_ownership');
      }
      if (submitted >= maxEvents) fail('analytics_limit');
      // Count an offered native command, not confirmed remote delivery. Event-specific
      // privacy fields override defaults even if the browser address changes meanwhile.
      submitted++;
      try {
        command('event', name, {
          ...selected, send_to: measurementId, page_location: page,
          page_title: '', page_referrer: '',
        });
      } catch {
        stop();
        fail('analytics_transport');
      }
      return Object.freeze({ submitted: true });
    },
  });
}
