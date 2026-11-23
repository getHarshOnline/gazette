import * as vscode from 'vscode';
import { CancellationToken, ErrorAction, CloseAction, RevealOutputChannelOn, TextDocumentSyncKind } from 'vscode-languageclient/node';
import { SelectedLanguageClient } from './languageClient';
import { serverOptions, sourcePath, documentSelector, literalGlob, SOURCE_COUNT } from './settings';
import { captureSources } from './sourceCapture';
import { type EditorSnapshot, captureEditorSnapshot } from './editorSnapshot';

/** One folder owns its client, coalesced restart and any uncertain cleanup. */
export class WorkspaceSession {
  readonly folder: vscode.WorkspaceFolder;
  readonly key: string;
  private readonly output: vscode.OutputChannel;
  private readonly custody: Map<string, string>;
  private readonly selectedRoots: Set<string>;
  private held: Set<string>;
  private client: SelectedLanguageClient | undefined;
  private transition: Promise<void> | undefined;
  private requested: boolean;
  private stopping: boolean;
  private blocked: boolean;
  private shutdownPromise: Promise<void> | undefined;
  private problem: string | undefined;
  private notifiedProblem: string | undefined;
  private configurationUris: ReadonlySet<string> = new Set();
  private readonly sourceWatchers = new Map<string, vscode.Disposable>();
  private textSources: ReadonlySet<string> = new Set();
  private readonly changedWhileOpen = new Set<string>();
  private readonly closedDocuments: vscode.Disposable;
  private fileRefresh: ReturnType<typeof setTimeout> | undefined;
  private captureEpoch = 0;
  private admitted = false;
  private nativeRefresh: (() => Promise<void>) | undefined;
  private nativeWork: Promise<void> | undefined;
  private nativeAdmission: (() => Promise<void>) | undefined;

  constructor(folder: vscode.WorkspaceFolder, output: vscode.OutputChannel, custody: Map<string, string> = new Map(), private readonly changed: () => void = () => {}) {
    this.folder = folder;
    this.key = folder.uri.toString();
    this.output = output;
    this.custody = custody;
    this.selectedRoots = new Set([this.key]);
    this.held = new Set();
    this.client = undefined;
    this.transition = undefined;
    this.requested = false;
    this.stopping = false;
    this.blocked = false;
    this.shutdownPromise = undefined;
    this.closedDocuments = vscode.workspace.onDidCloseTextDocument(document => {
      const uri = document.uri.toString();
      if (this.changedWhileOpen.delete(uri) && this.watchesSource(uri)) this.sourceChanged();
    });
  }

  async stop() {
    this.captureEpoch++;
    this.nativeRefresh = undefined;
    this.nativeAdmission = undefined;
    if (this.client) {
      this.client.retire();
      await this.client.stop();
    }
    // Never discard custody when stop rejects.
    this.client = undefined;
    this.nativeWork = undefined;
    for (const uri of this.held) if (this.custody.get(uri) === this.key) this.custody.delete(uri);
    this.held.clear();
  }

  owns(uri: vscode.Uri): boolean { return this.client !== undefined && this.held.has(uri.toString()) && this.custody.get(uri.toString()) === this.key; }

  selects(uri: vscode.Uri): boolean { return this.sourceWatchers.has(uri.toString()); }

  /** Explicit recovery reuses the public stop path. A previous timeout is not
   * process-exit proof; stop must now independently observe child/stdio close
   * and successful delivery before any custody or blocked state is released. */
  async retry(): Promise<void> {
    try { await this.transition; } catch { /* Retained below; never auto-retry. */ }
    if (!this.blocked || this.stopping) return;
    await this.stop();
    this.blocked = false;
    this.problem = undefined;
    this.changed();
  }

  private currentClient(client: SelectedLanguageClient, epoch = this.captureEpoch): boolean {
    return this.client === client && this.captureEpoch === epoch && !this.transition && !this.requested && !this.stopping && !this.blocked && !this.nativeWork && this.fileRefresh === undefined;
  }

  async language(document: vscode.TextDocument): Promise<void> {
    if (this.owns(document.uri) && this.configurationUris.has(document.uri.toString()) && document.languageId !== 'gazette') {
      await vscode.languages.setTextDocumentLanguage(document, 'gazette');
    }
  }

  async context(document: vscode.TextDocument, position: vscode.Position): Promise<unknown> {
    const client = this.client;
    const epoch = this.captureEpoch;
    if (this.nativeAdmission) await this.nativeAdmission();
    if (!client || !this.currentClient(client, epoch) || !this.owns(document.uri)) throw new Error(this.problem ?? 'This file is not selected by an active Gazette composition.');
    return client.navigation(document, CancellationToken.None, async () => {
      const manifest = await client.sendRequest<{generation: number}>('gazette/editorSnapshot', {});
      return client.sendRequest('gazette/authoringContext', {generation: manifest.generation,
        textDocument: {uri: document.uri.toString()}, position: {line: position.line, character: position.character}});
    });
  }

  async snapshot(document: vscode.TextDocument, progress?: (completed: number, total: number) => void): Promise<EditorSnapshot> {
    // Initial protocol admission still fails closed. A user export from an
    // already admitted session may wait for its exact source recapture.
    if (this.admitted) await this.settled();
    const client = this.client;
    const epoch = this.captureEpoch;
    if (!client || !this.currentClient(client) || !this.owns(document.uri)) throw new Error(this.problem ?? 'This file is not selected by an active Gazette composition.');
    const result = await client.navigation(document, CancellationToken.None, () => captureEditorSnapshot(client, progress, this.nativeRefresh !== undefined));
    if (!result) throw new Error('Editor content changed during capture; retry using the current content.');
    const current = () => this.currentClient(client, epoch);
    return {...result, current: async () => current() && await result.current() && current()};
  }

  async sourceUri(path: string): Promise<vscode.Uri> {
    const client = this.client;
    const epoch = this.captureEpoch;
    if (this.nativeAdmission) await this.nativeAdmission();
    if (!client || !this.currentClient(client)) throw new Error('This Gazette composition is no longer active.');
    const manifest = await client.sendRequest<{sources: {id: string; path: string}[]}>('gazette/editorSnapshot', {});
    if (!this.currentClient(client, epoch)) throw new Error('The Gazette composition changed before navigation.');
    const source = manifest.sources.find(source => source.path === path);
    if (!source) throw new Error('This source is not retained by the current composition.');
    return vscode.Uri.parse(source.id);
  }

  dependsOn(root: string): boolean { return this.selectedRoots.has(root); }

  private async settled(): Promise<void> {
    if (this.transition) await this.transition;
    else if (this.fileRefresh !== undefined && this.nativeRefresh) {
      clearTimeout(this.fileRefresh);
      this.fileRefresh = undefined;
      await this.nativeRefresh();
    } else if (this.fileRefresh !== undefined) await this.refresh();
    if (this.nativeWork) await this.nativeWork;
    if (this.nativeAdmission) await this.nativeAdmission();
  }

  private watchesSource(identity: string): boolean {
    return !this.stopping && this.sourceWatchers.has(identity)
      && (!this.custody.has(identity) || this.custody.get(identity) === this.key);
  }

  private watchSources(uris: Iterable<string>, text: readonly vscode.Uri[] = []): void {
    const selected = new Set(uris);
    this.textSources = new Set(text.map(uri => uri.toString()));
    for (const [uri, watcher] of this.sourceWatchers) if (!selected.has(uri)) {
      watcher.dispose();
      this.sourceWatchers.delete(uri);
      this.changedWhileOpen.delete(uri);
    }
    for (const identity of selected) {
      if (this.sourceWatchers.has(identity)) continue;
      const uri = vscode.Uri.parse(identity);
      // One literal filename under its exact parent, never a recursive glob.
      const filename = literalGlob(uri.path.slice(uri.path.lastIndexOf('/') + 1));
      const watcher = vscode.workspace.createFileSystemWatcher(new vscode.RelativePattern(vscode.Uri.joinPath(uri, '..'), filename));
      const changed = (event: vscode.Uri) => {
        if (event.toString() !== identity || !this.watchesSource(identity)) return;
        if (this.textSources.has(identity) && vscode.workspace.textDocuments.some(document => !document.isClosed && document.uri.toString() === identity)) {
          this.changedWhileOpen.add(identity);
          return;
        }
        this.sourceChanged();
      };
      const subscriptions = [watcher.onDidCreate(changed), watcher.onDidChange(changed), watcher.onDidDelete(changed)];
      this.sourceWatchers.set(identity, {dispose: () => { subscriptions.forEach(value => value.dispose()); watcher.dispose(); }});
    }
  }

  private sourceChanged(): void {
    if (this.stopping || this.blocked) return;
    if (this.nativeRefresh) {
      this.captureEpoch++;
      this.changed();
      if (this.fileRefresh !== undefined) return;
      this.fileRefresh = setTimeout(() => {
        this.fileRefresh = undefined;
        void this.nativeRefresh?.().catch(error => this.output.appendLine(`Gazette source refresh failed: ${String(error)}`));
      }, 50);
      return;
    }
    // Invalidate snapshots and in-flight providers immediately, even while the
    // short filesystem burst is being coalesced or initialization is pending.
    this.requested = true;
    this.client?.retire();
    this.changed();
    if (this.transition || this.fileRefresh !== undefined) return;
    this.fileRefresh = setTimeout(() => {
      this.fileRefresh = undefined;
      void this.refresh().catch(() => this.output.appendLine('Gazette selected-source refresh remains unproven.'));
    }, 50);
  }

  private startupProblem(error: unknown): void {
    const reason = error instanceof Error ? error.message : String(error);
    this.problem = `Gazette could not start in ${this.folder.name}: ${reason}`;
    this.output.appendLine(this.problem);
    if (this.stopping || this.notifiedProblem === this.problem) return;
    this.notifiedProblem = this.problem;
    // Dismissing a notification must not hold up startup or a later restart.
    void this.offerStartupRecovery(this.problem).catch(error => {
      this.output.appendLine(`Gazette setup could not be updated: ${String(error)}`);
    });
  }

  private async offerStartupRecovery(message: string): Promise<void> {
    const action = await vscode.window.showErrorMessage(message,
      'Choose Gazette Program', 'Open Settings', 'Show Output');
    if (this.stopping || !vscode.workspace.workspaceFolders?.includes(this.folder)) return;
    if (action === 'Show Output') this.output.show(true);
    else if (action === 'Open Settings') {
      await vscode.commands.executeCommand('workbench.action.openSettings', '@ext:rtepl.gazette-vscode');
    } else if (action === 'Choose Gazette Program') {
      const selected = await vscode.window.showOpenDialog({
        title: 'Choose the Gazette program', openLabel: 'Use Gazette',
        canSelectFiles: true, canSelectFolders: false, canSelectMany: false,
      });
      if (this.stopping || !vscode.workspace.isTrusted
          || !vscode.workspace.workspaceFolders?.includes(this.folder)) return;
      const program = selected?.[0];
      if (program?.scheme === 'file') {
        await vscode.workspace.getConfiguration('gazette', this.folder.uri)
          .update('server.path', program.fsPath, vscode.ConfigurationTarget.Global);
      }
    }
  }

  refresh(): Promise<void> {
    if (this.blocked) return Promise.reject(new Error('Gazette cleanup remains unproven.'));
    if (this.stopping) return Promise.resolve();
    if (this.fileRefresh !== undefined) { clearTimeout(this.fileRefresh); this.fileRefresh = undefined; }
    this.requested = true;
    this.client?.retire();
    this.changed();
    if (this.transition) return this.transition;
    this.transition = (async () => {
      while (this.requested && !this.stopping) {
        this.requested = false;
        await this.stop();
        if (this.stopping) break;
        if (!vscode.workspace.getConfiguration('gazette', this.folder.uri).get<boolean>('authoring.enabled', true)) {
          this.watchSources([]);
          this.selectedRoots.clear();
          this.selectedRoots.add(this.key);
          continue;
        }
        this.problem = undefined;
        let stage = 'configuration and explicit source selection';
        try {
          const captured = await captureSources(vscode, this.folder);
          if (this.stopping || this.requested) continue;
          if (!captured) {
            const path = sourcePath(vscode.workspace.getConfiguration('gazette', this.folder.uri).get('workspace.configuration', 'gazette.toml'));
            this.watchSources([vscode.Uri.joinPath(this.folder.uri, ...path.split('/')).toString()]);
            continue;
          }
          const server = serverOptions(vscode.workspace, this.folder);
          const workspaceUri = captured.uris[0];
          if (!workspaceUri) throw new Error('Gazette source selection is missing its workspace.');
          if ([...captured.owners.keys()].some(uri => this.custody.has(uri) && this.custody.get(uri) !== this.key)) {
            throw new Error('Gazette selected source is already held by another composition.');
          }
          for (const uri of captured.owners.keys()) { this.custody.set(uri, this.key); this.held.add(uri); }
          this.watchSources(captured.owners.keys(), captured.uris);
          this.selectedRoots.clear();
          for (const root of captured.rootUris) this.selectedRoots.add(root);
          this.configurationUris = captured.configurationUris;
          const selected = new Set(captured.uris.map(uri => uri.toString()));
          const owns = (uri: vscode.Uri): boolean => !this.stopping && !this.requested && !this.blocked && selected.has(uri.toString())
            && captured.rootUris.every(root => vscode.workspace.workspaceFolders?.some(folder => folder.uri.toString() === root))
            && (captured.discovery ? this.custody.get(uri.toString()) === this.key
              : vscode.workspace.getWorkspaceFolder(uri)?.uri.toString() === captured.owners.get(uri.toString()));
          let synchronization = Promise.resolve();
          let admissions = Promise.resolve();
          let admissionFailure: unknown;
          const admit = (operation: () => Promise<void>): Promise<void> => {
            const pending = admissions.then(operation);
            admissions = pending.catch(error => { admissionFailure = error; });
            return pending;
          };
          const currentOpen = (document: vscode.TextDocument) => !document.isClosed
            && vscode.workspace.textDocuments.find(value => !value.isClosed && value.uri.toString() === document.uri.toString()) === document;
          const admittedText = async (): Promise<void> => {
            for (let pass = 0; pass <= SOURCE_COUNT; pass++) {
              const opening = admissions;
              await opening;
              if (admissionFailure) throw admissionFailure;
              const selecting = synchronization;
              await selecting;
              if (opening === admissions && selecting === synchronization) return;
            }
            throw new Error('Gazette open buffers did not settle within the source limit.');
          };
          const synchronize = (opening?: vscode.TextDocument): Promise<void> => {
            const operation = synchronization.then(async () => {
              type SelectedSource = {id: string; role: {kind: string}; version: number | null};
              const configuration = (source: SelectedSource) => ['workspace', 'workspace_document'].includes(source.role.kind);
              const replay = async (document: vscode.TextDocument, source: SelectedSource): Promise<boolean> => {
                if (document === opening || document.isClosed || source.version === document.version) return false;
                if (source.version === null) {
                  await next.sendNotification('textDocument/didOpen', {textDocument: {uri: source.id, languageId: document.languageId, version: document.version, text: document.getText()}});
                  return true;
                }
                if (source.version < document.version) {
                  await next.fullChange({document} as vscode.TextDocumentChangeEvent);
                  return true;
                }
                return false;
              };
              // An open configuration overlay can select another already-open file.
              // Refresh membership before replaying content, bounded by source admission.
              for (let pass = 0; pass <= SOURCE_COUNT; pass++) {
                const selection = await next.sendRequest<{watchFiles: string[]}>('gazette/selectSource', {uri: workspaceUri.toString()});
                const manifest = await next.sendRequest<{generation: number; sources: SelectedSource[]; asset_observations?: Record<string, unknown>}>('gazette/editorSnapshot', {});
                if (this.client !== next || this.stopping || this.requested) return;
                const text = manifest.sources.filter(source => source.role.kind !== 'page_input');
                const all = new Set([...manifest.sources.map(source => source.id), ...Object.keys(manifest.asset_observations ?? {})]);
                if ([...all].some(uri => this.custody.has(uri) && this.custody.get(uri) !== this.key)) {
                  throw new Error('Gazette selected source is already held by another composition.');
                }
                for (const uri of this.held) if (!all.has(uri) && this.custody.get(uri) === this.key) this.custody.delete(uri);
                this.held = all;
                for (const uri of all) this.custody.set(uri, this.key);
                selected.clear();
                for (const source of text) selected.add(source.id);
                next.refreshSelection();
                this.configurationUris = new Set(text.filter(source => ['workspace', 'workspace_document', 'theme', 'plugin'].includes(source.role.kind)).map(source => source.id));
                this.watchSources(new Set([...all, ...selection.watchFiles]), text.map(source => vscode.Uri.parse(source.id)));
                const sources = new Map(text.map(source => [source.id, source]));
                let configurationReplayed = false;
                for (const document of vscode.workspace.textDocuments) {
                  const source = sources.get(document.uri.toString());
                  if (source && configuration(source) && await replay(document, source)) configurationReplayed = true;
                }
                if (configurationReplayed) continue;
                for (const document of vscode.workspace.textDocuments) {
                  const source = sources.get(document.uri.toString());
                  if (source && !configuration(source)) await replay(document, source);
                }
                next.refreshSymbols(manifest.generation);
                this.changed();
                return;
              }
              throw new Error('Gazette source selection did not settle within its source limit.');
            });
            synchronization = operation.catch(() => {});
            return operation;
          };
          const navigate = async <T>(document: vscode.TextDocument, token: vscode.CancellationToken, forward: () => vscode.ProviderResult<T>) => {
            const epoch = this.captureEpoch;
            const version = document.version;
            if (!this.currentClient(next, epoch)) return null;
            try {
              if (captured.discovery) await admittedText();
              if (!this.currentClient(next, epoch) || document.version !== version || !currentOpen(document)) return null;
              const result = await next.navigation(document, token, forward);
              return this.currentClient(next, epoch) ? result : null;
            } catch (error) {
              if (this.currentClient(next, epoch)) throw error;
              return null;
            }
          };
          const next: SelectedLanguageClient = new SelectedLanguageClient(`gazette:${this.key}`, `Gazette (${this.folder.name})`, server, {
            workspaceFolder: this.folder,
            documentSelector: captured.discovery ? [{scheme: 'file'}] : documentSelector(captured.uris),
            initializationOptions: captured.initializationOptions,
            outputChannel: this.output,
            revealOutputChannelOn: RevealOutputChannelOn.Never,
            errorHandler: {
              error: () => ({ action: ErrorAction.Shutdown }),
              closed: () => ({ action: CloseAction.DoNotRestart }),
            },
            middleware: {
              didOpen: async (document, forward) => {
                if (captured.discovery) {
                  await admit(async () => {
                    if (!currentOpen(document) || this.client !== next || this.stopping || this.requested) return;
                    const admission = await next.sendRequest<{selected: boolean}>('gazette/selectSource', {uri: document.uri.toString()});
                    if (!admission.selected || !currentOpen(document)) return;
                    await synchronize(document);
                    if (owns(document.uri) && currentOpen(document)) await forward(document);
                  });
                  return;
                }
                if (owns(document.uri)) await forward(document);
              },
              didChange: async event => {
                await next.fullChange(event);
                if (captured.discovery && this.configurationUris.has(event.document.uri.toString())) await synchronize();
              },
              didClose: async (document, forward) => {
                if (captured.discovery) {
                  await admit(async () => {
                    if (this.client !== next || this.stopping || this.requested) return;
                    if (selected.has(document.uri.toString())) await forward(document);
                    await synchronize();
                  });
                  return;
                }
                if (selected.has(document.uri.toString())) await forward(document);
              },
              didSave: async (document, forward) => {
                if (owns(document.uri)) await forward(document);
                if (captured.discovery) await synchronize();
              },
              provideDocumentSymbols: (document, token, forward) => navigate(document, token,
                () => forward(document, token)),
              provideDefinition: (document, position, token, forward) => navigate(document, token,
                () => forward(document, position, token)),
              provideDocumentLinks: (document, token, forward) => navigate(document, token,
                () => forward(document, token)),
              provideReferences: (document, position, options, token, forward) => navigate(document, token,
                () => forward(document, position, options, token)),
              provideCodeActions: (document, range, context, token, forward) => navigate(document, token,
                () => forward(document, range, context, token)),
              provideHover: (document, position, token, forward) => navigate(document, token,
                () => forward(document, position, token)),
              provideCompletionItem: (document, position, context, token, forward) => navigate(document, token,
                () => forward(document, position, context, token)),
              handleDiagnostics: (uri, diagnostics, forward) => { if (owns(uri)) forward(uri, diagnostics); },
            },
          }, owns, captured.rootUris.map(root => {
            const folder = vscode.workspace.workspaceFolders?.find(folder => folder.uri.toString() === root);
            if (!folder) throw new Error('Gazette selected root was removed before initialization.');
            return folder;
          }));
          this.client = next;
          if (captured.discovery) {
            this.nativeAdmission = admittedText;
            this.nativeRefresh = () => {
              const operation = (this.nativeWork ?? Promise.resolve()).then(async () => {
                if (this.client !== next || this.stopping || this.requested) return;
                await next.sendNotification('workspace/didChangeWatchedFiles', {changes: []});
                await synchronize();
              });
              const settled = operation.finally(() => { if (this.nativeWork === settled) this.nativeWork = undefined; });
              this.nativeWork = settled;
              return settled;
            };
          }
          stage = 'native initialization';
          await next.start();
          const capabilities = next.initializeResult?.capabilities;
          const sync = capabilities?.textDocumentSync;
          if (capabilities?.experimental?.gazette?.authoring !== '0.1.0'
              || (captured.discovery && capabilities.experimental?.gazette?.sourceDiscovery?.selectSource !== 'gazette/selectSource')
              || capabilities.documentSymbolProvider !== true
              || (typeof sync === 'number' ? sync : sync?.change) !== TextDocumentSyncKind.Full) {
            throw new Error('Gazette authoring protocol is unsupported.');
          }
          if (captured.discovery) await synchronize();
          stage = 'selected configuration language assignment';
          for (const document of vscode.workspace.textDocuments) await this.language(document);
          this.admitted = true;
          this.notifiedProblem = undefined;
        } catch (error) {
          if (stage === 'configuration and explicit source selection') this.watchSources([]);
          try { await this.stop(); }
          catch (cleanup) { throw new AggregateError([error, cleanup], 'Gazette startup and cleanup remain unproven.'); }
          this.startupProblem(error);
        }
      }
    })().catch(error => {
      this.blocked = true;
      this.problem = 'Gazette could not finish its previous language-server session. Source ownership is retained. Once its process closes, run Gazette: Restart Server to verify closure and retry.';
      this.output.appendLine(this.problem);
      throw error;
    })
      .finally(() => { this.transition = undefined; this.changed(); });
    return this.transition;
  }

  shutdown(): Promise<void> {
    if (!this.shutdownPromise) {
      this.stopping = true;
      this.requested = false;
      if (this.fileRefresh !== undefined) { clearTimeout(this.fileRefresh); this.fileRefresh = undefined; }
      this.watchSources([]);
      this.closedDocuments.dispose();
      this.shutdownPromise = (async () => {
        let failure: unknown;
        try { await this.transition; } catch (error) { failure = error; }
        try { await this.stop(); }
        catch (error) { this.blocked = true; failure = failure ? new AggregateError([failure, error], 'Gazette shutdown remains unproven.') : error; }
        if (failure) throw failure;
      })();
    }
    return this.shutdownPromise;
  }
}
