import * as vscode from 'vscode';
import { WorkspaceSession } from './workspaceSession';
import { ApplicationCommands } from './applicationCommands';
import { AuthoringView } from './authoringView';

const SETTINGS = [
  'gazette.authoring.enabled',
  'gazette.server.path',
  'gazette.workspace.configuration',
  'gazette.workspace.discovery',
  'gazette.workspace.manualSupport',
  'gazette.workspace.roots',
  'gazette.workspace.configurationDocuments',
  'gazette.workspace.documents',
  'gazette.workspace.manuals',
  'gazette.workspace.pagePackageInputs',
  'gazette.workspace.packageBaselines',
];
let runtime: Runtime | undefined;

/** Folder membership and presentation only; each session owns its guarded transport. */
class Runtime {
  private readonly output: vscode.LogOutputChannel;
  private readonly sessions: Map<string, WorkspaceSession>;
  private readonly custody: Map<string, string>;
  private readonly dirty: Set<string>;
  private transition: Promise<void> | undefined;
  private restarting: Promise<void> | undefined;
  private stopping: boolean;
  private shutdownPromise: Promise<void> | undefined;
  private readonly subscriptions: vscode.Disposable[];
  private readonly application: ApplicationCommands;
  private readonly authoring: AuthoringView;

  constructor(context: vscode.ExtensionContext) {
    this.application = new ApplicationCommands(vscode, context);
    this.output = vscode.window.createOutputChannel('Gazette', { log: true });
    this.sessions = new Map();
    this.custody = new Map();
    this.dirty = new Set();
    this.transition = undefined;
    this.stopping = false;
    this.shutdownPromise = undefined;
    this.subscriptions = [
      vscode.commands.registerCommand('gazette.restartLanguageServer', () => this.restartAll()),
      vscode.workspace.onDidChangeConfiguration(event => {
        const folders = (vscode.workspace.workspaceFolders ?? []).filter(folder => SETTINGS.some(key => event.affectsConfiguration(key, folder.uri)));
        void this.request(folders).catch(() => this.report());
      }),
      vscode.workspace.onDidGrantWorkspaceTrust(() => { void this.refreshAll().catch(() => this.report()); }),
      vscode.workspace.onDidChangeWorkspaceFolders(event => {
        const changed = [...event.added, ...event.removed];
        // Ancestors may lose/gain ownership of an explicitly selected nested path.
        const current = vscode.workspace.workspaceFolders ?? [];
        const affected = current.filter(folder => {
          const session = this.sessions.get(folder.uri.toString());
          const roots = [folder, ...current.filter(root => session?.dependsOn(root.uri.toString()))];
          return changed.some(other => session?.dependsOn(other.uri.toString()) || roots.some(root =>
            root.uri.toString() === other.uri.toString()
            || (root.uri.scheme === other.uri.scheme && root.uri.authority === other.uri.authority
              && other.uri.path.startsWith(root.uri.path.replace(/\/$/, '') + '/'))));
        });
        void this.request([...affected, ...event.removed]).catch(() => this.report());
      }),
    ];
    this.authoring = new AuthoringView(uri => [...this.sessions.values()].find(session => session.owns(uri) || session.selects(uri)), this.application);
    this.subscriptions.push(
      vscode.workspace.onDidOpenTextDocument(document => {
        for (const session of this.sessions.values()) void session.language(document).catch(error => this.output.appendLine(String(error)));
      }),
      this.authoring,
    );
    context.subscriptions.push(...this.subscriptions);
  }

  report() {
    this.output.appendLine('A Gazette transition could not prove cleanup. No replacement for that folder was admitted; inspect the local process before reloading.');
  }

  refreshAll() { return this.request(vscode.workspace.workspaceFolders ?? []); }

  private restartAll(): Promise<void> {
    if (this.restarting) return this.restarting;
    this.restarting = (async () => {
      const results = await Promise.allSettled([...this.sessions.values()].map(session => session.retry()));
      const failures = results.filter((result): result is PromiseRejectedResult => result.status === 'rejected');
      if (failures.length) {
        this.report();
        void vscode.window.showErrorMessage('Gazette could not finish its previous language-server session. Its process and source ownership are retained; inspect Gazette output before retrying.');
        throw new AggregateError(failures.map(result => result.reason), 'Gazette language-server cleanup remains unproven.');
      }
      await this.refreshAll();
    })().finally(() => { this.restarting = undefined; });
    return this.restarting;
  }

  request(folders: readonly vscode.WorkspaceFolder[]): Promise<void> {
    if (this.stopping) return Promise.resolve();
    for (const folder of folders) this.dirty.add(folder.uri.toString());
    if (this.transition) return this.transition;
    this.transition = (async () => {
      const failures: unknown[] = [];
      while (this.dirty.size && !this.stopping) {
        const dirty = new Set(this.dirty);
        this.dirty.clear();
        const current = new Map((vscode.workspace.workspaceFolders ?? []).map(folder => [folder.uri.toString(), folder]));
        for (const [key, session] of this.sessions) {
          if (current.has(key)) continue;
          try { await session.shutdown(); this.sessions.delete(key); }
          catch (error) { failures.push(error); }
        }
        if (this.stopping) break;
        const pending: Promise<void>[] = [];
        for (const [key, folder] of current) {
          if (!dirty.has(key)) continue;
          let session = this.sessions.get(key);
          if (!session) { session = new WorkspaceSession(folder, this.output, this.custody, () => this.authoring.invalidate()); this.sessions.set(key, session); }
          pending.push(session.refresh());
        }
        for (const result of await Promise.allSettled(pending)) {
          if (result.status === 'rejected') failures.push(result.reason);
        }
      }
      if (failures.length) throw new AggregateError(failures, 'Gazette folder transitions remain unproven.');
    })().finally(() => { this.transition = undefined; void this.authoring.refresh(); });
    return this.transition;
  }

  shutdown() {
    if (!this.shutdownPromise) {
      this.stopping = true;
      this.dirty.clear();
      for (const subscription of this.subscriptions.splice(0)) subscription.dispose();
      this.shutdownPromise = (async () => {
        const failures: unknown[] = [];
        try { await this.transition; } catch (error) { failures.push(error); }
        try { await this.restarting; } catch (error) { failures.push(error); }
        for (const [key, session] of this.sessions) {
          try { await session.shutdown(); this.sessions.delete(key); }
          catch (error) { failures.push(error); }
        }
        try { await this.application.shutdown(); } catch (error) { failures.push(error); }
        this.output.dispose();
        if (failures.length) throw new AggregateError(failures, 'Gazette shutdown remains unproven.');
      })();
    }
    return this.shutdownPromise;
  }
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  if (runtime) await runtime.shutdown();
  runtime = new Runtime(context);
  await runtime.refreshAll();
}

export async function deactivate(): Promise<void> {
  if (runtime) {
    await runtime.shutdown();
    runtime = undefined;
  }
}
