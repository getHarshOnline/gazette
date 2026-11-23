// Compile by package name through NodeNext exports, then execute this emitted ESM.
import { loadGazette, type Gazette, type NativeJson, type ProjectionLimits, type MarkdownOptions, type DocumentOptions, type RenderOptions, type PrepareOptions, type QualificationOptions, type DeliveryOptions, type HistoryOptions, type TreeOptions, type PublicationFiles, type DocumentationOptions, type DocumentationFiles, type ReviewOptions, type PagePackageRequest, type PagePackageOptions } from '@rtepl/gazette';

interface Queries {
  field: string;
  inherited: string;
  overridden: string;
  disabled: string;
}

interface Results {
  check: NativeJson;
  field: NativeJson;
  inherited: NativeJson;
  overridden: NativeJson;
  disabled: NativeJson;
  invalid: NativeJson;
  markdown: NativeJson;
  markdownHtml: NativeJson;
  document: NativeJson;
  documentHtml: NativeJson;
}

export function consume(addon: string, source: Uint8Array, invalid: Uint8Array, queries: Queries, markdown: Uint8Array, document: Uint8Array): Results {
  const api: Gazette = loadGazette(addon);
  const limits = { sourceBytes: source.byteLength, queryBytes: 16_384 } satisfies ProjectionLimits;
  const check: NativeJson = api.checkWorkspace(source, limits);
  const markdownOptions = { html:true, limits:{sourceBytes:markdown.byteLength} } satisfies MarkdownOptions;
  const documentOptions = { html:true, sourceBytes:document.byteLength, metadata:{nodes:1000}, markdown:{nodes:1000} } satisfies DocumentOptions;
  let failure: NativeJson;
  try {
    api.checkWorkspace(invalid);
    throw new Error('Invalid workspace unexpectedly succeeded');
  } catch (error: unknown) {
    if (!(error instanceof Error)) throw error;
    failure = error.message;
  }
  return {
    check,
    field: api.fieldOrigin(source, queries.field, limits),
    inherited: api.compositionOrigin(source, queries.inherited),
    overridden: api.compositionOrigin(source, queries.overridden),
    disabled: api.compositionOrigin(source, queries.disabled),
    invalid: failure,
    markdown: api.inspectMarkdown(markdown),
    markdownHtml: api.inspectMarkdown(markdown, markdownOptions),
    document: api.inspectDocument(document),
    documentHtml: api.inspectDocument(document, documentOptions),
  };
}

export function consumePublication(addon: string, recipe: Uint8Array, options: RenderOptions): NativeJson {
  return loadGazette(addon).renderSites(recipe, options);
}

export function consumePreparation(addon: string, recipe: Uint8Array, options: PrepareOptions): NativeJson {
  return loadGazette(addon).prepareSites(recipe, options);
}

export function consumeQualification(addon: string, recipe: Uint8Array, options: QualificationOptions): NativeJson {
  return loadGazette(addon).qualifySites(recipe, options);
}

export function consumeDelivery(addon: string, request: Uint8Array, options?: DeliveryOptions): NativeJson {
  return loadGazette(addon).observeDelivery(request, options);
}

export function consumeHistory(addon: string, request: Uint8Array, options: HistoryOptions): NativeJson {
  return loadGazette(addon).inspectReleaseHistory(request, options);
}

export function consumeReview(addon: string, source: Uint8Array, site: string, options: ReviewOptions): NativeJson {
  return loadGazette(addon).reviewPublication(source, site, options);
}

export function consumePublicationFiles(addon: string, recipe: Uint8Array, options: RenderOptions): PublicationFiles {
  const result = loadGazette(addon).renderSiteFiles(recipe, options);
  const report: NativeJson = result.report;
  for (const site of result.sites) {
    const identity: string = site.site;
    for (const file of site.files) {
      const path: string = file.path;
      const bytes: Uint8Array = file.bytes;
      void path;
      void bytes;
    }
    void identity;
  }
  void report;
  return result;
}

export function consumeTree(addon: string, selection: NativeJson, permission: TreeOptions, framed: boolean): NativeJson {
  const api = loadGazette(addon);
  return framed ? api.inspectDocumentTree(selection, permission, {html:true}) : api.inspectMarkdownTree(selection, permission, {html:true});
}

export function consumePagePackage(addon: string, request: PagePackageRequest, options?: PagePackageOptions): NativeJson {
  return loadGazette(addon).inspectPagePackage(request, options);
}

export function consumeDocumentation(addon: string, source: Uint8Array, site: string, options: DocumentationOptions): DocumentationFiles {
  const result = loadGazette(addon).assembleDocumentation(source, site, options);
  const report: NativeJson = result.report;
  for (const file of result.files) {
    const path: string = file.path;
    const bytes: Uint8Array = file.bytes;
    void path; void bytes;
  }
  void report;
  return result;
}

// These are compile-time contract probes, never calls to a replacement runtime.
// Unused @ts-expect-error is itself a compilation failure if types degrade to any.
export function rejectedTypes(api: Gazette, source: Uint8Array): void {
  // @ts-expect-error Page-package inputs require complete retained bytes, not source text.
  api.inspectPagePackage({workspace:{path:'workspace.toml',bytes:'source'},documents:[],companions:[],query:'page.md'});
  // @ts-expect-error The query is a logical path string, not another inferred source object.
  api.inspectPagePackage({workspace:{path:'workspace.toml',bytes:source},documents:[],companions:[],query:{path:'page.md'}});
  // @ts-expect-error Native source ceilings are checked numeric values.
  api.inspectPagePackage({workspace:{path:'workspace.toml',bytes:source},documents:[],companions:[],query:'page.md'}, {sourceBytes:'1024'});
  // @ts-expect-error The loader freezes the page-package projection.
  api.inspectPagePackage = () => 'null';
  // @ts-expect-error Native database reads require explicit permission.
  api.inspectReleaseHistory(source, {});
  // @ts-expect-error Preserve original JSON bytes, not a second decoded model.
  api.inspectReleaseHistory('{}', {allowRead:true});
  // @ts-expect-error Native coordinates remain serialized without JS numeric conversion.
  const historyReport: object = api.inspectReleaseHistory(source, {allowRead:true});
  // @ts-expect-error The loader freezes the operation.
  api.inspectReleaseHistory = () => '{}';
  void historyReport;
  // @ts-expect-error Delivery uses exact native request bytes, not a text approximation.
  api.observeDelivery('{}');
  // @ts-expect-error A decoded object is not the byte-preserving native input contract.
  api.observeDelivery({tree:'/output'});
  // @ts-expect-error Bounds are checked numeric values, not strings.
  api.observeDelivery(source, {resultBytes:'1000'});
  // @ts-expect-error Permission belongs to request.options.http, not a second host model.
  api.observeDelivery(source, {allowNetwork:true});
  // @ts-expect-error Complete native report numbers remain serialized JSON.
  const deliveryReport: object = api.observeDelivery(source);
  // @ts-expect-error The loader freezes this operation.
  api.observeDelivery = () => '{}';
  void deliveryReport;
  // @ts-expect-error Exact recipe bytes, not a second JavaScript model.
  api.qualifySites('{}', {allowRead:true});
  // @ts-expect-error Native reads require an explicit permission field.
  api.qualifySites(source, {allowPolicyExecution:true});
  // @ts-expect-error Checker permission is a boolean, not truthy text.
  api.qualifySites(source, {allowRead:true,allowPolicyExecution:'true'});
  // @ts-expect-error Qualification cannot acquire staging authority.
  api.qualifySites(source, {allowRead:true,createStage:true});
  // @ts-expect-error Reports cross as native JSON, not decoded lossy host objects.
  const qualificationReport: object = api.qualifySites(source, {allowRead:true});
  // @ts-expect-error The loader freezes the public operation.
  api.qualifySites = () => '{}';
  void qualificationReport;
  // @ts-expect-error Preparation requires explicit local/read permissions.
  api.prepareSites(source);
  // @ts-expect-error Effect flags are booleans, not truthy strings.
  api.prepareSites(source, {local:true,allowRead:true,createStage:'true'});
  // @ts-expect-error Native read options remain exact serialized JSON.
  api.prepareSites(source, {local:true,allowRead:true,limitsJson:{read:{allow_coordination_files:true}}});
  // @ts-expect-error Preparation cannot grant publication permission.
  api.prepareSites(source, {local:true,allowRead:true,publish:true});
  // @ts-expect-error Preparation follows the frozen API contract.
  api.prepareSites = () => '{}';
  // @ts-expect-error Private review requires explicit source-read permission.
  api.reviewPublication(source, 'site:one');
  // @ts-expect-error Private review does not authorize execution.
  api.reviewPublication(source, 'site:one', {allowRead:true,allowExecute:true});
  // @ts-expect-error Native limit schemas remain exact serialized JSON.
  api.reviewPublication(source, 'site:one', {allowRead:true,limitsJson:{review:{uses:1}}});
  // @ts-expect-error Review follows the frozen API contract.
  api.reviewPublication = () => '{}';
  // @ts-expect-error Native source is bytes, not a JS string.
  api.checkWorkspace('workspace text');
  // @ts-expect-error A raw array is not a typed source byte buffer.
  api.checkWorkspace([1, 2]);
  // @ts-expect-error Queries are exact serialized native JSON, not another JS schema.
  api.fieldOrigin(source, { kind: 'site', identity: 'site:one', field: ['id'] });
  // @ts-expect-error Native numeric output remains serialized, not a JS object.
  const model: { source_sha256: string } = api.checkWorkspace(source);
  // @ts-expect-error Host ceiling values are numbers, checked for safe integers natively.
  api.checkWorkspace(source, { sourceBytes: '1000' });
  // @ts-expect-error Unknown limit keys are not a supported public option.
  api.checkWorkspace(source, { source_bytes: 1000 });
  // @ts-expect-error The loader freezes this method at runtime.
  api.checkWorkspace = () => '{}';
  // @ts-expect-error Source acquisition is not part of this SDK slice.
  api.acquireSite(source);
  // @ts-expect-error Native loading requires an explicit path string.
  loadGazette(42);
  // @ts-expect-error Markdown sources remain exact byte arrays.
  api.inspectMarkdown('# Text');
  // @ts-expect-error HTML selection is an explicit boolean.
  api.inspectMarkdown(source, {html:'true'});
  // @ts-expect-error Documents have separate typed metadata ceilings.
  api.inspectDocument(source, {metadata:{nodes:'10'}});
  // @ts-expect-error Body limits are named objects, not a numeric shorthand.
  api.inspectDocument(source, {markdown:42});
  // @ts-expect-error Document framing is not a guessed transport option.
  api.inspectMarkdown(source, {frontMatter:'toml'});
  // @ts-expect-error Native rendering requires both explicit permission fields.
  api.renderSites(source);
  // @ts-expect-error Read permission is not inferred from local mode.
  api.renderSites(source, {local:true});
  // @ts-expect-error Native limit structures cross as exact JSON, not a second JS schema.
  api.renderSites(source, {local:true,allowRead:true,limitsJson:{output:{entries:1}}});
  // @ts-expect-error The new operation remains readonly like all SDK methods.
  api.renderSites = () => '{}';
  // @ts-expect-error Returning file bytes does not infer permission to read sources.
  api.renderSiteFiles(source);
  // @ts-expect-error Local intent alone does not grant native reads.
  api.renderSiteFiles(source, {local:true});
  // @ts-expect-error Read permission alone does not select private local rendering.
  api.renderSiteFiles(source, {allowRead:true});
  // @ts-expect-error Recipe input remains bytes, not decoded text.
  api.renderSiteFiles('{}', {local:true,allowRead:true});
  // @ts-expect-error The loader freezes the method, not its returned owned buffers.
  api.renderSiteFiles = () => ({report:'{}',sites:[]});
  const publication = api.renderSiteFiles(source, {local:true,allowRead:true});
  // @ts-expect-error Native report numbers remain in exact serialized JSON.
  const report: object = publication.report;
  for (const site of publication.sites) {
    for (const file of site.files) {
      // @ts-expect-error File content is bytes, not a decoded or base64 string.
      const text: string = file.bytes;
      void text;
    }
  }
  // @ts-expect-error A pin never implies permission to open its source.
  api.inspectMarkdownTree('{}');
  // @ts-expect-error Native Tree fields are exact JSON, not a parallel JS model.
  api.inspectMarkdownTree({root:'/source'}, {allowRead:true});
  // @ts-expect-error Permission is a boolean, not truthy text.
  api.inspectDocumentTree('{}', {allowRead:'true'});
  // @ts-expect-error Parser and tree-query limits are independent arguments.
  api.inspectDocumentTree('{}', {allowRead:true, sourceBytes:100});
  // @ts-expect-error Native tree entrypoints are frozen like supplied-source methods.
  api.inspectMarkdownTree = () => '{}';
  // @ts-expect-error Complete workspace input remains bytes.
  api.assembleDocumentation('source', 'site:one', {local:true,allowRead:true});
  // @ts-expect-error Site identity is not a second JavaScript selection model.
  api.assembleDocumentation(source, {site:'site:one'}, {local:true,allowRead:true});
  // @ts-expect-error Permissions are mandatory.
  api.assembleDocumentation(source, 'site:one');
  // @ts-expect-error Local intent alone does not grant native reads.
  api.assembleDocumentation(source, 'site:one', {local:true});
  // @ts-expect-error Execution permission belongs to the explicitly selected generation request.
  api.assembleDocumentation(source, 'site:one', {local:true,allowRead:true,allowExecute:true});
  api.assembleDocumentation(source, 'site:one', {local:true,allowRead:true,allowStage:true,
    generationJson:'{"parent":"/tmp","max_jobs":2,"directory_bytes":256,"entry_bytes":64,"allow_execute":true}'});
  // @ts-expect-error Generation uses exact named native JSON, not a second host object schema.
  api.assembleDocumentation(source, 'site:one', {local:true,allowRead:true,generationJson:{parent:'/tmp'}});
  // @ts-expect-error Native limits remain exact serialized JSON.
  api.assembleDocumentation(source, 'site:one', {local:true,allowRead:true,limitsJson:{output:{entries:1}}});
  // @ts-expect-error The public function is frozen.
  api.assembleDocumentation = () => ({report:'{}',files:[]});
  const documentation = api.assembleDocumentation(source, 'site:one', {local:true,allowRead:true});
  // @ts-expect-error Native report integers are not converted into JavaScript objects.
  const documentationReport: object = documentation.report;
  for (const file of documentation.files) {
    // @ts-expect-error Complete output bytes are not decoded strings.
    const documentationText: string = file.bytes;
    void documentationText;
  }
  void documentationReport;
  void model;
  void report;
}
