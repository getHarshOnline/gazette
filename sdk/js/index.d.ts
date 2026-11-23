/** Exact serialized native data. Parse with a lossless JSON reader where numbers may exceed JS precision. */
export type NativeJson = string;

/** Optional host ceilings. Values must be nonnegative safe integers. */
export interface ProjectionLimits {
  sourceBytes?: number;
  nodes?: number;
  depth?: number;
  queryBytes?: number;
  resultBytes?: number;
}

/** Native Markdown ceilings. Omitted fields retain Rust defaults; safe integers only. */
export interface MarkdownLimits {
  sourceBytes?: number;
  nodes?: number;
  depth?: number;
  factBytes?: number;
  outputBytes?: number;
}

/** Native TOML metadata ceilings, independent of body and complete-document bounds. */
export interface MetadataLimits {
  sourceBytes?: number;
  nodes?: number;
  depth?: number;
}

/** Body-only Markdown; include HTML explicitly, with no partial report on failure. */
export interface MarkdownOptions {
  html?: boolean;
  limits?: MarkdownLimits;
  resultBytes?: number;
}

/** Explicit +++ TOML framing, never inferred from a name or Markdown body. */
export interface DocumentOptions {
  html?: boolean;
  sourceBytes?: number;
  metadata?: MetadataLimits;
  markdown?: MarkdownLimits;
  resultBytes?: number;
}

/** Explicit complete-tree reads; semantic selection and capture limits use the Rust Tree JSON. */
export interface TreeOptions {
  /** Explicit permission for complete native capture. A valid pin does not imply permission. */
  allowRead: boolean;
  /** Maximum exact native Tree JSON bytes before decoding; default 16 KiB. */
  queryBytes?: number;
}

/** One exact logical compiler path and complete caller-supplied bytes. No path is opened. */
export interface PagePackageSource {
  path: string;
  bytes: Uint8Array;
}

/** Complete explicit selection for the shared compiler-owned page-package resolver. */
export interface PagePackageRequest {
  workspace: PagePackageSource;
  documents: PagePackageSource[];
  companions: PagePackageSource[];
  query: string;
}

/** Host/session ceilings; omitted values retain the Rust SDK defaults. */
export interface PagePackageOptions {
  selectionEntries?: number;
  identifierBytes?: number;
  sourceBytes?: number;
  resultBytes?: number;
}

/** Private local rendering through configured sources, never admission or release. */
export interface RenderOptions {
  local: boolean;
  allowRead: boolean;
  inputBytes?: number;
  resultBytes?: number;
  limitsBytes?: number;
  /** Exact JSON of native syntax/acquisition/output and aggregate ceilings. See the SDK README. */
  limitsJson?: NativeJson;
}

/** Host transport ceilings only; permissions belong to the exact native delivery request. */
export interface DeliveryOptions {
  /** Complete original request JSON ceiling; default 1 MiB. */
  inputBytes?: number;
  /** Complete report or pre-reserved unavailable envelope ceiling; default 1 MiB. */
  resultBytes?: number;
}

/** Native history observation only; coordination-file permission is separately explicit in request.read. */
export interface HistoryOptions {
  allowRead: boolean;
  /** Exact request JSON ceiling; default 16 KiB. */
  inputBytes?: number;
  /** Complete report or reserved failure envelope ceiling; default 1 MiB. */
  resultBytes?: number;
}

/** Complete-set technical checks, not publisher approval or release preparation. */
export interface QualificationOptions {
  allowRead: boolean;
  /** Explicit permission for the selected unsandboxed native checker; default denied. */
  allowPolicyExecution?: boolean;
  /** Complete exact JSON input ceiling; default 4 MiB. */
  inputBytes?: number;
  /** Complete report or pre-reserved effect-aware failure ceiling; default 4 MiB. */
  resultBytes?: number;
}

/** Native local preparation. All omitted effect flags deny permission. */
export interface PrepareOptions {
  local: boolean;
  allowRead: boolean;
  createStage?: boolean;
  recordPreparation?: boolean;
  advanceWriterGeneration?: boolean;
  reserveLocalCoordination?: boolean;
  writeCoordinatorAnchor?: boolean;
  initializeWorktree?: boolean;
  allowPolicyExecution?: boolean;
  inputBytes?: number;
  resultBytes?: number;
  limitsBytes?: number;
  /** Native syntax/acquisition/output/read options; read.allow_coordination_files is separately explicit. */
  limitsJson?: NativeJson;
}

/** One generated regular file. Bytes are owned and mutable, not borrowed native state. */
export interface PublicationFile {
  path: string;
  bytes: Uint8Array;
}

/** Every regular file for one selected site, in native path order. */
export interface PublicationSiteFiles {
  site: string;
  files: PublicationFile[];
}

/** Complete private render. The exact native report retains directory entries and file digests. */
export interface PublicationFiles {
  report: NativeJson;
  sites: PublicationSiteFiles[];
}

/** Local reference assembly with optional explicitly permitted native generation; no deployment. */
export interface DocumentationOptions {
  local: boolean;
  allowRead: boolean;
  /** Permit fresh retained generator stages; absent means denied. */
  allowStage?: boolean;
  /** Exact named native generation request, including independently denied allow_execute. */
  generationJson?: NativeJson;
  /** Private public-base to loopback-base JSON map for manual preview links. */
  previewLinkBasesJson?: NativeJson;
  inputBytes?: number;
  resultBytes?: number;
  limitsBytes?: number;
  /** Exact JSON of native syntax/acquisition/output ceilings, not a second workspace schema. */
  limitsJson?: NativeJson;
}

/** All configured project modules for one site, with the native destination in report. */
export interface DocumentationFiles {
  report: NativeJson;
  files: PublicationFile[];
}

/** Complete private publication review; source reads only, never policy execution or release. */
export interface ReviewOptions {
  allowRead: boolean;
  inputBytes?: number;
  resultBytes?: number;
  limitsBytes?: number;
  /** Exact JSON of native syntax/acquisition/corpus/graph/review ceilings. */
  limitsJson?: NativeJson;
}

/** Frozen synchronous API. Native failures have JSON messages; host type errors may not. */
export interface Gazette {
  /** Exact native history JSON; request selects kind, database, record_id, optional expected_header and native read options. No release effects. */
  readonly inspectReleaseHistory: (request: Uint8Array, options: HistoryOptions) => NativeJson;
  /** Exact native delivery Report JSON; mismatches throw the complete selected-route report. No publication or settlement. */
  readonly observeDelivery: (request: Uint8Array, options?: DeliveryOptions) => NativeJson;
  /** Complete native qualification report; failures never return a reduced successful set. Lost delivery does not authorize retry. */
  readonly qualifySites: (recipe: Uint8Array, options: QualificationOptions) => NativeJson;
  /** Native complete report. Errors retain partial effects or original reconciliation coordinates; never retry blindly. */
  readonly prepareSites: (recipe: Uint8Array, options: PrepareOptions) => NativeJson;
  /** Native workspace Compilation JSON, not publication or native-source approval. */
  readonly checkWorkspace: (source: Uint8Array, limits?: ProjectionLimits) => NativeJson;
  /** queryJson is the Rust SDK FieldQuery; returned spans are half-open UTF-8 byte ranges. */
  readonly fieldOrigin: (source: Uint8Array, queryJson: string, limits?: ProjectionLimits) => NativeJson;
  /** queryJson is the Rust SDK CompositionOriginQuery, preserving the winning declaration and inheritance origin. */
  readonly compositionOrigin: (source: Uint8Array, queryJson: string, limits?: ProjectionLimits) => NativeJson;
  /** Exact application PagePackageOwnership JSON, or JSON null for a standalone page. Performs no native reads. */
  readonly inspectPagePackage: (request: PagePackageRequest, options?: PagePackageOptions) => NativeJson;
  /** Native {inspection, html?} JSON with exact UTF-8 source coordinates. */
  readonly inspectMarkdown: (source: Uint8Array, options?: MarkdownOptions) => NativeJson;
  /** Native {document, html?} JSON; body coordinates remain relative to document.body_span. */
  readonly inspectDocument: (source: Uint8Array, options?: DocumentOptions) => NativeJson;
  /** Exact Rust markdown::input::Tree JSON; returns native {input, inspection, html?}. */
  readonly inspectMarkdownTree: (treeJson: string, tree: TreeOptions, options?: MarkdownOptions) => NativeJson;
  /** Exact same complete-tree capture, with explicit +++ framing; returns {input, document, html?}. */
  readonly inspectDocumentTree: (treeJson: string, tree: TreeOptions, options?: DocumentOptions) => NativeJson;
  /** Complete native inspection; incomplete sets throw the same full report without dropping siblings. */
  readonly renderSites: (recipe: Uint8Array, options: RenderOptions) => NativeJson;
  /** Same complete render plus owned file bytes; no staging, CLI bridge or publication effect. */
  readonly renderSiteFiles: (recipe: Uint8Array, options: RenderOptions) => PublicationFiles;
  /** Complete selected manual/reference target and native report; configured generators require independent stage/execute permissions. */
  readonly assembleDocumentation: (workspace: Uint8Array, site: string, options: DocumentationOptions) => DocumentationFiles;
  /** Exact private native PublicationReview JSON; a digest is not approval or certification. */
  readonly reviewPublication: (workspace: Uint8Array, site: string, options: ReviewOptions) => NativeJson;
}

/** Explicit local native-code loading. No package installation, executable search or CLI bridge. */
export function loadGazette(addonPath: string): Gazette;
