// Local lifecycle/command-shape proof only: no Google service or hosted-readiness claim.
import assert from 'node:assert/strict';
import test from 'node:test';
import { AnalyticsError, createAnalytics, events } from '../analytics/index.mjs';

const configuration = { measurementId: 'G-ABC123', publicPageUrl: 'https://example.test/article/' };
const coordinates = { site: 'site:one', content: 'article:one', journey: 'journey:read', button: 'read-more' };
const code = expected => error => error instanceof AnalyticsError && error.code === expected;

function browser() {
  const scripts = [];
  const timers = new Map();
  let nextTimer = 0;
  const document = {
    head: { appendChild(script) { scripts.push(script); } },
    createElement(name) { assert.equal(name, 'script'); return {}; },
    querySelector() { return null; },
  };
  // Reading ambient page/form/browser data would fail this real module invocation.
  for (const field of ['title', 'referrer', 'location', 'cookie']) {
    Object.defineProperty(document, field, { get() { throw new Error(`private DOM read: ${field}`); } });
  }
  const host = {
    document,
    setTimeout(callback, milliseconds) { timers.set(++nextTimer, { callback, milliseconds }); return nextTimer; },
    clearTimeout(id) { timers.delete(id); },
  };
  for (const field of ['location', 'localStorage', 'sessionStorage', 'navigator']) {
    Object.defineProperty(host, field, { get() { throw new Error(`private browser read: ${field}`); } });
  }
  return { host, scripts, timers };
}

const commands = host => Array.from(host.gazetteAnalyticsData ?? [], args => Array.from(args));

test('import and creation do not touch global browser state, load tags or infer consent', async () => {
  const globals = ['window', 'document', 'dataLayer', 'gtag', 'localStorage', 'sessionStorage'];
  const before = new Map(globals.map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  try {
    for (const key of globals) Object.defineProperty(globalThis, key, {
      configurable: true,
      get() { assert.fail(`import read ${key}`); },
      set() { assert.fail(`import wrote ${key}`); },
    });
    await import('../analytics/index.mjs?import-effects-proof');
  } finally {
    for (const [key, descriptor] of before) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
  const { host, scripts, timers } = browser();
  const controller = createAnalytics(configuration, host);
  assert.deepEqual(controller.status(), { consent: 'withheld', tag: 'not-requested', eventsSubmitted: 0, mayHaveEffects: false });
  assert.deepEqual(controller.event('content_view', coordinates), { submitted: false, reason: 'withheld' });
  assert.equal(scripts.length, 0);
  assert.equal(timers.size, 0);
  assert.equal('gazetteAnalyticsData' in host, false);
  assert.equal('ga-disable-G-ABC123' in host, false);
});

test('explicit grant loads one exact Google tag and emits no initial or queued pageview', async () => {
  const { host, scripts, timers } = browser();
  const controller = createAnalytics(configuration, host);
  const pending = controller.grant();
  assert.equal(controller.grant(), pending);
  assert.equal(scripts.length, 1);
  assert.equal(scripts[0].src, 'https://www.googletagmanager.com/gtag/js?id=G-ABC123&l=gazetteAnalyticsData');
  assert.equal(scripts[0].async, true);
  assert.equal(scripts[0].referrerPolicy, 'no-referrer');
  assert.deepEqual(commands(host), []);
  assert.deepEqual(controller.event('content_view', coordinates), { submitted: false, reason: 'loading' });
  scripts[0].onload();
  assert.equal((await pending).tag, 'loaded');
  assert.equal(timers.size, 0);
  const native = commands(host);
  assert.deepEqual(native[0], ['consent', 'default', {
    analytics_storage: 'granted', ad_storage: 'denied', ad_user_data: 'denied', ad_personalization: 'denied',
  }]);
  assert.equal(native[1][0], 'js');
  assert.ok(native[1][1] instanceof Date);
  assert.deepEqual(native[2], ['config', 'G-ABC123', {
    send_page_view: false, allow_google_signals: false, allow_ad_personalization_signals: false,
    page_location: configuration.publicPageUrl, page_title: '', page_referrer: '', ignore_referrer: true,
  }]);
  assert.equal(native.length, 3, 'events before readiness were dropped, not replayed');
  assert.equal(native.some(item => item[0] === 'event'), false);
});

test('only the seven events submit bounded copied public coordinates to the exact selected ID', async () => {
  const { host, scripts } = browser();
  const controller = createAnalytics(configuration, host);
  const pending = controller.grant();
  scripts[0].onload();
  await pending;
  assert.deepEqual(events, ['content_view', 'cross_site_navigate', 'cta_view', 'cta_click', 'intake_start', 'subscribe_start', 'discussion_open']);
  for (const name of events) {
    const input = { ...coordinates };
    assert.deepEqual(controller.event(name, input), { submitted: true });
    input.content = 'content:changed';
  }
  const native = commands(host).slice(3);
  assert.deepEqual(native.map(item => item[1]), events);
  for (const item of native) assert.deepEqual(item, ['event', item[1], {
    ...coordinates, send_to: configuration.measurementId, page_location: configuration.publicPageUrl,
    page_title: '', page_referrer: '',
  }]);
  assert.equal(controller.status().eventsSubmitted, 7);
  assert.ok(Object.isFrozen(controller) && Object.isFrozen(controller.status()) && Object.isFrozen(events));
});

test('withdrawal suppresses future commands without claiming earlier provider effects were recalled', async () => {
  const { host, scripts } = browser();
  const controller = createAnalytics(configuration, host);
  const pending = controller.grant();
  scripts[0].onload();
  await pending;
  controller.event('intake_start', coordinates);
  const native = commands(host);
  assert.deepEqual(controller.withdraw(), { consent: 'withdrawn', tag: 'loaded', eventsSubmitted: 1, mayHaveEffects: true });
  assert.equal(host['ga-disable-G-ABC123'], true);
  assert.deepEqual(controller.event('subscribe_start', coordinates), { submitted: false, reason: 'withdrawn' });
  assert.deepEqual(commands(host), native, 'withdrawal emits no advanced-consent denied ping or replay');
  await assert.rejects(controller.grant(), code('analytics_new_document_required'));
  assert.throws(() => createAnalytics(configuration, host), code('analytics_ownership'));
  assert.equal(scripts.length, 1);
});

test('withdrawal during a requested tag load keeps late completion disabled with zero config/events', async () => {
  const { host, scripts, timers } = browser();
  const controller = createAnalytics(configuration, host);
  const pending = controller.grant();
  const lateLoad = scripts[0].onload;
  const lateFailure = scripts[0].onerror;
  controller.event('content_view', coordinates);
  controller.withdraw();
  assert.equal((await pending).consent, 'withdrawn');
  lateLoad();
  lateFailure();
  assert.equal(host['ga-disable-G-ABC123'], true);
  assert.equal(timers.size, 0);
  assert.deepEqual(commands(host), []);
  await assert.rejects(controller.grant(), code('analytics_new_document_required'));
});

test('withdrawal before grant never loads and explicitly requires a fresh document', async () => {
  const { host, scripts } = browser();
  const controller = createAnalytics(configuration, host);
  assert.equal(controller.withdraw().mayHaveEffects, false);
  await assert.rejects(controller.grant(), code('analytics_new_document_required'));
  assert.equal(scripts.length, 0);
});

test('tag errors/timeouts and environmental append failures refuse without automatic retries', async () => {
  for (const mode of ['error', 'timeout', 'append']) {
    const { host, scripts, timers } = browser();
    if (mode === 'append') host.document.head.appendChild = () => { throw new Error('blocked'); };
    const controller = createAnalytics(configuration, host);
    const pending = controller.grant();
    if (mode === 'error') scripts[0].onerror();
    if (mode === 'timeout') [...timers.values()][0].callback();
    await assert.rejects(pending, code('analytics_load'));
    assert.equal(controller.status().tag, 'failed');
    assert.equal(host['ga-disable-G-ABC123'], true);
    assert.equal(timers.size, 0);
    assert.deepEqual(controller.event('content_view', coordinates), { submitted: false, reason: 'failed' });
    await assert.rejects(controller.grant(), code('analytics_load'));
    assert.deepEqual(commands(host), []);
  }
});

test('invalid configuration and ambiguous Google ownership refuse before tag/timer effects', () => {
  for (const invalid of [null, {}, { ...configuration, email: 'private@example.test' },
    { ...configuration, measurementId: 'UA-123' }, { ...configuration, measurementId: 'G-ABC<script>' },
    { ...configuration, maxEvents: 0 }, { ...configuration, maxEvents: 1001 },
    { ...configuration, maxEvents: null }, { ...configuration, maxEvents: undefined },
    { ...configuration, loadTimeoutMs: null }, { ...configuration, loadTimeoutMs: 30001 }]) {
    const { host, scripts, timers } = browser();
    assert.throws(() => createAnalytics(invalid, host), AnalyticsError);
    assert.equal(scripts.length, 0);
    assert.equal(timers.size, 0);
  }
  for (const publicPageUrl of ['javascript:alert(1)', '/relative', 'https://user:pass@example.test/',
    'https://example.test/?email=private', 'https://example.test/#private', 'https://example.test/?',
    'https://example.test/a%0ab/', 'https://example.test/a\\b/', 'https://example.test', 'https://example.test/' + 'a'.repeat(1000)]) {
    assert.throws(() => createAnalytics({ ...configuration, publicPageUrl }, browser().host), code('analytics_page_url'));
  }
  for (const key of ['dataLayer', 'gtag', 'google_tag_manager', 'google_tag_data', 'gazetteAnalyticsData', 'ga-disable-G-ABC123']) {
    const { host, scripts } = browser();
    host[key] = [];
    assert.throws(() => createAnalytics(configuration, host), code('analytics_ownership'));
    assert.equal(scripts.length, 0);
  }
  const { host, scripts } = browser();
  const controller = createAnalytics(configuration, host);
  host.gtag = () => {};
  assert.throws(() => controller.grant(), code('analytics_ownership'));
  assert.equal(scripts.length, 0);
  const competing = browser();
  competing.host.document.querySelector = () => ({ src: 'https://www.googletagmanager.com/gtag/js?id=G-OTHER' });
  assert.throws(() => createAnalytics(configuration, competing.host), code('analytics_ownership'));
});

test('shape, accessors, email/form/free-form values and coordinate ceilings cannot enter events', () => {
  const controller = createAnalytics(configuration, browser().host);
  for (const name of ['page_view', 'form_submit', 'purchase', '', undefined]) {
    assert.throws(() => controller.event(name, coordinates), code('analytics_event'));
  }
  for (const input of [null, {}, { ...coordinates, email: 'private@example.test' },
    { ...coordinates, form: {} }, { ...coordinates, content: 'someone@example.test' },
    { ...coordinates, content: 'content:https://example.test/' },
    { ...coordinates, site: 'person:one' }, { ...coordinates, journey: 'content:one' },
    { ...coordinates, button: 'Visible raw label' }, { ...coordinates, button: 'x'.repeat(65) },
    { ...coordinates, content: 'content:' + 'x'.repeat(128) }, { ...coordinates, content: 'content:line\nbreak' },
    { ...coordinates, content: 'content:one\n' }, { ...coordinates, button: 'read-more\n' },
    { ...coordinates, content: null }, { ...coordinates, button: undefined }]) {
    assert.throws(() => controller.event('content_view', input), AnalyticsError);
  }
  const accessor = { ...coordinates };
  Object.defineProperty(accessor, 'site', { get() { assert.fail('getter invoked'); }, enumerable: true });
  assert.throws(() => controller.event('content_view', accessor), code('analytics_shape'));
  const symbol = { ...coordinates, [Symbol('private')]: 'secret' };
  assert.throws(() => controller.event('content_view', symbol), code('analytics_shape'));
  assert.deepEqual(controller.event('content_view', { site: 'site:one', content: 'index:home' }), { submitted: false, reason: 'withheld' });
});

test('module event ceiling and changed layer ownership suppress additional commands', async () => {
  const { host, scripts } = browser();
  const controller = createAnalytics({ ...configuration, maxEvents: 1 }, host);
  const pending = controller.grant();
  scripts[0].onload();
  await pending;
  controller.event('cta_click', coordinates);
  assert.throws(() => controller.event('cta_click', coordinates), code('analytics_limit'));
  assert.equal(commands(host).length, 4);
  const unrelated = [];
  host.gazetteAnalyticsData = unrelated;
  assert.throws(() => controller.event('content_view', coordinates), code('analytics_ownership'));
  assert.equal(host['ga-disable-G-ABC123'], true);
  assert.deepEqual(unrelated, [], 'another layer is never adopted, cleared or overwritten');
  assert.equal(controller.status().consent, 'withdrawn');
});

test('withdrawal inside native command handling blocks subsequent startup commands too', async () => {
  const { host, scripts } = browser();
  const controller = createAnalytics(configuration, host);
  const pending = controller.grant();
  const layer = host.gazetteAnalyticsData;
  layer.push = function (command) {
    Array.prototype.push.call(this, command);
    controller.withdraw();
  };
  scripts[0].onload();
  assert.equal((await pending).consent, 'withdrawn');
  assert.equal(host['ga-disable-G-ABC123'], true);
  assert.equal(commands(host).length, 1, 'no config/js/event is offered after withdrawal');
  assert.equal(commands(host)[0][0], 'consent');
  assert.deepEqual(controller.event('content_view', coordinates), { submitted: false, reason: 'withdrawn' });
});

test('a native dispatch failure is explicit, may have effects, and gates future calls', async () => {
  const { host, scripts } = browser();
  const controller = createAnalytics(configuration, host);
  const pending = controller.grant();
  scripts[0].onload();
  await pending;
  const layer = host.gazetteAnalyticsData;
  layer.push = function (command) {
    Array.prototype.push.call(this, command);
    throw new Error('native receiver failed after accepting command');
  };
  assert.throws(() => controller.event('cta_click', coordinates), code('analytics_transport'));
  assert.equal(commands(host).length, 4, 'the failed command may already have been accepted');
  assert.equal(controller.status().eventsSubmitted, 1);
  assert.equal(controller.status().mayHaveEffects, true);
  assert.equal(controller.status().consent, 'withdrawn');
  assert.equal(host['ga-disable-G-ABC123'], true);
  assert.deepEqual(controller.event('cta_click', coordinates), { submitted: false, reason: 'withdrawn' });
  await assert.rejects(controller.grant(), code('analytics_new_document_required'));
  assert.equal(commands(host).length, 4);
});

test('measurement spelling matches the native 64-character ASCII bound, without property qualification', async () => {
  const { host, scripts } = browser();
  const measurementId = `G-${'A'.repeat(62)}`;
  const controller = createAnalytics({ ...configuration, measurementId }, host);
  const pending = controller.grant();
  assert.equal(new URL(scripts[0].src).searchParams.get('id'), measurementId);
  scripts[0].onload();
  await pending;
  assert.equal(commands(host)[2][1], measurementId);
  for (const invalid of [`G-${'A'.repeat(63)}`, 'G-A\n', 'G-A\r', 'G-A\u2028', 'G-Å', 'G-a', 'G-', ' G-A']) {
    assert.throws(() => createAnalytics({ ...configuration, measurementId: invalid }, browser().host), code('analytics_measurement_id'));
  }
});
