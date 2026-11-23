// These functions run in an empty browser document; source markup is never attached.
export function sourceTreatment(media, sourceRoot) {
  let treatment = null;
  for (let node = media; node !== sourceRoot; node = node.parentNode) {
    if (!node) throw new Error('invalid-media-source');
    if (treatment === null && node.nodeType === 1 && node.hasAttribute('data-treatment')) {
      treatment = node.getAttribute('data-treatment');
    }
  }
  return treatment;
}

export function prepare(input, treatmentWithinSource) {
  const insist = (value, reason) => { if (!value) throw new Error(reason); };
  const doc = new DOMParser().parseFromString(input.html, 'text/html');
  const one = selector => {
    const nodes = doc.querySelectorAll(selector);
    insist(nodes.length === 1, 'invalid-card-markers');
    return nodes[0];
  };
  const source = one('template[data-social-card-source]');
  const slot = one('[data-social-card-media]');
  const titleSlot = one('[data-social-card-title]');
  insist(slot !== titleSlot && !slot.contains(titleSlot) && !titleSlot.contains(slot), 'invalid-card-markers');
  const heading = source.content.querySelector('h1');
  const title = heading ? heading.textContent.replace(/[\t\n\f\r ]+/g, ' ').replace(/^ +| +$/g, '') : input.initial_title;
  insist(title.length && new TextEncoder().encode(title).length <= 1024, 'invalid-title');
  const selected = source.content.querySelectorAll(input.media_selector);
  insist(selected.length === 1, 'invalid-media-selection');
  const media = selected[0];
  let src;
  if (input.media_kind === 'image') {
    insist(media.localName === 'img', 'invalid-media'); src = media.getAttribute('src');
  } else if (media.localName === 'a') {
    src = media.getAttribute('href');
  } else {
    insist(media.localName === 'video', 'invalid-media');
    const sources = [...media.querySelectorAll(':scope > source[src]')];
    insist(!(media.hasAttribute('src') && sources.length) && sources.length <= 1, 'invalid-media');
    src = media.getAttribute('src') ?? sources[0]?.getAttribute('src');
  }
  insist(src && src.trim() === src, 'invalid-media');
  const mediaURL = new URL(src, input.document_url).href;
  insist(input.assets.some(asset => asset.url === mediaURL && asset.role === 'media'), 'unretained-media');
  let imageVariants = null;
  if (input.media_kind === 'image') {
    const address = media.getAttribute('data-gazette-image');
    const declaration = input.image_variants?.find(image => image.address === address);
    if (declaration) {
      insist(declaration.destination === mediaURL, 'invalid-image-binding');
      for (const url of Object.values(declaration.variants)) {
        insist(input.assets.some(asset => asset.url === url && asset.role === 'media'), 'unretained-media');
      }
      imageVariants = declaration.variants;
    }
  }
  const treatment = treatmentWithinSource(media, source.content);
  const label = media.getAttribute('aria-label') ?? media.getAttribute('alt');
  source.remove(); slot.replaceChildren(); titleSlot.textContent = title;
  const assets = new Map(input.assets.map(asset => [asset.url, asset]));
  const admitted = (value, role) => {
    const resolved = new URL(value, input.document_url).href;
    insist(assets.get(resolved)?.role === role, 'unretained-card-resource');
    return resolved;
  };
  // Only the selected card modules may execute. The source's scripts were discarded above.
  for (const script of doc.querySelectorAll('script')) {
    if (script.type === 'module' && script.hasAttribute('src')) {
      script.setAttribute('src', admitted(script.getAttribute('src'), 'module'));
      insist(!script.textContent.trim(), 'invalid-card-script');
    } else if (script.type === 'importmap' && !script.hasAttribute('src')) {
      const map = JSON.parse(script.textContent);
      insist(map && typeof map === 'object' && !Array.isArray(map) && Object.keys(map).every(key => key === 'imports'), 'invalid-import-map');
      insist(map.imports && typeof map.imports === 'object' && !Array.isArray(map.imports), 'invalid-import-map');
      for (const value of Object.values(map.imports)) insist(typeof value === 'string' && admitted(value, 'module'), 'invalid-import-map');
    } else throw new Error('invalid-card-script');
  }
  insist(!doc.querySelector('base, iframe, frame, object, embed, audio, video, img, source, template, meta[http-equiv]'), 'invalid-card-element');
  for (const element of doc.querySelectorAll('*')) {
    for (const attribute of element.attributes) {
      insist(!/^on/i.test(attribute.name) && attribute.name !== 'srcdoc', 'invalid-card-attribute');
    }
  }
  for (const link of doc.querySelectorAll('link')) {
    if (link.rel === 'stylesheet') link.href = admitted(link.getAttribute('href'), 'stylesheet');
    else if (link.rel === 'preload' && link.as === 'font') link.href = admitted(link.getAttribute('href'), 'font');
    else throw new Error('invalid-card-link');
  }
  const importMaps = [...doc.querySelectorAll('script[type="importmap"]')].map(script => script.textContent);
  return { html: '<!doctype html>\n' + doc.documentElement.outerHTML, title, mediaURL, imageVariants, treatment, label, importMaps };
}

// Resolve only an explicitly declared alternative after the card's CSS has loaded.
export function imageURL(prepared, colorScheme, prefersDark) {
  const modes = colorScheme.split(/\s+/);
  const mode = modes.includes('dark') && !modes.includes('light') ? 'dark'
    : modes.includes('light') && !modes.includes('dark') ? 'light'
    : prefersDark ? 'dark' : 'light';
  return prepared.imageVariants?.[mode] ?? prepared.mediaURL;
}

export function installCompletion() {
  globalThis.__gazetteCard = { result: null, errors: [] };
  document.addEventListener('gazette:social-card-media-complete', event => {
    const state = globalThis.__gazetteCard;
    const slot = document.querySelector('[data-social-card-media]');
    if (event.target !== slot || state.result !== null || !state.requested) {
      state.errors.push('invalid-completion'); return;
    }
    const detail = event.detail;
    if (!detail || typeof detail !== 'object' || Array.isArray(detail)) {
      state.errors.push('invalid-completion'); return;
    }
    const keys = Object.keys(detail).sort().join(',');
    const reasons = ['unsupported-treatment', 'invalid-media', 'invalid-geometry', 'invalid-palette', 'render-failed'];
    if (detail.status === 'complete' && keys === 'status') state.result = { status: 'complete' };
    else if (detail.status === 'failed' && keys === 'reason,status' && reasons.includes(detail.reason)) state.result = { status: 'failed', reason: detail.reason };
    else state.errors.push('invalid-completion');
  }, true);
  addEventListener('error', () => globalThis.__gazetteCard.errors.push('card-resource-error'), true);
  addEventListener('unhandledrejection', () => globalThis.__gazetteCard.errors.push('card-module-error'));
  document.addEventListener('securitypolicyviolation', () => globalThis.__gazetteCard.errors.push('blocked-card-effect'));
}

export async function insertMedia(input, prepared) {
  const slot = document.querySelector('[data-social-card-media]');
  const media = document.createElement(input.media_kind === 'image' ? 'img' : 'video');
  if (prepared.treatment !== null) media.setAttribute('data-treatment', prepared.treatment);
  if (prepared.label !== null) media.setAttribute('aria-label', prepared.label);
  slot.replaceChildren(media);
  if (input.media_kind === 'image') {
    media.src = prepared.mediaURL;
    await media.decode();
    if (!media.naturalWidth || !media.naturalHeight) throw new Error('invalid-media');
  } else {
    media.muted = true; media.preload = 'auto'; media.playsInline = true;
    const loaded = new Promise((resolve, reject) => {
      media.addEventListener('loadeddata', resolve, { once: true });
      media.addEventListener('error', () => reject(new Error('invalid-media')), { once: true });
    });
    media.src = prepared.mediaURL; await loaded; media.pause();
    const time = (input.video_time_ms ?? 0) / 1000;
    if (!Number.isFinite(media.duration) || time >= media.duration || !media.videoWidth || !media.videoHeight) throw new Error('invalid-media');
    if (time !== media.currentTime) {
      await new Promise((resolve, reject) => {
        media.addEventListener('seeked', resolve, { once: true });
        media.addEventListener('error', () => reject(new Error('invalid-media')), { once: true });
        media.currentTime = time;
      });
    }
    if (media.readyState < 2 || Math.abs(media.currentTime - time) > 0.001) throw new Error('invalid-media');
  }
  await document.fonts.ready;
  if ([...document.fonts].some(font => font.status === 'error')) throw new Error('invalid-font');
  await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  globalThis.__gazetteCard.requested = true;
  slot.dispatchEvent(new CustomEvent('gazette:social-card-media-ready'));
}
