import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { INPUT_BYTES, requireThat, validate } from './social_card/contract.mjs';
import { verifiedFile, retainedAsset } from './social_card/assets.mjs';
import { CDP } from './social_card/cdp.mjs';
import { prepare, sourceTreatment, imageURL, installCompletion, insertMedia } from './social_card/document.mjs';
import { browserLaunch } from './social_card/launch.mjs';

const root = dirname(fileURLToPath(import.meta.url));
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

export async function capture(value, assetRoot = root) {
  const input = validate(value);
  let child, exited, cdp, profile, timer;
  let failed, captureError;
  const work = async () => {
    const launch = browserLaunch(profile, process.platform);
    for (const directory of launch.directories) await mkdir(directory, { mode: 0o700 });
    if (failed) throw failed;
    child = spawn(input.browser.program, launch.args, { env: launch.env, stdio: ['ignore', 'ignore', 'pipe'] });
    exited = new Promise(resolve => { child.once('exit', resolve); child.once('error', resolve); });
    const address = await new Promise((resolve, reject) => {
      let diagnostics = '';
      child.once('error', () => reject(new Error('browser-start-failed')));
      child.once('exit', () => reject(new Error('browser-exited')));
      child.stderr.on('data', chunk => {
        diagnostics += chunk.toString('utf8');
        if (Buffer.byteLength(diagnostics) > 65536) { failed = new Error('browser-diagnostic-limit'); reject(failed); return; }
        const match = diagnostics.match(/DevTools listening on (ws:\/\/[^\s]+)/);
        if (match) resolve(match[1]);
      });
    });
    cdp = await CDP.connect(address);
    if (failed) { cdp.close(); throw failed; }
    const { browserContextId } = await cdp.send('Target.createBrowserContext');
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank', browserContextId });
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
    const send = (method, params = {}) => cdp.send(method, params, sessionId);
    const evaluate = async expression => {
      const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error('card-evaluation-failed', { cause: result.exceptionDetails });
      if (failed) throw failed;
      return result.result.value;
    };
    let prepared, loaded = false, requests = 0, served = 0;
    const assets = new Map(input.assets.map(asset => [asset.url, asset]));
    const tasks = new Set();
    const request = async event => {
      const { requestId, request: observed, resourceType } = event.params;
      try {
        requireThat(++requests <= 20000 && observed.method === 'GET', 'blocked-request');
        // Chrome probes this browser-chrome icon without a link in the card. It is never served.
        if (resourceType === 'Other' && observed.url === new URL('/favicon.ico', input.document_url).href) {
          await send('Fetch.failRequest', { requestId, errorReason: 'BlockedByClient' }); return;
        }
        let body, headers, responseCode = 200;
        if (prepared && resourceType === 'Document' && observed.url === input.document_url && !loaded) {
          loaded = true; body = Buffer.from(prepared.html);
          const importHashes = prepared.importMaps.map(map => `'sha256-${createHash('sha256').update(map).digest('base64')}'`).join(' ');
          headers = [
            { name: 'Content-Type', value: 'text/html; charset=utf-8' },
            { name: 'Content-Security-Policy', value: `default-src 'none'; script-src 'self' ${importHashes}; style-src 'self' 'unsafe-inline'; img-src 'self'; media-src 'self'; font-src 'self'; connect-src 'none'; worker-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'` },
          ];
        } else {
          const asset = assets.get(observed.url);
          requireThat(prepared && asset, 'blocked-request');
          const role = { Script: 'module', Stylesheet: 'stylesheet', Font: 'font', Image: 'media', Media: 'media' }[resourceType];
          requireThat(role && role === asset.role && (role !== 'media' || observed.url === prepared.mediaURL), 'blocked-request');
          body = await retainedAsset(assetRoot, asset);
          headers = [{ name: 'Content-Type', value: asset.media_type }, { name: 'Access-Control-Allow-Origin', value: new URL(input.document_url).origin }];
          if (role === 'media') {
            headers.push({ name: 'Accept-Ranges', value: 'bytes' });
            const range = Object.entries(observed.headers).find(([name]) => name.toLowerCase() === 'range')?.[1];
            if (range) {
              const match = /^bytes=(\d+)-(\d*)$/.exec(range);
              requireThat(match, 'invalid-media-range');
              const start = Number(match[1]), end = match[2] ? Number(match[2]) : body.length - 1;
              requireThat(Number.isSafeInteger(start) && Number.isSafeInteger(end) && start >= 0 && start <= end && end < body.length, 'invalid-media-range');
              headers.push({ name: 'Content-Range', value: `bytes ${start}-${end}/${body.length}` });
              body = body.subarray(start, end + 1); responseCode = 206;
            }
          }
        }
        served += body.length; requireThat(served <= 1073741824, 'resource-byte-limit');
        await send('Fetch.fulfillRequest', { requestId, responseCode, responseHeaders: [...headers, { name: 'Content-Length', value: String(body.length) }, { name: 'Cache-Control', value: 'no-store' }, { name: 'X-Content-Type-Options', value: 'nosniff' }], body: body.toString('base64') });
      } catch (error) {
        error.cause = { url: observed.url, resourceType };
        failed ??= error;
        await send('Fetch.failRequest', { requestId, errorReason: 'BlockedByClient' }).catch(() => {});
      }
    };
    cdp.listeners.add(event => {
      if (event.sessionId !== sessionId) return;
      if (event.method === 'Fetch.requestPaused') {
        const task = request(event); tasks.add(task); void task.finally(() => tasks.delete(task));
      }
      if (event.method === 'Page.windowOpen' || event.method === 'Page.javascriptDialogOpening') failed ??= new Error('blocked-browser-effect');
    });
    await send('Page.enable'); await send('Runtime.enable');
    await send('Network.enable'); await send('Network.setCacheDisabled', { cacheDisabled: true });
    await send('Network.setBypassServiceWorker', { bypass: true });
    await cdp.send('Browser.setDownloadBehavior', { behavior: 'deny', browserContextId });
    await send('Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Request' }] });
    await send('Emulation.setDeviceMetricsOverride', { width: input.width, height: input.height, deviceScaleFactor: 1, mobile: false });
    await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
    prepared = await evaluate(`(${prepare.toString()})(${JSON.stringify(input)}, ${sourceTreatment.toString()})`);
    await send('Page.addScriptToEvaluateOnNewDocument', { source: `(${installCompletion.toString()})()` });
    await send('Page.navigate', { url: input.document_url });
    for (;;) {
      if (failed) throw failed;
      if (await evaluate('document.readyState === "complete" && Boolean(globalThis.__gazetteCard)')) break;
      await delay(10);
    }
    await evaluate('Promise.all(Array.from(document.querySelectorAll(\'script[type="module"][src]\'), script => import(script.src))).then(() => true)');
    if (input.media_kind === 'image') {
      prepared.mediaURL = await evaluate(`(${imageURL.toString()})(${JSON.stringify(prepared)}, getComputedStyle(document.documentElement).colorScheme, matchMedia('(prefers-color-scheme: dark)').matches)`);
    }
    await evaluate(`(${insertMedia.toString()})(${JSON.stringify(input)}, ${JSON.stringify(prepared)})`);
    for (;;) {
      const state = await evaluate('globalThis.__gazetteCard');
      requireThat(state.errors.length === 0, 'card-resource-error');
      if (state.result) { requireThat(state.result.status === 'complete', state.result.reason); break; }
      await delay(10);
    }
    await Promise.all(tasks);
    const finalTitle = await evaluate('document.querySelector("[data-social-card-title]").textContent');
    requireThat(finalTitle === prepared.title, 'changed-title');
    const { data } = await send('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false, clip: { x: 0, y: 0, width: input.width, height: input.height, scale: 1 } });
    if (failed) throw failed;
    const result = JSON.stringify({ version: 1, request_sha256: input.request_sha256, title: finalTitle, png_base64: data });
    requireThat(Buffer.byteLength(result) <= input.limits.max_output_bytes, 'output-limit');
    return result;
  };
  await verifiedFile(input.browser.program, input.browser);
  profile = await mkdtemp(join(tmpdir(), 'gazette-social-card-'));
  try {
    return await Promise.race([work(), new Promise((_, reject) => {
      timer = setTimeout(() => { failed = new Error('capture-timeout'); reject(failed); }, input.limits.timeout_ms);
    })]);
  } catch (error) {
    captureError = error;
    throw error;
  } finally {
    clearTimeout(timer);
    await cleanupCapture({ cdp, child, exited,
      verify: () => verifiedFile(input.browser.program, input.browser),
      remove: () => profile && rm(profile, { recursive: true, force: true }),
    }, captureError);
  }
}

// A disconnected control socket must not prevent owned-child and profile cleanup.
export async function cleanupCapture({ cdp, child, exited, verify, remove }, originalError) {
  let failure;
  const attempt = async action => { try { await action(); } catch (error) { failure ??= error; } };
  try {
    if (cdp) await attempt(async () => {
      try { await Promise.race([Promise.resolve().then(() => cdp.send('Browser.close')).catch(() => {}), delay(300)]); }
      finally { cdp.close(); }
    });
  } finally {
    try {
      if (child) await attempt(async () => {
        if (child.exitCode === null && child.signalCode === null) {
          await Promise.race([exited, delay(500)]);
          if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
        }
        await Promise.race([exited, delay(1000).then(() => { throw new Error('browser-cleanup-failed'); })]);
      });
    } finally {
      try { await attempt(verify); }
      finally { await attempt(remove); }
    }
  }
  if (failure && !originalError) throw failure;
}

const invokedPath = process.argv[1] && await realpath(process.argv[1]).catch(() => undefined);
if (invokedPath === fileURLToPath(import.meta.url)) {
  try {
    requireThat(process.argv.length === 2, 'invalid-arguments');
    const chunks = []; let length = 0;
    for await (const chunk of process.stdin) {
      length += chunk.length; requireThat(length <= INPUT_BYTES, 'input-limit'); chunks.push(chunk);
    }
    const input = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks)));
    process.stdout.write(await capture(input));
  } catch (error) {
    process.stderr.write(`social-card: ${/^[a-z-]+$/.test(error.message) ? error.message : 'capture-failed'}\n`);
    process.exitCode = 1;
  }
}
