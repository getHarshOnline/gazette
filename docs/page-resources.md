# Page packages and page-local resources

Give a publication page its own stylesheet, browser module, font or image. Gazette compiles the declared files and passes their URLs to the Theme in `presentation.resources`. The template still needs to include the stylesheet or script.

This applies to pages rendered by a Theme, including product help. Use the [Mintlify documentation target](mintlify.md) for the repository’s developer manuals and API reference.

## Declare a page resource

Framed authored pages and framed project-native documentation can declare resources from their existing content source. The following declarations assume those files exist in that selected source. They are frontmatter fragments, alongside the page's complete `[content]` declaration.

Put resource tables at the frontmatter root, alongside `[content]`:

```toml
[[resources]]
inputs = ["main.scss", "_tokens.scss"]
build = { kind = "stylesheet", entry = "main.scss" }
output = "/assets/article.css"

[[resources]]
inputs = ["main.ts", "dep.ts"]
build = { kind = "browser-module", entry = "main.ts" }
output = "/assets/article.js"

[[resources]]
inputs = ["type/article.woff2"]
build = { kind = "woff2-font", entry = "type/article.woff2" }
output = "/assets/type/article.woff2"

[content]
# The page's existing required structural fields follow here.
```

Build the site to produce `/assets/article.css`, `/assets/article.js` and `/assets/type/article.woff2`. Each declaration owns its own output path. The publication build uses the same adapters as site-level resources. See [browser resources](render-resources.md#capabilities-providers-and-selected-site-resources) for supported inputs. TypeScript erasure removes types without checking them.

## Consume the resolved resources in a Theme

The owning page and its aliases receive `presentation.resources`, an ordered list of `kind`, `url`, `output`, `sha256` and `bytes`. Use `url` in markup; Gazette has already applied the publication's public or local-preview base. `output` is the site-relative output path. Other pages receive their own list, empty when they have no page resources.

```html
{% for resource in presentation.resources %}
  {% if resource.kind == "stylesheet" %}
  <link rel="stylesheet" href="{{ resource.url }}">
  {% elif resource.kind == "browser-module" %}
  <script type="module" src="{{ resource.url }}"></script>
  {% endif %}
{% endfor %}
```

The Theme places this loop where its document structure needs the resources. Theme and page JSON receive the same resolved values. Page resources remain separate from the complete top-level `resources` map, `presentation.theme_resources` and `presentation.capabilities` provider outcomes. `presentation.theme_resources` contains required Theme resources in requirement order, followed by resources explicitly selected with `use = "site"`. Dependency-only resources are not executable roots.

## Keep page companions with their owner

A framed `guide/index.md` or `guide/index.markdown` owns the retained companions beneath `guide/`. A standalone `guide.md` does not claim a sibling directory. Gazette classifies companions as notebooks, programs, stylesheets, WOFF2 font candidates, supported static assets or unhandled inputs.

An article may also own one unframed sibling `notes.md`, including when its bundle is the selected source root. Notes share the article's identity and have no independent route or search record. Other Markdown beneath a leaf package is refused. See [article notes](content-model.md#article-notes-stay-with-their-article) for rendering and anchor behavior.

Authoring interfaces can select notes as editable documents. Other companions can be supplied as opaque page inputs, which do not accept text overlays. The compiler resolves ownership once; SDK, LSP, MCP and VS Code show that result. Unhandled companions appear in authoring diagnostics and prevent a successful publication build when they remain unconsumed.

Compiled companions require an explicit page route. Without a resource declaration, a leaf package admits one top-level non-test JavaScript/TypeScript program entry and one top-level non-partial CSS/Sass/SCSS stylesheet entry. More than one candidate of either kind requires explicit declarations. Nested files, TypeScript declarations, tests and Sass partials remain inputs, not standalone public outputs. Notebooks and static files retain their own format and asset rules.

## Bind inputs and dependencies

Name inputs relative to the captured source. Do not use the page's address, corpus prefix or public URL. The entry must be in the unique input set. Imports stay within that set and the supported resource profile. Gazette preserves resource order and treats input lists as sets. Output name, kind, execution order and owner come from declarations, not filenames.

Use `build.imports` to map a browser module's import aliases to [workspace libraries](render-resources.md#dependency-resources-across-selected-sources). Gazette emits each library once at its configured URL. The page's other files still come from its own source. The alias does not read another source or activate a capability.

The Theme and site model receive the same module graph. A Theme can serialize its import map and render each page's supplied `modulepreload` dependencies. Neither adds a script that runs the module. Gazette does not invent priorities, timestamps or a separate preload manifest. Plain Markdown without frontmatter cannot declare page resources.

## Validate the complete output

Stylesheets may carry `references`, mapping each actual decoded CSS URL to its checked site-output path. References resolve from the emitted stylesheet, not its source file or owning page. They may target selected static assets or compiled resources. Missing or out-of-site targets fail before staging; no dependency is fetched or selected implicitly.

Page resources share the site's inventory with pages, aliases, static assets, site resources and machine outputs. Conflicting claims fail even when their bytes match. Missing inputs, invalid recipes and undeclared imports reject the site. Use the retained publication build path: rendering a bare Theme page cannot silently omit its declared resources.

Only actual compiler dependencies consume the corresponding opaque inputs. Listing an unused notebook or arbitrary file in `inputs` does not make it compiled; other unhandled companions still fail the build. Resource declarations and their encoded text consume the authored graph bounds. Site and page resources also share metadata and emitted-byte limits before the complete site inventory check. These bound records and bytes, not adapter CPU or all intermediate allocations.

Private `page_resources` evidence records each page's source, recipe, dependencies, compiler and output identities. Native source coordinates do not enter the public resource list. Later rebuilds use their captured bytes rather than reopening a source during rendering. Local rendering remains draft-inclusive and separate from disclosure review or release.

## WOFF2 fonts

The 0.1 font profile admits WOFF2 only. A font recipe must select exactly one retained file and performs complete WOFF2 decompression plus required `head`, `maxp` and `name` table parsing through the pinned decoder/parser pair. TrueType- and CFF-flavored single fonts are admitted; collections and unknown flavors are refused. Output preserves the exact retained WOFF2 bytes, while private evidence binds both encoded and reconstructed SFNT identities. No system font lookup, live filesystem read, network provider, automatic suffix admission or font subsetting occurs.

`@font-face` is a closed stylesheet profile rather than a general opaque-at-rule escape. It requires exactly one family and one source descriptor. A URL-backed face requires one URL with explicit `format(woff2)`. A local fallback face may instead list 1–16 literal `local()` font names, each nonempty and control-free after decoding, with both its decoded name and the CSS parser’s serialized name (including any quotes or escapes) bounded to 256 UTF-8 bytes. Local names remain browser font lookups: they add no retained URL, font-byte evidence or installation claim. Generic family keywords, mixed local/URL lists, multiple URL candidates, technology hints, unknown descriptors and duplicates are refused. Optional style, weight, stretch and Unicode-range descriptors may appear once. The profile also admits one `font-display` value from `auto`, `block`, `swap`, `fallback` or `optional`; one finite nonnegative percentage for `size-adjust`; and either `normal` or one finite nonnegative percentage for each of `ascent-override`, `descent-override` and `line-gap-override`. No expression, unit substitution or duplicate spelling is accepted. The URL is resolved relative to the emitted stylesheet and must identify the emitted font resource; queries, fragments, cross-site, escaping and missing targets remain invalid under the existing complete output-reference closure. Preview serves the producer-declared resource as `font/woff2`; a filename alone never grants that declaration.

## Supply a Theme file from a shared source {#theme-resources-can-use-retained-shared-inputs}

A workspace may supply a Theme resource's already declared logical input from another configured source through `design_inputs` and `theme_packages.resource_inputs`. The [workspace example](workspace-presentation.md#supply-declared-theme-inputs-from-a-shared-source) defines the exact alias, source and entry mapping. The Theme manifest still owns the resource recipe and complete input list; workspace composition independently selects its output and use. Merely declaring an alias emits nothing and grants no native read permission.

Package-local and mounted inputs share one virtual resource-input tree. For example, a Theme-local Sass entry can import a declared mounted token partial at its logical path without copying the token source into the Theme repository. Existing resource adapters consume retained bytes from that tree, with the same input, dependency, compiler and output checks as local inputs. Their public resource records and serving URLs remain unchanged.

The selected Theme/dependency closure brings its mounted source keys into the normal bounded acquisition, once per exact source key. Missing or nonregular inputs, undeclared targets and package-local/mounted ambiguity refuse metadata; no fallback file search or successful partial resource is returned. Compilation and retained replay never reopen those native sources. The private resource evidence preserves each logical input's original alias, source key, entry and exact retained identities rather than treating the virtual path as native provenance.

Shared inputs apply to Theme resource recipes, not page companions, template mounts or Mintlify inputs. Page-local resources continue to use their owning content source.

For a mounted `dot-image` recipe, the existing explicit diagram-acquisition path resolves the single declared input to its original retained source and entry. It requires the same pinned `dot_renderer` executable and execution permission as local DOT, validates the generated PNG and retains its exact input/output binding. Resource rendering checks those retained bytes, then expresses input identity and dependency paths in the virtual resource namespace; it does not confuse those paths with native provenance, reread the source or execute Graphviz again. The executable pin alone does not make the generator's libraries, plugins, fonts or other ambient dependencies hermetic.

## Captured image transformations

For placing a figure in an article and continuing the writing, return to [Figures, notes and notebooks](https://rawthoughts.in/gazette/figures-notes-and-math/).

Image recipes use that same input, compilation, output and evidence path. They do not expose a Tera filesystem function or create another asset namespace:

```toml
[[resources]]
inputs = ["photos/source.png"]
output = "/assets/thumbnail.png"
build = { kind = "raster-image", entry = "photos/source.png", input_format = "png", apply_orientation = true, resize = { operation = "fit", width = 640, height = 480 }, encoding = { format = "png" } }
```

`input_format` is `png`, `jpeg` or single-image `webp`, independent of the filename. `resize` selects `scale` (exact width/height), `fit-width`, `fit-height`, `fit` (bounded without upscaling), or `fill` (center-cropped exact width/height). The fixed raster-adapter profile uses Lanczos3. Orientation is applied before resizing only when selected. Output uses eight-bit channels; PNG and lossless WebP preserve transformed alpha. Use `encoding = { format = "webp" }` for lossless eight-bit WebP output. With unchanged dimensions and no crop, WebP preserves every decoded eight-bit RGBA channel, including RGB beneath zero alpha; resize or orientation remains an explicit transformation. Higher-depth input is quantized by this explicit profile, not preserved at its original depth. JPEG uses an explicit `quality` in 1–100 and `background` RGB triplet, for example `encoding = { format = "jpeg", quality = 85, background = [255, 255, 255] }`.

The shared public resource kind is `png-image`, `jpeg-image` or `webp-image`. Theme chooses how to present its URL and supply authored alternative text; Gazette does not invent captions or image meaning. Both page resources and site-selected resources carry `image` geometry in the same public resource projection: `source` is the stored pixel size, `oriented` is its size after selected orientation handling, and `output` is the actual encoded size. Each has `width` and `height`. The raster adapter decodes the final encoded artifact to check its output dimensions; renderers do not reread images or guess dimensions from a resize request. Theme can set intrinsic HTML dimensions from `resource.image.output.width` and `.height`; JSON receives those same facts. Scripts, styles and fonts omit `image`. Preview and disclosure dispatch use the generated representation, even for an extensionless or misleadingly named output. Private evidence retains the exact recipe, source hash, compiler profile and output hash.

The raster adapter consumes only retained bytes and uses the maintained image library for transformations. Resource input/output ceilings apply alongside `resource::Limits.raster` pixel/buffer ceilings. These are bounded operations, not a process-wide allocation sandbox. Sampling is premultiplied-alpha in encoded channel space, not an ICC/color-managed transform. Re-encoding does not grant disclosure approval; the existing output review remains separate. Animation, video, other raster codecs, automatic responsive variants, colour-profile transformation, wide-gamut output and browser/color-management qualification are not supplied by this PNG/JPEG/WebP profile.

### Bind an original image to its compiled rendition

Add `rendition = true` beside `inputs`, `build` and `output` on a page raster resource. Markdown and `images.default`, `light` and `dark` continue to reference the original retained source. The native reference resolver maps that recipe’s entry to its compiled output for this page, preserving query strings, fragments and public/local serving bases. Only one rendition may bind an original per page; additional variants remain ordinary resources unless explicitly selected as responsive candidates. Scripts are not searched and rewritten for matching strings.

The public resource projection exposes `rendition_of`, the original image’s resolved serving URL, alongside the derived `url` and verified `image.output` dimensions. A browser component can use that mapping without private paths or output-suffix guesses. Independently selected originals remain available for their other uses. After resource compilation, image occurrences carry `intrinsic` dimensions, and native Markdown HTML emits width/height attributes with that aspect ratio. An authored display width takes effect in the initial HTML, with height scaled proportionally and rounded to a positive integer; without verified dimensions only the declared width is emitted. The verified raster display dimensions remain separate in `intrinsic`. Ordinary retained PNG, JPEG and WebP images also receive native dimensions when the bounded decoder verifies their pixels and display orientation. Inspection keeps stored width and height unchanged; its optional `display` extent accounts for orientation. Missing orientation metadata means the stored extent applies; unrecognised orientation metadata leaves display geometry unknown. Failed inspection leaves the image transport unchanged and supplies no invented dimensions.

### Supply responsive image sizes

Keep the full fallback as `rendition = true`, add smaller outputs with `responsive = true`, and declare the image's intended layout in `images.sizes`:

```toml
[[resources]]
rendition = true
inputs = ["portrait.png"]
output = "/images/portrait.webp"
build = { kind = "raster-image", entry = "portrait.png", input_format = "png", apply_orientation = true, resize = { operation = "fit-width", width = 1600 }, encoding = { format = "webp" } }

[[resources]]
responsive = true
inputs = ["portrait.png"]
output = "/images/portrait-small.webp"
build = { kind = "raster-image", entry = "portrait.png", input_format = "png", apply_orientation = true, resize = { operation = "fit-width", width = 640 }, encoding = { format = "webp" } }

[[images]]
default = "portrait.png"
sizes = "(max-width: 48rem) 100vw, 50vw"
```

Markdown keeps referencing `portrait.png`. Native HTML receives `srcset` with the verified output widths and the declared `sizes`; `src` remains the full fallback. The browser chooses a candidate for its viewport and display density. Gazette does not infer layout from filenames, container classes or an image's position on the page. `sizes` accepts at most 1,024 bytes of literal ASCII layout syntax; the browser evaluates its media conditions and lengths.

Each original admits up to 16 additional candidates. They must share the fallback's original, input format and orientation setting, using `fit-width`, `fit-height` or `fit` throughout. Cropped `fill` and distorted `scale` recipes cannot form a responsive group. Integer rounding during proportional resizing is expected: descriptors use independently decoded output widths, including when a fit bound produces a narrower image than requested. Duplicate output widths fail. Missing fallback, missing verified geometry, missing `sizes` on a declared responsive image, and `sizes` without any responsive candidates also fail. Responsive occurrences may retain a query string but cannot select an image fragment.

The image model exposes `responsive.sizes` and `responsive.default`, with `responsive.light` and `responsive.dark` when those modes are declared. Each candidate is `{url, width}`, ordered by increasing pixel width and including the mode's fallback. Existing `destination` and `variants` keep the full fallback URLs for image expansion. Native HTML also emits a declared dark mode as a `picture` source ahead of the default `img`, with the dark candidates and the same `sizes`, so the browser chooses the dark rendition before any script runs; the source's `media` follows the Theme's [starting colour mode](theme-reference.md#initial-color-mode). A Theme changing display mode updates that source's `media`, or the image's `srcset`, `sizes` and `src` where there is no source, from the same image record; it does not derive another filename. Page resources expose `responsive_of` for additional candidates and retain the unique `rendition_of` on the fallback. Both identify the original's serving URL, using the selected public or local base.

## Static SVG images

The separate `svg-image` recipe admits exactly one retained UTF-8 SVG and emits its exact bytes. It is an inert static-image profile, not a general XML, HTML or SVG application runtime:

```toml
[[resources]]
inputs = ["marks/icon.svg"]
output = "/assets/icon.svg"
build = { kind = "svg-image", entry = "marks/icon.svg" }
```

The root must be an unprefixed `svg` element with the exact SVG default namespace. XML declarations, DTDs, processing instructions, prefixed elements, `script`, `style`, `foreignObject`, inline style, event attributes, embedded data, prefixed attributes and external references are rejected. `href` may name only a local fragment. Parsing is bounded by the same retained-input/output limits plus finite event, nesting, name, attribute and value ceilings. The adapter neither fetches dependencies nor rewrites markup.

This profile is suitable for an explicitly selected site icon or ordinary Theme image. A link-preview role remains PNG/JPEG because social-preview consumers do not provide a portable SVG contract. The SVG profile does not support animation or active content; accessibility and publication review still depend on the selected image and presentation.

### Inline SVG figures

An image declaration can opt into inline rendering while retaining its external SVG file:

```toml
[[images]]
default = "figure.svg"
inline = true
width = 480
```

The SVG must already be an automatic leaf-page companion or an explicit `svg-image` page resource. Inline rendering adds stricter validation; it does not make arbitrary SVG or raw HTML admissible. Unsupported content fails with a diagnostic instead of being silently removed. The emitted external resource retains its original bytes. A declared width sizes the inline SVG before scripts run, preserving its intrinsic viewport ratio, or its view-box ratio when intrinsic dimensions are absent. Theme CSS can constrain that width to the available column. [Image renditions](markdown.md#declare-image-renditions) explains occurrence identity, the hidden image fallback and viewer integration.

The first inline profile supports exactly `svg`, `g`, `title`, `desc`, `path`, `text`, `tspan`, `rect`, `circle`, `ellipse`, `line`, `polyline` and `polygon`. Names are case-sensitive, the single root declares `xmlns="http://www.w3.org/2000/svg"`, and nested SVG roots or alternate namespaces are refused. It admits geometry and transforms, fill/stroke and their presentation attributes, opacity, font family/size/weight/style, text anchoring and spacing, `viewBox`, dimensions and aspect ratio. `id`, `aria-labelledby`, `aria-describedby` and `role="img"` supply accessible descriptions. IDs must be unique ASCII letters, digits, hyphens or underscores, at most 256 bytes; accessibility references must resolve inside the same SVG. Each occurrence receives its own generated ID prefix, including repeated figures and notebook cells.

Live typography and colours can use `currentColor` or CSS variables such as `font-family="var(--type-family-reading, sans-serif)"` and `fill="var(--color-ink, #292722)"`. Variables accept one `--token` and an optional simple fallback without nested functions. Presentation values use ASCII letters, digits, whitespace and `- _ # % . , ( ) +`; supported value functions are `var`, `rgb`, `rgba`, `hsl`, `hsla`, `translate`, `scale`, `rotate`, `matrix`, `skewX` and `skewY`. CSS escapes, style attributes/elements, classes, events, scripts, foreign content, links, paint URLs, embedded media, filters and animation are not part of this profile. XML declarations, DTDs, processing instructions, CDATA and unknown entities are refused. Text and attribute entities are decoded, validated and safely re-escaped; comments are omitted from the canonical inline markup.

Admission limits retained input to one MiB, 100,000 decoded events, 64 nested elements, 64 attributes per element and 64 KiB per attribute value. The enclosing Markdown plan and final output enforce their own byte limits. Inline rendering does not fetch files, execute code, infer captions or change image ownership.

Explicit variants can be composed with a stylesheet resource, for example:

```css
.illustration {
  background-image: image-set("thumbnail.png" 1x type("image/png"),
                             "thumbnail-dense.jpg" 2x type("image/jpeg"));
}
```

Declare both raster recipes and the stylesheet using the existing resource settings. Relative URLs resolve from the stylesheet's emitted route, not its source folder. Every candidate must exist in the final output, even one a browser might skip because of its resolution or type hint. The same checked references reach Theme and the model; a missing candidate refuses the build before staging. Gazette preserves the native CSS rather than choosing a device-dependent image. Variant generation remains explicit; the browser chooses among the emitted candidates. The syntax follows the [CSS Images image-set definition](https://www.w3.org/TR/css-images-4/#image-set-notation).

## Resource caching

Multi-site SDK rendering shares a session-local resource cache; watch keeps it across generations. A hit requires the same complete input tree, recipe, selected module URLs and effective limits. Each site still resolves its own output and URL, and the application checks dependency and adapter evidence before using the result. Acquisition, semantic resolution, Theme rendering and output closure still run.

The default cache holds at most 128 successful requests and 32 MiB of logical payload: keys, output, dependency/reference strings and compiler profiles. It evicts oldest entries first and bypasses oversized results. `ResourceCacheLimits` can change those allowances; zero disables retention. Container overhead and returned copies are additional memory. Cache hits do not change artifact identity.

The persistent whole-site cache is separate. It stores an already completed output tree under its source, configuration, Theme/template, tool and semantic identities. A warm whole-site hit can bypass rendering. A clean cache write must revalidate the complete plan under the same adapters, options and limits. Neither cache grants publication approval or bypasses the relevant output and release checks.

### External SVG layout

The existing SVG validator records root viewport geometry for both explicit `svg-image` resources and automatic leaf-page companions. The public `svg` evidence is separate from raster `image` geometry. Positive finite absolute root width and height establish the ratio; otherwise a valid `viewBox` may establish it. View-box offsets do not change the ratio. Percentage and automatic lengths do not establish intrinsic dimensions.

Native image HTML uses a known natural width or the authored display width and reserves the proportional height before scripts run. A view-box ratio alone does not invent a natural pixel size. For both rasters and SVGs, fragment-selected views and mode variants with unverified or differing ratios receive no borrowed geometry. Exact SVG bytes and the existing admission policy remain unchanged.
