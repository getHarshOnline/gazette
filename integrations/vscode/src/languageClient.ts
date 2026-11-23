import * as vscode from 'vscode';
import { spawn, type ChildProcess } from 'node:child_process';
import { SOURCE_COUNT } from './settings';
import { DidChangeTextDocumentNotification, LanguageClient, PublishDiagnosticsNotification, TextDocumentSyncKind, type Executable, type InitializeParams, type LanguageClientOptions, type PublishDiagnosticsParams } from 'vscode-languageclient/node';

interface ChildLifetime { child?: ChildProcess; closed?: Promise<void> }

/** The public server factory keeps child custody independent of provider state.
 * A close event proves process exit AND closure of its inherited stdio handles. */
function launch(server: Executable, lifetime: ChildLifetime): Promise<ChildProcess> {
  if (lifetime.child) return Promise.reject(new Error('Gazette child is already held.'));
  const child = spawn(server.command, server.args ?? [], {
    ...server.options, shell: false, detached: false, stdio: ['pipe', 'pipe', 'pipe'],
  });
  lifetime.child = child;
  lifetime.closed = new Promise(resolve => child.once('close', () => resolve()));
  return new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('spawn', () => resolve(child));
  });
}

async function closed(lifetime: ChildLifetime, timeout: number): Promise<void> {
  if (!lifetime.closed) throw new Error('Gazette child closure has no retained evidence.');
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([lifetime.closed, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error('Gazette child closure remains unproven.')), timeout);
    })]);
  } finally { if (timer !== undefined) clearTimeout(timer); }
}

interface Arrival {
  uri: vscode.Uri;
  params: PublishDiagnosticsParams;
  document: vscode.TextDocument | undefined;
  version: number | undefined;
  open: () => vscode.TextDocument | undefined;
}

/** Version-pinned bridge: 9.0.1 drops publishDiagnostics.version in its private
 * handler before middleware. Preserve transport identity through conversion,
 * using public notification registration/converters/collection; no private override
 * or native diagnostic interpretation. Registered before start, the pinned client
 * installs this handler after its builtin diagnostics handler. */
export class SelectedLanguageClient extends LanguageClient {
  private readonly ownsSource: (uri: vscode.Uri) => boolean;
  private readonly diagnosticArrivals: Map<string, Arrival>;
  private readonly diagnosticConversions: Set<string>;
  private readonly pendingSelection = new Map<string, Arrival>();
  private acceptResponses: boolean;
  private readonly output: vscode.OutputChannel;
  private readonly selectedRoots: readonly vscode.WorkspaceFolder[];
  private readonly childLifetime: ChildLifetime;
  private initialized: boolean;
  private changeDelivery: Promise<void> | undefined;
  private changeFailure: unknown;
  private symbolGeneration: number | undefined;

  constructor(id: string, name: string, server: Executable, options: LanguageClientOptions & { outputChannel: vscode.OutputChannel }, owns: (uri: vscode.Uri) => boolean, roots: readonly vscode.WorkspaceFolder[]) {
    const lifetime: ChildLifetime = {};
    super(id, name, () => launch(server, lifetime), options);
    this.childLifetime = lifetime;
    this.initialized = false;
    this.ownsSource = owns;
    this.diagnosticArrivals = new Map();
    this.diagnosticConversions = new Set();
    this.acceptResponses = true;
    this.output = options.outputChannel;
    this.selectedRoots = roots;
    this.onNotification(PublishDiagnosticsNotification.type, params => this.receiveDiagnostics(params));
  }

  retire() { this.acceptResponses = false; this.diagnosticArrivals.clear(); this.pendingSelection.clear(); }

  /** Native diagnostics may precede the manifest that confirms their membership.
   * Keep their original open-buffer identity; never replay into a reopened file. */
  refreshSelection(): void {
    if (!this.acceptResponses) return;
    for (const [id, arrival] of this.diagnosticArrivals) if (!this.ownsSource(arrival.uri)) {
      this.diagnosticArrivals.delete(id);
      this.diagnostics?.delete(arrival.uri);
    }
    const pending = [...this.pendingSelection.values()];
    this.pendingSelection.clear();
    for (const arrival of pending) {
      if (this.ownsSource(arrival.uri) && arrival.open() === arrival.document
          && (!arrival.document || arrival.document.version === arrival.version)) {
        this.receiveDiagnostics(arrival.params);
      }
    }
  }

  /** VS Code caches outlines by document version, but native symbols also depend
   * on configuration. Renew the pinned client's public feature registration when
   * that context changes; keep one native provider and the same server process. */
  refreshSymbols(generation: number): void {
    if (!this.acceptResponses || !this.initialized || this.symbolGeneration === generation
        || this.clientOptions.initializationOptions?.native_selection?.discover !== true) return;
    const documentSelector = this.clientOptions.documentSelector?.map(selector =>
      typeof selector === 'string' ? {language: selector} : selector);
    if (!documentSelector) return;
    const feature = this.getFeature('textDocument/documentSymbol');
    feature.clear();
    feature.register({id: 'gazette-native-symbols', registerOptions: {documentSelector}});
    this.symbolGeneration = generation;
  }

  override async start(): Promise<void> {
    await super.start();
    this.initialized = true;
  }

  /** Pinned 9.0.1 refuses stop while initialize is Starting/StartFailed. That
   * refusal is recoverable only for a never-initialized child whose close is
   * observed. Running shutdown failures and unclosed children keep custody. */
  override async stop(timeout = 2000): Promise<void> {
    // Accepted full replacements must reach the writer before protocol shutdown.
    // A failed delivery remains a failure even if subsequent child cleanup succeeds.
    let deliveryTimer: ReturnType<typeof setTimeout> | undefined;
    try {
      if (this.changeDelivery) await Promise.race([this.changeDelivery, new Promise<never>((_, reject) => {
        deliveryTimer = setTimeout(() => reject(new Error('Gazette full-change delivery remains unproven.')), timeout);
      })]);
    } catch (error) { this.changeFailure ??= error; }
    finally { if (deliveryTimer !== undefined) clearTimeout(deliveryTimer); }
    let failure: unknown;
    try { await super.stop(timeout); } catch (error) { failure = error; }
    if (this.childLifetime.child) {
      // Initialize refusal has no running protocol shutdown. Close ONLY this
      // never-initialized child's owned input, allowing native EOF teardown;
      // neither EOF nor exit alone replaces the subsequent close proof.
      if (failure && !this.initialized) this.childLifetime.child.stdin?.end();
      try { await closed(this.childLifetime, timeout); }
      catch (error) { throw failure ? new AggregateError([failure, error], 'Gazette child cleanup remains unproven.') : error; }
    } else if (failure) { throw failure; }
    if (failure && this.initialized) throw failure;
    if (failure) this.diagnostics?.dispose();
    if (this.changeFailure) throw this.changeFailure;
  }

  /** Native Full conversion snapshots URI, version and complete source now.
   * In pinned 9.0.1 Full middleware's next() only queues a mutable document and
   * schedules a Delayer which can retrigger after stop. Public sendNotification
   * preserves initialization/open ordering and the native writer semaphore,
   * without that pending map, a second serializer or private provider state. */
  fullChange(event: vscode.TextDocumentChangeEvent): Promise<void> {
    if (!this.acceptResponses || !this.ownsSource(event.document.uri)) return Promise.resolve();
    if (this.changeFailure) return Promise.reject(this.changeFailure);
    const sync = this.initializeResult?.capabilities.textDocumentSync;
    if ((typeof sync === 'number' ? sync : sync?.change) !== TextDocumentSyncKind.Full) {
      return Promise.reject(new Error('Gazette requires native full-document synchronization.'));
    }
    const params = this.code2ProtocolConverter.asChangeTextDocumentParams(event.document);
    const delivery = this.sendNotification(DidChangeTextDocumentNotification.type, params);
    this.changeDelivery = delivery.catch(error => { this.changeFailure = error; throw error; });
    return this.changeDelivery;
  }

  /** The pinned folder client otherwise sends only its anchor. Use the documented
   * initialization hook to project exact captured roots, never ambient folders. */
  protected override fillInitializeParams(params: InitializeParams): void {
    super.fillInitializeParams(params);
    params.workspaceFolders = this.selectedRoots.map(folder => ({
      uri: this.code2ProtocolConverter.asUri(folder.uri), name: folder.name,
    }));
  }

  /** Bind queries to the complete selected open-buffer set across async transport.
   * A sibling edit can move a target or change meaning without changing the caller. */
  async navigation<T>(document: vscode.TextDocument, token: vscode.CancellationToken, forward: () => vscode.ProviderResult<T>): Promise<T | null | undefined> {
    const uri = document.uri.toString();
    const version = document.version;
    const selected = () => this.acceptResponses && !token.isCancellationRequested
      && this.ownsSource(document.uri) && !document.isClosed
      && vscode.workspace.textDocuments.find(value => !value.isClosed && value.uri.toString() === uri) === document
      && document.version === version;
    if (!selected()) return null;
    const open = () => vscode.workspace.textDocuments.filter(value => !value.isClosed && this.ownsSource(value.uri));
    const captured = new Map(open().map(value => [value, { version: value.version, text: value.getText() }]));
    const current = () => {
      if (!selected()) return false;
      const documents = open();
      return documents.length === captured.size && documents.every(value => {
        const prior = captured.get(value);
        return prior !== undefined && value.version === prior.version && value.getText() === prior.text;
      });
    };
    try {
      if (this.changeDelivery) await this.changeDelivery;
      if (this.changeFailure) throw this.changeFailure;
      if (!current()) return null;
      const result = await forward();
      return current() ? result : null;
    } catch (error) {
      if (current()) throw error;
      return null;
    }
  }

  private receiveDiagnostics(params: PublishDiagnosticsParams): void {
    try {
      if (!params || typeof params.uri !== 'string' || !Array.isArray(params.diagnostics)) return;
      const uri = this.protocol2CodeConverter.asUri(params.uri);
      if (!this.acceptResponses || !this.diagnostics) return;
      const open = () => vscode.workspace.textDocuments.find(document => !document.isClosed && document.uri.toString() === params.uri);
      const document = open();
      const version = document?.version;
      if (document ? params.version !== version : params.version !== undefined) return;
      const arrival = { uri, params, document, version, open };
      if (!this.ownsSource(uri)) {
        if (this.clientOptions.initializationOptions?.native_selection?.discover === true
            && (this.pendingSelection.has(params.uri) || this.pendingSelection.size < SOURCE_COUNT)) {
          this.pendingSelection.set(params.uri, arrival);
        }
        return;
      }
      this.pendingSelection.delete(params.uri);
      this.diagnosticArrivals.set(params.uri, arrival);
      if (!this.diagnosticConversions.has(params.uri)) this.convertDiagnostics(arrival);
    } catch {
      this.output.appendLine('Gazette diagnostic transport input was rejected.');
    }
  }

  private convertDiagnostics(arrival: Arrival): void {
    const { uri, params, document, version, open } = arrival;
    this.diagnosticConversions.add(params.uri);
    const current = () => this.acceptResponses && this.ownsSource(uri)
      && this.diagnosticArrivals.get(params.uri) === arrival && open() === document
      && (!document || document.version === version);
    // At most one conversion and one coalesced latest packet per selected URI.
    Promise.resolve().then(() => this.protocol2CodeConverter.asDiagnostics(params.diagnostics)).then(diagnostics => {
        if (!current()) return;
        const forward = (target: vscode.Uri, values: vscode.Diagnostic[]): void => {
          if (target.toString() === params.uri && current()) this.diagnostics?.set(target, values);
        };
        const middleware = this.clientOptions.middleware?.handleDiagnostics;
        if (middleware) middleware(uri, diagnostics, forward);
        else forward(uri, diagnostics);
    }).catch(() => this.output.appendLine('Gazette diagnostic transport conversion failed.'))
      .finally(() => {
        this.diagnosticConversions.delete(params.uri);
        const next = this.diagnosticArrivals.get(params.uri);
        if (this.acceptResponses && next && next !== arrival) this.convertDiagnostics(next);
      });
  }
}
