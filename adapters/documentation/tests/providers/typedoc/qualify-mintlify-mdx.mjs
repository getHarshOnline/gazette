// Compile and render an already assembled local Mintlify tree. This is a
// Markdown/HTML and MDX representation qualifier, not a hosted Mintlify build or deployment.
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { isAbsolute, join, relative, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { evaluate } from '@mdx-js/mdx';
import * as runtime from 'react/jsx-runtime';
import { renderToStaticMarkup } from 'react-dom/server';
import remarkFrontmatter from 'remark-frontmatter';
import rehypeRaw from 'rehype-raw';
import { parseFragment } from 'parse5';

const [root] = process.argv.slice(2);
assert.ok(root && isAbsolute(root), 'supply the absolute assembled target root');

async function files(directory) {
  const result = [];
  const entries = await readdir(directory, { withFileTypes: true });
  entries.sort((left, right) => left.name.localeCompare(right.name, 'en'));
  for (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await files(path));
    else if (entry.isFile()) result.push(path);
    else assert.fail(`special target entry: ${path}`);
  }
  return result;
}

function routes(value, output) {
  if (typeof value === 'string') output.add(value);
  else if (Array.isArray(value)) for (const item of value) routes(item, output);
  else if (value && typeof value === 'object') routes(value.pages, output);
  else assert.fail('unsupported navigation shape');
}

const targetFiles = await files(root);
const pageFiles = targetFiles.filter(path => path.endsWith('.mdx') || path.endsWith('.md'));
assert.ok(pageFiles.length > 0, 'assembled target contains no documentation pages');

// Compare semantic trees, not serialized HTML spelling. The independent maintained
// browser parser decodes entities and React's text-boundary comments. No handwritten
// HTML/YAML parser and no MDX reinterpretation of Markdown are used here.
function children(nodes, convert) {
  const result = [];
  for (const node of nodes ?? []) {
    const item = convert(node);
    if (item === null) continue;
    const previous = result.at(-1);
    if (item.kind === 'text' && previous?.kind === 'text') previous.value += item.value;
    else result.push(item);
  }
  return result;
}

function hast(node) {
  if (node.type === 'comment') return null;
  if (node.type === 'text') return { kind: 'text', value: node.value };
  assert.ok(node.type === 'root' || node.type === 'element', `unexpected Markdown HTML node: ${node.type}`);
  const result = { kind: node.type, children: children(node.children, hast) };
  if (node.type === 'element') {
    result.name = node.tagName;
    result.attributes = Object.entries(node.properties ?? {})
      .map(([name, value]) => [name === 'className' ? 'class' : name, Array.isArray(value) ? value.join(' ') : String(value)])
      .sort(([left], [right]) => left.localeCompare(right, 'en'));
  }
  return result;
}

function dom(node) {
  if (node.nodeName === '#comment') return null;
  if (node.nodeName === '#text') return { kind: 'text', value: node.value };
  const result = { kind: node.tagName ? 'element' : 'root', children: children(node.childNodes, dom) };
  if (node.tagName) {
    result.name = node.tagName;
    result.attributes = node.attrs.map(({ name, value }) => [name, value])
      .sort(([left], [right]) => left.localeCompare(right, 'en'));
  }
  return result;
}

function references(tree) {
  const anchors = new Set();
  const links = [];
  function visit(node) {
    if (node.kind === 'element') {
      for (const [name, value] of node.attributes) {
        if (name === 'id') {
          assert.ok(!anchors.has(value), `duplicate rendered anchor: ${value}`);
          anchors.add(value);
        }
        if (name === 'href') links.push(value);
      }
    }
    for (const child of node.children ?? []) visit(child);
  }
  visit(tree);
  return { anchors, links };
}

const rendered = new Map();
const trees = new Map();
let markdownPages = 0;
let mdxPages = 0;
for (const path of pageFiles) {
  const source = await readFile(path, 'utf8');
  const markdown = path.endsWith('.md');
  let expected;
  function retainHtmlTree() {
    return tree => { expected = hast(tree); };
  }
  const module = await evaluate(
    { path, value: source },
    {
      ...runtime,
      baseUrl: pathToFileURL(path),
      format: markdown ? 'md' : 'mdx',
      remarkPlugins: [remarkFrontmatter],
      rehypePlugins: markdown ? [rehypeRaw, retainHtmlTree] : [],
    },
  );
  const html = renderToStaticMarkup(runtime.jsx(module.default, {}));
  assert.ok(html.length > 0, `empty rendered documentation: ${path}`);
  const observed = dom(parseFragment(html));
  if (markdown) {
    assert.ok(expected, `Markdown HTML tree not observed: ${path}`);
    assert.deepEqual(observed, expected, `Markdown HTML text/structure/attributes changed: ${path}`);
    markdownPages += 1;
  } else mdxPages += 1;
  rendered.set(relative(root, path).split(sep).join('/'), references(observed));
  trees.set(relative(root, path).split(sep).join('/'), observed);
}

const ignores = new Set((await readFile(join(root, '.mintignore'), 'utf8')).split('\n'));
function ignoredCompanion(path) {
  const escaped = '/' + path.replace(/[\\*?\[\]!# ]/g, '\\$&');
  return ignores.has(escaped);
}
function selected(route) {
  const native = `${route}.md`;
  const provider = `${route}.mdx`;
  if (rendered.has(native) && rendered.has(provider)) {
    assert.ok(ignoredCompanion(native), `paired native page must be ignored exactly: ${native}`);
    assert.ok(!ignoredCompanion(provider), `provider page must remain selected: ${provider}`);
    return [provider];
  }
  return [native, provider].filter(path => rendered.has(path));
}
function text(node) {
  return node.kind === 'text' ? node.value : node.children.map(text).join('');
}
function protectedText(node, output = []) {
  if (node.kind === 'element' && /^(pre|code|h[1-6])$/.test(node.name)) {
    output.push([node.name, text(node)]);
  }
  for (const child of node.children ?? []) protectedText(child, output);
  return output;
}
let pairedPages = 0;
for (const [path, native] of trees) {
  if (!path.endsWith('.md')) continue;
  const provider = trees.get(`${path}x`);
  if (!provider) continue;
  selected(path.slice(0, -3));
  assert.deepEqual(protectedText(provider), protectedText(native), `code/heading text changed: ${path}`);
  assert.equal(text(provider).replace(/[ \t\r\n\f]+/g, ' '), text(native).replace(/[ \t\r\n\f]+/g, ' '), `reading text changed: ${path}`);
  assert.deepEqual(rendered.get(`${path}x`), rendered.get(path), `links/anchors changed: ${path}`);
  pairedPages += 1;
}

const configuration = JSON.parse(await readFile(join(root, 'docs.json'), 'utf8'));
const navigation = new Set();
routes(configuration.navigation?.groups, navigation);
assert.ok(navigation.size > 0, 'assembled target has no navigation routes');
for (const route of navigation) {
  const matches = selected(route);
  assert.equal(matches.length, 1, `navigation route needs exactly one rendered page: ${route}`);
}

for (const [path, { links }] of rendered) {
  // Controlled Rustdoc Markdown references must retain complete local closure,
  // including source-range anchors, after actual Markdown-mode rendering.
  const origin = `https://qualification.invalid/${path.replace(/\.mdx?$/, '')}`;
  for (const href of links) {
    const url = new URL(href, origin);
    if (url.origin !== 'https://qualification.invalid') continue;
    const route = decodeURIComponent(url.pathname.slice(1)).replace(/\.mdx?$/, '');
    const matches = selected(route);
    assert.equal(matches.length, 1, `rendered local link has no unique target: ${path} → ${href}`);
    if (url.hash) {
      const fragment = decodeURIComponent(url.hash.slice(1));
      assert.ok(rendered.get(matches[0]).anchors.has(fragment), `rendered local fragment missing: ${path} → ${href}`);
    }
  }
}

process.stdout.write(`${JSON.stringify({
  mdx_pages_compiled_and_rendered: mdxPages,
  markdown_pages_compiled_and_rendered: markdownPages,
  markdown_html_parity_pages: markdownPages,
  paired_representation_parity_pages: pairedPages,
  navigation_routes_resolved: navigation.size,
})}\n`);
