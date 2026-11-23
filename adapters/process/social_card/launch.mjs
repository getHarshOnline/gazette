import { isAbsolute, join, normalize } from 'node:path';
import { requireThat } from './contract.mjs';

// Chromium's test switches avoid native credential prompts without changing the user's stores.
// https://chromium.googlesource.com/chromium/src/+/main/docs/mac_build_instructions.md
// https://chromium.googlesource.com/chromium/src/+/main/chrome/test/base/in_process_browser_test.cc
export function browserLaunch(profile, platform) {
  requireThat(['darwin', 'linux'].includes(platform), 'unsupported-browser-host');
  requireThat(typeof profile === 'string' && isAbsolute(profile) && normalize(profile) === profile
    && profile !== '/' && !/[\u0000-\u001f\u007f-\u009f]/u.test(profile), 'invalid-browser-profile');
  const cache = join(profile, 'cache'), temporary = join(profile, 'tmp');
  const args = [
    '--headless=new', '--no-startup-window', '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
    `--user-data-dir=${profile}`, `--disk-cache-dir=${cache}`, '--disk-cache-size=8388608', '--media-cache-size=8388608',
    '--no-first-run', '--no-default-browser-check', '--disable-background-networking', '--disable-component-update',
    '--disable-sync', '--disable-extensions', '--disable-default-apps', '--disable-breakpad', '--disable-domain-reliability',
    '--metrics-recording-only', '--force-device-scale-factor=1', '--disable-features=DialMediaRouteProvider',
    '--proxy-server=http://127.0.0.1:9', '--proxy-bypass-list=<-loopback>', '--host-resolver-rules=MAP * ~NOTFOUND',
    platform === 'darwin' ? '--use-mock-keychain' : '--password-store=basic',
  ];
  // Deliberately omit HOME and inherited credentials, flags, proxies and browser configuration.
  const env = { TMPDIR: temporary, TMP: temporary, TEMP: temporary, LANG: 'en_US.UTF-8' };
  const directories = [cache, temporary];
  if (platform === 'linux') {
    env.XDG_CACHE_HOME = cache;
    env.XDG_CONFIG_HOME = join(profile, 'config');
    directories.push(env.XDG_CONFIG_HOME);
  }
  return { args, env, directories };
}
