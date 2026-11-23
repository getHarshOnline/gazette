// Local host loading only; the addon owns all workspace behavior and JSON encoding.
import { createRequire } from 'node:module';
import { isAbsolute, extname } from 'node:path';

const require = createRequire(import.meta.url);

/** Load one explicitly selected local native addon; never install or search for one. */
export function loadGazette(addonPath) {
  if (typeof addonPath !== 'string' || !isAbsolute(addonPath) || extname(addonPath) !== '.node') {
    throw new TypeError('Supply an absolute local .node addon path');
  }
  const native = require(addonPath);
  for (const name of ['checkWorkspace', 'fieldOrigin', 'compositionOrigin', 'inspectPagePackage', 'inspectMarkdown', 'inspectDocument', 'inspectMarkdownTree', 'inspectDocumentTree', 'renderSites', 'renderSiteFiles', 'assembleDocumentation', 'reviewPublication', 'prepareSites', 'qualifySites', 'observeDelivery', 'inspectReleaseHistory']) {
    if (typeof native[name] !== 'function') throw new TypeError(`Native addon lacks ${name}`);
  }
  return Object.freeze({
    checkWorkspace: native.checkWorkspace,
    fieldOrigin: native.fieldOrigin,
    compositionOrigin: native.compositionOrigin,
    inspectPagePackage: native.inspectPagePackage,
    inspectMarkdown: native.inspectMarkdown,
    inspectDocument: native.inspectDocument,
    inspectMarkdownTree: native.inspectMarkdownTree,
    inspectDocumentTree: native.inspectDocumentTree,
    renderSites: native.renderSites,
    renderSiteFiles: native.renderSiteFiles,
    assembleDocumentation: native.assembleDocumentation,
    reviewPublication: native.reviewPublication,
    prepareSites: native.prepareSites,
    qualifySites: native.qualifySites,
    observeDelivery: native.observeDelivery,
    inspectReleaseHistory: native.inspectReleaseHistory,
  });
}
