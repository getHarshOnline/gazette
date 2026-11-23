import assert from 'node:assert/strict';
import test from 'node:test';
import { sourceTreatment, imageURL } from '../social_card/document.mjs';

test('declared image mode follows the loaded card color scheme before system preference', () => {
  const prepared = { mediaURL: 'https://card.test/default.png', imageVariants: {
    light: 'https://card.test/light.png', dark: 'https://card.test/dark.png',
  } };
  assert.equal(imageURL(prepared, 'dark', false), prepared.imageVariants.dark);
  assert.equal(imageURL(prepared, 'only light', true), prepared.imageVariants.light);
  assert.equal(imageURL(prepared, 'light dark', true), prepared.imageVariants.dark);
  assert.equal(imageURL(prepared, 'normal', false), prepared.imageVariants.light);
  assert.equal(imageURL({ ...prepared, imageVariants: { light: prepared.imageVariants.light } }, 'dark', true), prepared.mediaURL);
  assert.equal(imageURL({ ...prepared, imageVariants: null }, 'dark', true), prepared.mediaURL);
});

// The resolver only needs the DOM's parent relation and literal attribute access.
function element(parentNode, treatment) {
  return {
    nodeType: 1, parentNode,
    hasAttribute: name => name === 'data-treatment' && treatment !== undefined,
    getAttribute: name => name === 'data-treatment' ? treatment ?? null : null,
  };
}
const fragment = () => ({ nodeType: 11, parentNode: null });

test('selected Folio anchor carries the nearest explicit source-wrapper treatment', () => {
  const source = fragment();
  const outer = element(source, 'natural');
  const reveal = element(outer, 'monochrome');
  const paragraph = element(reveal);
  assert.equal(sourceTreatment(element(paragraph), source), 'monochrome');
});

test('the selected element overrides wrappers without rewriting the declared value', () => {
  const source = fragment(), reveal = element(source, 'monochrome');
  assert.equal(sourceTreatment(element(reveal, 'natural'), source), 'natural');
  assert.equal(sourceTreatment(element(reveal, 'future-treatment'), source), 'future-treatment');
  assert.equal(sourceTreatment(element(reveal, ''), source), '');
});

test('absence stays absent and never borrows an outer card treatment', () => {
  const source = fragment();
  // Even a caller-supplied outer relation must not extend the declared source boundary.
  source.parentNode = element(null, 'monochrome');
  assert.equal(sourceTreatment(element(element(source)), source), null);
});

test('a node outside the exact source fragment cannot supply a treatment', () => {
  const source = fragment(), other = fragment();
  assert.throws(() => sourceTreatment(element(element(other, 'monochrome')), source), /invalid-media-source/);
});
