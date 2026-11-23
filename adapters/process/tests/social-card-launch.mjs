import assert from 'node:assert/strict';
import test from 'node:test';
import { browserLaunch } from '../social_card/launch.mjs';

// These tests only construct values. They neither create profiles nor start processes.
test('macOS capture isolates browser storage and uses Chromium mock Keychain', () => {
  const profile = '/owned temporary/gazette-social-card-test';
  const { args, env, directories } = browserLaunch(profile, 'darwin');
  assert.ok(args.includes('--use-mock-keychain'));
  assert.ok(args.includes('--disable-features=DialMediaRouteProvider'));
  assert.ok(args.includes(`--user-data-dir=${profile}`));
  assert.ok(args.includes(`--disk-cache-dir=${profile}/cache`));
  assert.ok(args.includes('--disk-cache-size=8388608'));
  assert.ok(args.includes('--media-cache-size=8388608'));
  assert.deepEqual(directories, [`${profile}/cache`, `${profile}/tmp`]);
  assert.deepEqual(env, { TMPDIR: `${profile}/tmp`, TMP: `${profile}/tmp`, TEMP: `${profile}/tmp`, LANG: 'en_US.UTF-8' });
  assert.ok(!Object.hasOwn(env, 'HOME'));
  assert.ok(!args.includes('--password-store=basic'), 'Linux store selection is not the macOS Keychain control');
});

test('Linux capture selects the basic password store and owned XDG paths', () => {
  const profile = '/owned/gazette-social-card-test';
  const { args, env, directories } = browserLaunch(profile, 'linux');
  assert.ok(args.includes('--password-store=basic'));
  assert.ok(!args.includes('--use-mock-keychain'));
  assert.equal(env.XDG_CONFIG_HOME, `${profile}/config`);
  assert.equal(env.XDG_CACHE_HOME, `${profile}/cache`);
  assert.ok(directories.includes(env.XDG_CONFIG_HOME));
  assert.ok(!Object.hasOwn(env, 'HOME'));
});

test('launch configuration keeps sandboxing, explicit arguments and network suppression', () => {
  for (const platform of ['darwin', 'linux']) {
    const first = browserLaunch('/owned/profile', platform);
    assert.ok(first.args.includes('--disable-background-networking'));
    assert.ok(first.args.includes('--disable-sync'));
    assert.ok(first.args.includes('--disable-extensions'));
    assert.ok(first.args.includes('--no-first-run'));
    assert.ok(first.args.includes('--no-default-browser-check'));
    assert.ok(first.args.includes('--remote-debugging-address=127.0.0.1'));
    assert.ok(first.args.includes('--proxy-server=http://127.0.0.1:9'));
    assert.ok(first.args.includes('--host-resolver-rules=MAP * ~NOTFOUND'));
    assert.ok(!first.args.some(arg => /no-sandbox|disable-.*sandbox|disable-web-security|ignore-certificate-errors/.test(arg)));
    assert.ok(!Object.hasOwn(first.env, 'NODE_OPTIONS'));
    first.args.push('--arbitrary'); first.env.NODE_OPTIONS = '--arbitrary';
    const second = browserLaunch('/owned/profile', platform);
    assert.ok(!second.args.includes('--arbitrary'));
    assert.ok(!Object.hasOwn(second.env, 'HOME'));
    assert.ok(!Object.hasOwn(second.env, 'NODE_OPTIONS'));
  }
});

test('unsupported hosts and ambiguous profile paths fail before native effects', () => {
  for (const profile of ['relative', '/', '/owned/../profile', '/owned/profile\n', '/owned//profile']) {
    assert.throws(() => browserLaunch(profile, 'darwin'), /invalid-browser-profile/);
  }
  assert.throws(() => browserLaunch('/owned/profile', 'win32'), /unsupported-browser-host/);
});
