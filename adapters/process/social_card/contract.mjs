// The wire format carries one retained card, never a browser command or script.
export const INPUT_BYTES = 64 << 20;
const controls = /[\u0000-\u001f\u007f-\u009f]/u;
export function requireThat(value, reason = 'invalid-request') {
  if (!value) throw new Error(reason);
}
export function keys(value, required, optional = []) {
  requireThat(value && typeof value === 'object' && !Array.isArray(value));
  requireThat(required.every(key => Object.hasOwn(value, key)));
  requireThat(Object.keys(value).every(key => required.includes(key) || optional.includes(key)));
}
function integer(value, low, high) { requireThat(Number.isSafeInteger(value) && value >= low && value <= high); }
export function text(value, maximum, nonempty = true) {
  requireThat(typeof value === 'string' && (!nonempty || value.length > 0) && Buffer.byteLength(value) <= maximum);
}
export function digest(value) { requireThat(typeof value === 'string' && /^[a-f0-9]{64}$/.test(value)); }
export function url(value, allowFragment = false) {
  text(value, 8192);
  requireThat(!controls.test(value) && value.trim() === value);
  const parsed = new URL(value);
  requireThat(['http:', 'https:'].includes(parsed.protocol) && !parsed.username && !parsed.password && (allowFragment || !parsed.hash));
  requireThat(parsed.href === value);
  return parsed;
}
export function validate(input) {
  keys(input, ['version', 'request_sha256', 'html', 'initial_title', 'document_url', 'width', 'height', 'media_selector', 'media_kind', 'browser', 'limits', 'assets'], ['video_time_ms', 'image_variants', 'jpeg_quality']);
  requireThat(input.version === 1);
  digest(input.request_sha256);
  text(input.html, 8 << 20);
  text(input.initial_title, 1024);
  integer(input.width, 1, 4096); integer(input.height, 1, 4096);
  if (Object.hasOwn(input, 'jpeg_quality')) integer(input.jpeg_quality, 1, 100);
  text(input.media_selector, 1024);
  requireThat(input.media_selector.trim() === input.media_selector && !controls.test(input.media_selector));
  requireThat(['image', 'video'].includes(input.media_kind));
  if (Object.hasOwn(input, 'video_time_ms')) {
    requireThat(input.media_kind === 'video'); integer(input.video_time_ms, 0, 600000);
  }
  const origin = url(input.document_url).origin;
  if (Object.hasOwn(input, 'image_variants')) {
    requireThat(Array.isArray(input.image_variants) && input.image_variants.length <= 10000);
    const addresses = new Set();
    for (const image of input.image_variants) {
      keys(image, ['address', 'destination', 'variants']);
      text(image.address, 8192);
      requireThat(!controls.test(image.address) && !addresses.has(image.address));
      addresses.add(image.address);
      url(image.destination, true);
      keys(image.variants, [], ['light', 'dark']);
      requireThat(Object.keys(image.variants).length > 0);
      for (const value of Object.values(image.variants)) url(value, true);
    }
  }
  keys(input.browser, ['program', 'bytes', 'sha256']);
  text(input.browser.program, 8192);
  requireThat(input.browser.program.startsWith('/') && !controls.test(input.browser.program));
  integer(input.browser.bytes, 1, 256 << 20); digest(input.browser.sha256);
  keys(input.limits, ['timeout_ms', 'max_output_bytes']);
  integer(input.limits.timeout_ms, 1, 300000); integer(input.limits.max_output_bytes, 1, 64 << 20);
  requireThat(Array.isArray(input.assets) && input.assets.length <= 10000);
  const urls = new Set(), entries = new Set();
  let bytes = 0;
  for (const asset of input.assets) {
    keys(asset, ['url', 'entry', 'media_type', 'bytes', 'sha256', 'role']);
    requireThat(url(asset.url).origin === origin && asset.url !== input.document_url && !urls.has(asset.url));
    urls.add(asset.url);
    text(asset.entry, 8192);
    requireThat(asset.entry.startsWith('payload/') && !controls.test(asset.entry) && !asset.entry.includes('\\'));
    requireThat(asset.entry.split('/').every(part => part && part !== '.' && part !== '..'));
    requireThat(!entries.has(asset.entry)); entries.add(asset.entry);
    text(asset.media_type, 256);
    requireThat(/^[a-zA-Z0-9!#$&^_.+-]+\/[a-zA-Z0-9!#$&^_.+-]+$/.test(asset.media_type));
    requireThat(['module', 'stylesheet', 'font', 'media'].includes(asset.role));
    integer(asset.bytes, 0, 256 << 20); digest(asset.sha256);
    bytes += asset.bytes; requireThat(bytes <= 1073741824);
  }
  return input;
}
