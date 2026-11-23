# JSON-LD from the shared page model

When you add structured data to a Theme, use the JSON-LD Gazette supplies with the page. It comes from the same `PageProjection` as the HTML, so the article, publisher and relationships come from the content you are rendering. The Theme chooses where to embed that data.

Set `machine = { json_ld = true }` in the existing brand publication binding. Gazette then supplies the template's `json_ld` value and writes the site's `site.jsonld` file. Embed the page value with the [JSON script encoder](tera.md#json-script-data):

```html
{% if json_ld is defined %}
<script type="application/ld+json">
  {{ json_ld | json_script }}
</script>
{% endif %}
```

`json_script` encodes the supplied value for a script element. Use it instead of interpolating raw JSON; it needs no additional `safe` filter. The filter handles encoding, while Gazette's page model determines what the data says.

> [!IMPORTANT]
>
> Local builds can include drafts. Check the selected pages before exposing their output. Producing JSON-LD does not publish the page or guarantee that a search engine will index it or show a rich result.

For the publisher's setup, follow [Help readers find the work](https://rawthoughts.in/gazette/help-readers-find-the-work/).

## Read an emitted article graph

The [first-publication tutorial](https://rawthoughts.in/gazette/first-publication/) contains an article, “Keep the observation with its context”, inside the Field Notes collection. Enable JSON-LD in that publication and render the article. This excerpt keeps its identities and breadcrumb relationship; other properties are omitted:

```json
{
  "@context": {
    "@version": 1.1,
    "@vocab": "https://schema.org/",
    "url": {
      "@id": "https://schema.org/url",
      "@type": "@id"
    }
  },
  "@id": "https://field-notes.example/keep-the-context/#article",
  "@type": "Article",
  "identifier": "content:keep-the-context",
  "url": "https://field-notes.example/keep-the-context/",
  "mainEntityOfPage": {
    "@id": "https://field-notes.example/keep-the-context/",
    "@type": "WebPage",
    "name": "Keep the observation with its context",
    "url": "https://field-notes.example/keep-the-context/",
    "breadcrumb": [
      {
        "@type": "BreadcrumbList",
        "numberOfItems": 2,
        "itemListElement": [
          {
            "@type": "ListItem",
            "position": 1,
            "name": "Field Notes",
            "item": { "@id": "https://field-notes.example/" }
          },
          {
            "@type": "ListItem",
            "position": 2,
            "name": "Keep the observation with its context",
            "item": { "@id": "https://field-notes.example/keep-the-context/" }
          }
        ]
      }
    ]
  }
}
```

Start with the article's `identifier`: `content:keep-the-context` is the key from its source. Its `url` comes from the resolved page route. In this example the serving and canonical addresses coincide. [Local-link previews](preview.md) use their loopback routes; [syndicated copies](routing.md#syndicated-bodies) keep a separate reference to the original page.

Next, follow `mainEntityOfPage`. The article and the `WebPage` carrying it share a URL here but have distinct public identities: the article uses the canonical address plus `#article`, while its page uses the canonical address. The article holds the content key, lifecycle, language and keywords; the page holds only its name, address and place, so the same facts are not stated twice. This lets the breadcrumb name the page without treating the article itself as a `WebPage`. The `BreadcrumbList` starts at Field Notes and ends at that presentation page. Gazette takes its positions from the collection structure, not from path segments or alphabetical order.

When debugging a Theme, compare the page model's `json_ld` with the JSON decoded from that page's `application/ld+json` script. They should describe the same value. For `site.jsonld`, compare the corresponding discovery/listing entry in `pages[id].json_ld`: that entry can omit relationships present on the direct page. The tutorial remains draft content for local learning, so its direct page graph alone is no reason to include it in a public index.

## Identities and vocabulary

Public page values, generated listings and `site.jsonld` use Schema.org terms with a local JSON-LD 1.1 context. Gazette includes that context instead of fetching one. The public serializer selects supported publication facts explicitly; it does not rename internal evidence, provenance or operational state into superficially similar standard properties.

A content key stays a literal `identifier`. Routed works use canonical public IRIs: an article uses its canonical page address plus `#article`, and its presenting `WebPage` uses the canonical address. Other pages, collections, journeys and generated listings use their canonical page addresses. These identities stay public in local previews while serving `url` fields may use loopback addresses. Breadcrumbs, article presentation pages, linked works and public ItemLists use the same identities. Unrouted works retain document-local blank nodes; equal blank-node labels across separate documents do not establish shared identity. A principal with a configured website uses that website's canonical address plus `#person` or `#organization`, so every page and site names the same person or organisation; give each principal of a kind its own website. A principal without a website, and each scoped taxonomy term, keeps a document-local blank node. Route aliases do not become `sameAs`.

Each document describes the page's own work once. Every other mention of a work, whether in `isPartOf`, `hasPart`, `subjectOf`, a journey or a listing, is a reference with its identity, content key, type, name and serving `url`; its keywords, lifecycle and language stay on its own page. A breadcrumb item carries the `name` shown in the trail and the target's `@id`. Empty relationships are omitted rather than written as empty arrays.

Titles supply names. Publisher, author and rights-holder fields come from their resolved declarations, with each role kept separate. Supply the principal's kind and name where needed; Gazette will not invent them from its key. The same applies to contacts, social links, dates, offers, licences and rights.

A principal's configured website supplies `Person.url` or `Organization.url` through its public `canonical_url`, including in preview. Configured `same_as` entries become `sameAs` references with `@id` values. Gazette neither discovers these entries from social profiles, aliases or publisher roles nor verifies them remotely. A Theme's navigation can still use the separate serving `url` to keep a preview link local.

## How page fields map to JSON-LD {#projection-contract}

Use this table to trace a public property back to the page model. Private inspection includes the complete structure. Public aggregate output follows the [discovery rules](render-inputs.md#discovery-without-weakening-materialization), filtering ineligible relationship endpoints, including breadcrumbs.

A filtered journey keeps each remaining member's original one-based position and counts only the members it lists. If any declaration behind a project relationship is ineligible, that relationship is omitted from aggregate `about`/`mentions`. Filtering this output leaves authored prose links and the underlying graph intact.

| Resolved input | JSON-LD property | What to expect |
| --- | --- | --- |
| Content family | Article → `Article`; collection/journey or explicitly configured listing → `CollectionPage`; other pages → `WebPage` | An extension key does not create a Schema.org type or make its page an article. |
| Title, lifecycle, language and retained body text | `name`, article-only [`headline`](https://schema.org/headline), `creativeWorkStatus`, optional `inLanguage`, `text` | `Article`, `BlogPosting` and `TechArticle` reuse the article title as `headline`, with no new identity. Language uses the page's explicit value or publication default; unresolved language stays absent. `text` carries the body, not a generated summary. |
| Root description and authored dates | `description`, `datePublished`, `dateModified` | The description stays plain text. Dates retain XML Schema `date` or `dateTime` types. Missing values stay absent, date-only values stay date-only, and shallow linked records receive no extra description or dates. |
| Explicit body-summary text | `abstract` | The same retained prefix used by Theme and search, separate from description. An absent summary stays absent; HTML and the summary marker are excluded. |
| Content licence | `license` with the exact URI as `@id` | Uses the content field, without falling back to a repository licence, fetching terms or granting publication permission. |
| Site terms of use | `usageInfo` with the configured [terms address](deployment-output.md#terms-of-use) as `@id` | Appears on each page's own node when the publication declares `terms`. It never fills an absent content `license`; a page's explicit licence stays alongside it. |
| Site publisher | `publisher` with the declared `Person` or `Organization` type, optional name and configured website | Uses the exact publisher principal through the same profile projection as attributions. Publishing a site does not make the principal its author or rights holder; a site address does not become an undeclared organisation website. |
| Utility-page subtype and explicit subject | `AboutPage` with an optional principal `mainEntity`, or `WebPage` with a distinct `Service` `mainEntity` | Uses the [declared subject](content-model.md#declare-an-about-page-or-a-service-subject). An About page without a subject invents no `mainEntity`; existing project and brand relationships remain intact. Principal identity comes from the existing catalog. Service identity, name, provider and optional description are explicit; page titles, publishers, dates and lifecycle never fill missing service facts. Existing listing and journey `ItemList` identities remain unchanged. |
| Scoped attributions | `author`, `copyrightHolder` | Principal kind and optional name come from the referenced person or organisation. Each role keeps its declared principals. Eligible published content uses the same site author default as its byline; a missing author does not fall back to the publisher or a guessed name. |
| Direct subject and mentions | `about`, `mentions` | The explicit project kind supplies its type; omission remains `Thing`. Project entities use their explicit public IRI or canonical landing plus `#entity`. Configured internal reading destinations enter `subjectOf` only when their resolved relationship is About this project, with the target’s actual title, type and work identity. A Mentions index may itself declare About; its navigation label is not its relationship. |
| Editorial residence | `isPartOf` | Keeps the direct collection or sole containing journey as a reference. Array-shaped output does not permit extra memberships; a page with neither omits the property. |
| Children and editorial links | `hasPart`, `relatedLink` | Children are references with resolved serving URLs; a page without children or FAQ answers omits `hasPart`. A related page need not share membership or subject matter. |
| Journey members | `mainEntity` → `ItemList` with positioned `ListItem` values | Keeps traversal order, original one-based position, exact title/type and resolved URL. A member without a route keeps its identity without a guessed link. |
| Structural breadcrumb paths | `breadcrumb` → one `BreadcrumbList` per path | Positions start at one and the last item is the page itself. Each `ListItem` carries the trail `name` and the target `@id`. A path holding only the page itself is omitted. Missing routes stay missing. |
| Literal keywords and classification bindings | `keywords` → strings and `DefinedTerm` values with `inDefinedTermSet` | Each term keeps its exact taxonomy identity and label; equal keys in different taxonomies remain distinct. The native view retains the taxonomy language constraint. A label does not create a subject relationship. |
| Authored image occurrences | `image` → `ImageObject` values | Each occurrence keeps its resolved `contentUrl`, alt text as `description` and optional title as `name`. An explicitly bound [figure caption](theme-reference.md#bind-an-image-to-its-authored-caption) supplies `caption` from its admitted reading text. Images inside components explicitly declared `presentation_only` are excluded. No image is guessed from a stylesheet, icon or unreferenced asset. |
| Validated FAQ components | `hasPart` → `FAQPage`, ordered `Question` values and `acceptedAnswer` | Keeps the exact question text and admitted rich answer body, without changing the containing page type or its listing identity. |

Compiler callers select this public view with `page.json_ld().public()` or by applying `.public()` to a discovery/listing JSON-LD selection. Generated listing views provide `ListingPage::public_json_ld()`. These borrowed serializers retain the caller's output limits. The application uses them for templates and the site graph.

The existing `page.json_ld()` remains a native inspection view, including Gazette's technical namespace and complete relationship origins, evidence, provenance, project state, scoped taxonomy language, role order, primary flags and rendering coordinates. The complete asset inventory remains in the page inspection model. Public output omits those technical fields without deleting or changing the underlying facts. Source provenance and relationship evidence do not become `isBasedOn` or `citation`; availability does not become an `Offer`. A generic project's repository and licence remain native metadata and visible resource links, rather than falsely typing the project as software or applying CreativeWork properties to an unknown kind of thing.

Journey and listing [ItemList](https://schema.org/ItemList) entries use explicit positions and counts, so array ordering is not their only record of editorial order.

[`creativeWorkStatus`](https://schema.org/creativeWorkStatus) carries textual work status. [Publication](https://schema.org/datePublished) and [modification](https://schema.org/dateModified) dates can be Date or DateTime values. Gazette keeps that distinction through [typed values](https://www.w3.org/TR/json-ld11/#typed-values); it does not turn a date into a midnight timestamp. The accepted inputs and missing-value rules are in [authored chronology](content-model.md#authored-description-and-chronology).

## Explicit project subjects

Declare `project.kind` on the [owning landing or inline catalog record](content-model.md#keep-project-facts-on-their-landing). The finite mapping is `software-application` → [SoftwareApplication](https://schema.org/SoftwareApplication), `software-source-code` → [SoftwareSourceCode](https://schema.org/SoftwareSourceCode), `creative-work` → [CreativeWork](https://schema.org/CreativeWork), `collection` → [Collection](https://schema.org/Collection), `project` → [Project](https://schema.org/Project), and `research-project` → [ResearchProject](https://schema.org/ResearchProject). Project and ResearchProject describe organizational undertakings, not their papers, applications or source code. A collection is distinct from the CollectionPage presenting it. Omission emits Thing; filenames, repositories, page families and themes never supply a project kind.

The optional `project.identity` is a public HTTP(S) entity IRI. It preserves an explicit fragment, rejects credentials and local/private endpoints, and must not collide with another project or a routed work/page. Without it, the declared landing supplies its canonical public address plus `#entity`. Without either anchor, the subject keeps a document-local identity and its declared type; neither its name nor a URL is invented. Kind, lifecycle, visibility and availability remain independent. Only an explicitly software-source-code subject emits its repository as `codeRepository`; an application is not silently made identical to its source repository.

[subjectOf](https://schema.org/subjectOf) means a work about the subject. Internal learning and article-index links are resolved against the captured graph and remain shallow, deduplicated references. Mentions-only targets keep their visible navigation links without becoming subjectOf. A configured code-documentation URL that exactly matches a validated provider page for the same project becomes a shallow WebPage reference with its public URL and available provider title. The page's physical or virtual storage form does not imply a more specific semantic type. Preview navigation keeps its local address separately. Unknown destinations retain bare IRI references; titles are never inferred from a route or project name, and incomplete provider catalogs are rejected rather than used to supply partial project metadata. Cycles do not recursively expand pages. The native inspection graph keeps its own identity scheme and relationship evidence.

## Rendering, scope and failure

[Breadcrumbs](https://schema.org/breadcrumb) belong to a `WebPage`. An article therefore links to its presentation page through [mainEntityOfPage](https://schema.org/mainEntityOfPage), and that page carries the breadcrumbs under its canonical public identity. Other page families and containers carry `breadcrumb` directly. Breadcrumb and neighbour references use the same distinction throughout the graph.

For a syndicated article, `mainEntityOfPage` instead identifies the original page and its canonical URL. The copy keeps its own serving URL and publisher; its breadcrumbs, publisher and attributions are not copied onto the original's page record. See [syndicated bodies](routing.md#syndicated-bodies).

Each `BreadcrumbList` keeps the path's endpoints, names and configured URLs, with explicit integer positions. The native view additionally identifies the direct container without choosing a primary path. An endpoint without a route keeps its local identity but has no URL. Collection, child, editorial-related and journey-member records likewise keep their resolved serving URLs. Ranked recommendations remain suggestions rather than becoming authored relationships or Schema.org subject claims.

The site file's `@graph` contains the discovery/listing values for the selected site's discoverable authored pages, followed by its generated taxonomy and listing pages. Authored pages are ordered by their resolved serving URLs. Aliases and linked foreign pages do not gain their own top-level records, though related records can appear inside a local document. Check the selected content and disclosure policy separately from this local discovery filtering. Turning JSON-LD off removes both the page field and `site.jsonld`; search, sitemap and `llms.txt` have their own settings.

Page-context, whole-file and inventory limits still apply. A missing required value, a collision or an exhausted limit fails the render rather than returning a truncated graph or substitute JSON. Building these outputs adds no wall-clock value, native path or mutable remote observation to artifact identity.
