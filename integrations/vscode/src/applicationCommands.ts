import type * as vscode from 'vscode';
import { createHash } from 'node:crypto';
import applicationCommandCatalog = require('./applicationCommandCatalog.json');
import {
  ApplicationError,
  ApplicationSession,
  coherentEffectProfile,
  type ApplicationResult,
  type ToolCapability,
  type ToolDescriptor,
  type ToolIntent,
  type ToolPotentialEffect,
  type ToolRelationKind,
} from './applicationSession';
import {
  guidedRequest,
  requestSkeleton,
  requestText,
  RequestFragment,
  setRequestValue,
  type GuidedField,
} from './applicationRequest';
import { executable, sourcePath } from './settings';
import { BuildInputError, openBuildInput } from './buildInputs';
import { editorContext, type EditorSnapshot } from './editorSnapshot';
import { workspaceCheckText } from './workspaceCheck';

type Host = typeof vscode;
type Session = {
  readonly folder: vscode.WorkspaceFolder;
  readonly executable: string;
  readonly native: ApplicationSession;
  current: boolean;
  retiring: boolean;
  receipt: RetainedReceipt | undefined;
  undisplayed: boolean;
};
type Request = {
  readonly session: Session;
  readonly tool: ToolDescriptor;
  readonly catalog: readonly ToolDescriptor[];
  readonly document: vscode.TextDocument;
};
type StepResult<T> = { readonly action: 'value'; readonly value: T }
  | { readonly action: 'back' }
  | { readonly action: 'cancel' };
type ReceiptDisposition = 'success' | 'failed-with-evidence' | 'not-dispatched' | 'outcome-unknown';
type RetainedReceipt = ApplicationResult & { readonly disposition: ReceiptDisposition; readonly operation?: string };

export interface EditorInspection {
  readonly editor_generation: number;
  readonly document: string;
  readonly offset: number;
  readonly tool: string;
  readonly sources: EditorSnapshot['manifest']['sources'];
  readonly result: Record<string, unknown>;
}

const CAPABILITY_ORDER: readonly ToolCapability[] = [
  'authoring', 'workspace', 'build', 'preview', 'documentation', 'store', 'release', 'observation', 'recovery',
];
const INTENT_ORDER: readonly ToolIntent[] = ['inspect', 'act', 'verify', 'observe', 'reconcile', 'recover'];

export interface ApplicationCommandRoute {
  readonly cli: string;
  readonly tools: readonly string[];
}

/** Extension-owned interface projection; native MCP still owns schemas and effects. */
export const applicationCommandRoutes: readonly ApplicationCommandRoute[] = applicationCommandCatalog;

export function applicationCommandId(cli: string): string {
  return `gazette.application.${cli}`;
}

/** Bind one exact live descriptor to its schema-derived inert request. */
export function bindOperationRequest(tools: readonly ToolDescriptor[], name: string): {
  readonly tool: ToolDescriptor;
  readonly text: string;
} {
  const matches = tools.filter(tool => tool.name === name);
  if (matches.length !== 1) throw new Error('The current native catalog does not contain one unique named tool.');
  return { tool: matches[0]!, text: requestSkeleton(matches[0]!.inputSchema) };
}

const POTENTIAL_EFFECT_LABELS: Readonly<Record<ToolPotentialEffect, string>> = {
  'memory-state': 'changes process-local state',
  'filesystem-read': 'reads local files',
  'filesystem-write': 'writes local files',
  sqlite: 'uses SQLite coordination or database state',
  process: 'runs native subprocesses',
  git: 'mutates Git state',
  listener: 'opens or closes local listeners',
  network: 'uses the network',
};

function effectHints(tool: ToolDescriptor): string {
  const hints = tool.annotations;
  const effect = tool._meta['io.rtepl.gazette/effect'];
  if (!coherentEffectProfile(tool)) {
    return `${effect} effect · effect unknown (conflicting native hints) · review the exact operation before dispatch`;
  }
  const potential = tool._meta['io.rtepl.gazette/potentialEffects'];
  const effects = hints.readOnlyHint ? ['read-only'] : [];
  effects.push(...(potential.length
    ? potential.map(value => POTENTIAL_EFFECT_LABELS[value])
    : ['no native side effects listed']));
  if (hints.destructiveHint) effects.push('may replace or remove state');
  if (!hints.readOnlyHint && !hints.idempotentHint) effects.push('repeat may create another effect');
  return effects.join(' · ');
}

/** The explicit Execute command is sufficient only for a coherent, closed-world pure read. */
function requiresDispatchConfirmation(tool: ToolDescriptor): boolean {
  const hints = tool.annotations;
  return !coherentEffectProfile(tool)
    || tool._meta['io.rtepl.gazette/effect'] !== 'read'
    || !hints.readOnlyHint
    || hints.destructiveHint
    || !hints.idempotentHint
    || hints.openWorldHint;
}

function title(value: string): string {
  return `${value[0]?.toUpperCase() ?? ''}${value.slice(1).replaceAll('-', ' ')}`;
}

function compare(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function orderedTools(tools: readonly ToolDescriptor[]): ToolDescriptor[] {
  return [...tools].sort((left, right) => {
    const prominence = Number(left._meta['io.rtepl.gazette/prominence'] === 'advanced')
      - Number(right._meta['io.rtepl.gazette/prominence'] === 'advanced');
    return prominence || compare(left.title, right.title) || compare(left.name, right.name);
  });
}

function intentRank(intent: ToolIntent): number {
  return INTENT_ORDER.indexOf(intent);
}

function orderedIntents(groups: ReadonlyMap<ToolIntent, readonly ToolDescriptor[]>): ToolIntent[] {
  return [...groups].sort(([left, leftTools], [right, rightTools]) => {
    const prominence = Number(!leftTools.some(tool => tool._meta['io.rtepl.gazette/prominence'] === 'common'))
      - Number(!rightTools.some(tool => tool._meta['io.rtepl.gazette/prominence'] === 'common'));
    return prominence || intentRank(left) - intentRank(right);
  }).map(([intent]) => intent);
}

function relationTitle(kind: ToolRelationKind): string {
  return title(kind);
}

function schemaType(schema: Readonly<Record<string, unknown>>): string {
  if (typeof schema.type === 'string') return schema.type;
  return 'JSON';
}

function choiceValues(schema: Readonly<Record<string, unknown>>): readonly unknown[] | undefined {
  if (Object.hasOwn(schema, 'const')) return [schema.const];
  if (Array.isArray(schema.enum) && schema.enum.length <= 64) return schema.enum;
  const variants = Array.isArray(schema.oneOf) ? schema.oneOf : Array.isArray(schema.anyOf) ? schema.anyOf : undefined;
  if (!variants || variants.length > 64) return undefined;
  const values = variants.map(variant =>
    typeof variant === 'object' && variant !== null && Object.hasOwn(variant, 'const')
      ? (variant as Record<string, unknown>).const : undefined);
  return values.every(value => value !== undefined) ? values : undefined;
}

function choiceLabel(value: unknown): string {
  const label = JSON.stringify(value);
  return label.length <= 160 ? label : `${label.slice(0, 157)}...`;
}

class EditorActionError extends Error {}

function visibleError(error: unknown): string {
  if (error instanceof EditorActionError || error instanceof BuildInputError) return error.message;
  if (error instanceof ApplicationError) {
    if (error.effect === 'not-dispatched') {
      return 'Gazette did not dispatch this operation. Correct the request or session state before trying again.';
    }
    return 'Gazette cannot prove this operation’s outcome. Do not retry it. Retain the session and use the matching inspect, read-back, or recovery operation.';
  }
  return 'Gazette could not complete this editor action. No automatic retry or replacement was attempted; retain any private result and session custody.';
}

/** Editor custody and confirmation only; native MCP owns tools, schemas and effects. */
export class ApplicationCommands {
  private readonly sessions = new Map<string, Session>();
  private readonly requests = new Map<string, Request>();
  private readonly subscriptions: vscode.Disposable[];
  private stopping = false;
  private checking = false;
  private readonly resultDocuments = new Map<string, string>();

  constructor(private readonly host: Host, context: vscode.ExtensionContext) {
    const command = (name: string, run: (...args: unknown[]) => Promise<unknown>) =>
      host.commands.registerCommand(name, async (...args: unknown[]) => {
        try { return await run(...args); }
        catch (error) {
          const message = visibleError(error);
          await host.window.showErrorMessage(message);
          throw new Error(message);
        }
      });
    const routed = applicationCommandRoutes.map(route =>
      command(applicationCommandId(route.cli), () => this.createRoutedRequest(route)));
    this.subscriptions = [
      host.workspace.registerTextDocumentContentProvider('gazette-result', {
        provideTextDocumentContent: uri => this.resultDocuments.get(uri.toString()),
      }),
      host.workspace.onDidCloseTextDocument(document => {
        if (document.uri.scheme === 'gazette-result') this.resultDocuments.delete(document.uri.toString());
      }),
      ...routed,
      command('gazette.runCommand', (name?: unknown) => this.chooseCommand(name)),
      command('gazette.checkSite', () => this.checkSite()),
      command('gazette.createOperationRequest', (name?: unknown) => this.createRequest(name)),
      command('gazette.executeOperationRequest', () => this.executeRequest()),
      command('gazette.showApplicationTools', () => this.showTools()),
      command('gazette.showApplicationResult', () => this.showResult()),
      command('gazette.openBuildInput', () => this.showBuildInput()),
      command('gazette.stopApplicationSession', () => this.stopSession()),
      host.workspace.onDidChangeConfiguration(event => {
        for (const session of this.sessions.values()) {
          if (event.affectsConfiguration('gazette.server.path', session.folder.uri)) session.current = false;
        }
      }),
      host.workspace.onDidChangeWorkspaceFolders(event => {
        for (const folder of event.removed) {
          const session = this.sessions.get(folder.uri.toString());
          if (session) session.current = false;
        }
      }),
    ];
    context.subscriptions.push(...this.subscriptions);
  }

  private assertCurrent(session: Session) {
    if (this.stopping || !session.current || session.retiring
        || this.sessions.get(session.folder.uri.toString()) !== session
        || !this.host.workspace.workspaceFolders?.includes(session.folder)
        || executable(this.host.workspace, session.folder) !== session.executable) {
      session.current = false;
      throw new EditorActionError('The application session must be explicitly retired before replacement.');
    }
  }

  private async selected(preferred?: vscode.WorkspaceFolder): Promise<Session | undefined> {
    if (this.stopping) throw new Error('Application shutdown is in progress.');
    const folders = this.host.workspace.workspaceFolders ?? [];
    const folder = preferred ?? (folders.length === 1 ? folders[0] : (await this.host.window.showQuickPick(
      folders.map(folder => ({ label: folder.name, description: folder.uri.fsPath, folder })),
      { title: 'Gazette application workspace folder', ignoreFocusOut: true },
    ))?.folder);
    if (!folder) return undefined;
    // Read the user/machine setting after the picker; never capture authored sources here.
    const path = executable(this.host.workspace, folder);
    if (this.stopping || !this.host.workspace.workspaceFolders?.includes(folder)) throw new Error('Folder changed.');
    const key = folder.uri.toString();
    let session = this.sessions.get(key);
    if (!session) {
      if (this.sessions.size >= 32) throw new Error('Application session ceiling reached.');
      session = { folder, executable: path, native: new ApplicationSession(path, folder.uri.fsPath),
        current: true, retiring: false, receipt: undefined, undisplayed: false };
      this.sessions.set(key, session);
    }
    this.assertCurrent(session);
    return session;
  }

  async chooseEditorInspection(): Promise<string | undefined> {
    return (await this.host.window.showQuickPick([
      {label: 'Diagnostics', tool: 'authoring_diagnostics'},
      {label: 'Schema, fields and allowed values', tool: 'authoring_assistance'},
      {label: 'Theme component at cursor', tool: 'authoring_component_assistance'},
      {label: 'Hover at cursor', tool: 'authoring_hover'},
      {label: 'Reference completions at cursor', tool: 'authoring_complete_reference'},
      {label: 'Page owner and companions', tool: 'authoring_page_package'},
    ], {title: 'Query current editor through native Gazette MCP'}))?.tool;
  }

  /** Recreate exact native editor inputs in MCP; only read-only queries are exposed here. */
  async inspectEditor(folder: vscode.WorkspaceFolder, snapshot: EditorSnapshot, document: string, offset: number, tool: string, cancellation?: vscode.CancellationToken): Promise<EditorInspection | undefined> {
    if (!['authoring_diagnostics', 'authoring_assistance', 'authoring_component_assistance', 'authoring_hover', 'authoring_complete_reference', 'authoring_page_package'].includes(tool)) throw new Error('Unsupported editor inspection.');
    const session = await this.selected(folder);
    if (!session) return;
    if (!await snapshot.current()) throw new Error('The editor changed before the MCP query; capture it again.');
    const call = async (name: string, args: unknown): Promise<Record<string, unknown>> => {
      this.assertCurrent(session);
      const result = await session.native.call(name, JSON.stringify(args));
      const envelope = JSON.parse(result.raw);
      const value = envelope.result?.structuredContent;
      if (result.isError || !value || typeof value !== 'object') {
        session.receipt = {...result, disposition: 'failed-with-evidence'};
        session.undisplayed = true;
        throw new Error(`Native ${name} refused the current editor input. Use Gazette: Show Application Result to inspect the complete private receipt.`);
      }
      return value as Record<string, unknown>;
    };
    const context = await editorContext(snapshot, folder.uri.toString(), document, offset);
    if (cancellation?.isCancellationRequested) return;
    const started = await call('authoring_import_editor_context', {context,
      ...(snapshot.nativeSelection ? {allow_native_reads: true} : {})});
    const handle = started.session;
    const generation = started.generation;
    if (typeof handle !== 'string' || typeof generation !== 'number') throw new Error('MCP returned no authoring session identity.');
    let failure: unknown;
    try {
      if (cancellation?.isCancellationRequested) return;
      const args: Record<string, unknown> = {session: handle, generation, document};
      if (!['authoring_diagnostics', 'authoring_page_package'].includes(tool)) args.offset = offset;
      const value = await call(tool, args);
      if (cancellation?.isCancellationRequested) return;
      if (!await snapshot.current()) throw new Error('The editor changed during the MCP query; the stale result was discarded.');
      return {editor_generation: snapshot.generation, document, offset, tool,
        sources: snapshot.manifest.sources, result: value};
    } catch (error) {
      failure = error;
      throw error;
    } finally {
      try {
        await call('authoring_close_session', {session: handle, generation});
      } catch (cleanup) {
        if (failure instanceof Error) {
          throw new AggregateError([failure, cleanup], `${failure.message} Gazette could not verify closure of the temporary inspection session.`);
        }
        throw cleanup;
      }
    }
  }

  private async showRaw(raw: string, preserveFocus = false, language = 'json'): Promise<vscode.TextDocument> {
    // Content-addressed virtual documents are read-only and clean, while the
    // retained native receipt remains the authority for exact result bytes.
    const digest = createHash('sha256').update(raw).digest('hex');
    const uri = this.host.Uri.parse(`gazette-result:/result-${digest}.${language === 'json' ? 'json' : 'txt'}`);
    this.resultDocuments.set(uri.toString(), raw);
    try {
      const opened = await this.host.workspace.openTextDocument(uri);
      const document = await this.host.languages.setTextDocumentLanguage(opened, language);
      await this.host.window.showTextDocument(document, { preview: false, preserveFocus });
      return document;
    } catch (error) {
      if (!this.host.workspace.textDocuments.some(document => !document.isClosed && document.uri.toString() === uri.toString())) {
        this.resultDocuments.delete(uri.toString());
      }
      throw error;
    }
  }

  private async checkSite(): Promise<vscode.TextDocument | undefined> {
    if (this.checking) return undefined;
    this.checking = true;
    try {
      const editor = this.host.window.activeTextEditor;
      const folder = editor && this.host.workspace.getWorkspaceFolder(editor.document.uri);
      const session = await this.selected(folder);
      if (!session) return undefined;
      if (session.undisplayed) throw new EditorActionError('Show the previous application result before checking the site.');
      return await this.host.window.withProgress({
        location: this.host.ProgressLocation.Window, title: 'Gazette: Checking saved site',
      }, async () => {
        const catalog = await this.discover(session);
        this.assertCurrent(session);
        const tools = catalog.tools.filter(tool => tool.name === 'check_workspace');
        if (tools.length !== 1) throw new EditorActionError('This Gazette program does not provide one site check.');
        const tool = tools[0]!;
        if (!coherentEffectProfile(tool) || !tool.annotations.readOnlyHint
            || tool._meta['io.rtepl.gazette/potentialEffects'].some(effect => effect !== 'filesystem-read')) {
          throw new EditorActionError('This Gazette program does not provide a read-only site check.');
        }
        if (session.undisplayed) throw new EditorActionError('Show the previous application result before checking the site.');
        const configuration = sourcePath(this.host.workspace.getConfiguration('gazette', session.folder.uri)
          .get<unknown>('workspace.configuration', 'gazette.toml'));
        const workspace = this.host.Uri.joinPath(session.folder.uri, ...configuration.split('/')).fsPath;
        const receipt = await this.dispatch(session, tool.name, JSON.stringify({workspace}));
        const document = await this.showRaw(workspaceCheckText(receipt.raw), false, 'plaintext');
        if (session.receipt === receipt) session.undisplayed = false;
        return document;
      });
    } finally { this.checking = false; }
  }

  private nativeQuickPick<T>(
    items: readonly (vscode.QuickPickItem & { readonly value: T })[],
    title: string,
    step: number,
    totalSteps: number,
    allowBack: boolean,
  ): Promise<StepResult<T>> {
    const input = this.host.window.createQuickPick<vscode.QuickPickItem & { readonly value: T }>();
    input.title = title;
    input.step = step;
    input.totalSteps = totalSteps;
    input.ignoreFocusOut = true;
    input.items = items;
    input.buttons = allowBack ? [this.host.QuickInputButtons.Back] : [];
    return new Promise(resolve => {
      let settled = false;
      const subscriptions: vscode.Disposable[] = [];
      const finish = (result: StepResult<T>) => {
        if (settled) return;
        settled = true;
        input.hide();
        for (const subscription of subscriptions) subscription.dispose();
        input.dispose();
        resolve(result);
      };
      subscriptions.push(
        input.onDidAccept(() => {
          const selected = input.selectedItems[0];
          if (selected) finish({ action: 'value', value: selected.value });
        }),
        input.onDidTriggerButton(() => finish({ action: 'back' })),
        input.onDidHide(() => finish({ action: 'cancel' })),
      );
      input.show();
    });
  }

  private nativeInput(
    field: GuidedField,
    title: string,
    prompt: string,
    step: number,
    totalSteps: number,
    allowBack: boolean,
    previous: unknown,
  ): Promise<StepResult<string>> {
    const input = this.host.window.createInputBox();
    input.title = title;
    input.step = step;
    input.totalSteps = totalSteps;
    input.prompt = prompt;
    input.ignoreFocusOut = true;
    input.password = /(?:password|secret|token|credential|private[_ -]?key)/i.test(field.path.at(-1) ?? '');
    input.buttons = allowBack ? [this.host.QuickInputButtons.Back] : [];
    if (previous !== undefined) input.value = previous instanceof RequestFragment ? previous.source : typeof previous === 'string' ? previous : JSON.stringify(previous);
    return new Promise(resolve => {
      let settled = false;
      const subscriptions: vscode.Disposable[] = [];
      const finish = (result: StepResult<string>) => {
        if (settled) return;
        settled = true;
        input.hide();
        for (const subscription of subscriptions) subscription.dispose();
        input.dispose();
        resolve(result);
      };
      subscriptions.push(
        input.onDidAccept(() => finish({ action: 'value', value: input.value })),
        input.onDidTriggerButton(() => finish({ action: 'back' })),
        input.onDidHide(() => finish({ action: 'cancel' })),
      );
      input.show();
    });
  }

  private async guidedValue(
    field: GuidedField,
    step: number,
    totalSteps: number,
    allowBack: boolean,
    previous: unknown,
  ): Promise<StepResult<unknown>> {
    const title = `${field.required ? 'Required' : 'Optional'} field: ${field.label}`;
    const choices = choiceValues(field.schema);
    if (choices) {
      return this.nativeQuickPick(
        choices.map(value => ({ label: choiceLabel(value), value })),
        title, step, totalSteps, allowBack,
      );
    }
    if (field.schema.type === 'boolean') {
      return this.nativeQuickPick([
        { label: 'true', value: true }, { label: 'false', value: false },
      ], title, step, totalSteps, allowBack);
    }
    const stringValue = field.schema.type === 'string';
    const raw = await this.nativeInput(
      field,
      title,
      field.description ?? (stringValue
        ? 'Enter the exact string. Gazette performs authoritative validation after dispatch.'
        : `Enter one JSON ${schemaType(field.schema)} value. Gazette performs authoritative validation after dispatch.`),
      step,
      totalSteps,
      allowBack,
      previous,
    );
    if (raw.action !== 'value' || stringValue) return raw;
    try { return { action: 'value', value: RequestFragment.parse(raw.value) }; }
    catch { throw new Error(`Field ${field.label} requires one valid JSON value; no request was created.`); }
  }

  private async guidedText(
    session: Session,
    tool: ToolDescriptor,
    plan: ReturnType<typeof guidedRequest>,
  ): Promise<string | undefined> {
    const required = plan.fields.filter(field => field.required);
    const optional = plan.fields.filter(field => !field.required);
    const answers = new Map<GuidedField, unknown>();
    const totalSteps = Math.max(1, required.length + Number(optional.length > 0));
    const collectRequired = async (start: number): Promise<boolean> => {
      let index = start;
      while (index < required.length) {
        const field = required[index]!;
        const entered = await this.guidedValue(field, index + 1, totalSteps, index > 0, answers.get(field));
        this.assertCurrent(session);
        if (entered.action === 'cancel') return false;
        if (entered.action === 'back') {
          index = Math.max(0, index - 1);
          continue;
        }
        answers.set(field, entered.value);
        index++;
      }
      return true;
    };
    if (!await collectRequired(0)) return undefined;

    const value = JSON.parse(JSON.stringify(plan.value)) as Record<string, unknown>;
    for (const field of required) setRequestValue(value, field.path, answers.get(field));
    const remaining = new Set(optional);
    while (remaining.size) {
      const optionalStep = required.length + 1;
      const selected = await this.nativeQuickPick([
        { label: 'Review request', description: 'Open the bounded request assembled so far.', value: undefined },
        ...[...remaining].map(field => ({
          label: field.label,
          description: schemaType(field.schema),
          detail: field.description ?? 'Optional live-schema field',
          value: field,
        })),
      ], `${tool.title}: optional fields`, optionalStep, totalSteps, required.length > 0);
      this.assertCurrent(session);
      if (selected.action === 'cancel') return undefined;
      if (selected.action === 'back') {
        if (!await collectRequired(required.length - 1)) return undefined;
        for (const field of required) setRequestValue(value, field.path, answers.get(field));
        continue;
      }
      if (!selected.value) break;
      const entered = await this.guidedValue(
        selected.value, optionalStep, totalSteps, true, answers.get(selected.value),
      );
      this.assertCurrent(session);
      if (entered.action === 'cancel') return undefined;
      if (entered.action === 'back') continue;
      answers.set(selected.value, entered.value);
      setRequestValue(value, selected.value.path, entered.value);
      remaining.delete(selected.value);
    }
    return requestText(value);
  }

  async showTools(): Promise<vscode.TextDocument | undefined> {
    const session = await this.selected();
    if (!session) return undefined;
    const catalog = await this.discover(session);
    this.assertCurrent(session);
    const document = await this.showRaw(catalog.raw);
    this.assertCurrent(session);
    return document;
  }

  /** Optional exact name is still selected from the same freshly discovered native catalog. */
  async createRequest(name?: unknown): Promise<vscode.Uri | undefined> {
    if (name !== undefined && typeof name !== 'string') throw new Error('Expected a tool name.');
    const session = await this.selected();
    if (!session) return undefined;
    const catalog = await this.discover(session);
    this.assertCurrent(session);
    let selected: { readonly name: string } | undefined;
    if (name === undefined) {
      const groups = new Map<ToolCapability, ToolDescriptor[]>();
      for (const tool of catalog.tools) {
        const group = tool._meta['io.rtepl.gazette/capability'];
        groups.set(group, [...(groups.get(group) ?? []), tool]);
      }
      const group = groups.size === 1 ? [...groups.keys()][0] : (await this.host.window.showQuickPick(
        CAPABILITY_ORDER.filter(name => groups.has(name)).map(name => {
          const tools = groups.get(name) ?? [];
          const common = tools.filter(tool => tool._meta['io.rtepl.gazette/prominence'] === 'common').length;
          return {
            label: title(name),
            description: `${tools.length} live operation${tools.length === 1 ? '' : 's'}`,
            detail: `${common} common · ${tools.length - common} advanced`,
            name,
          };
        }),
        { title: 'Gazette capability', ignoreFocusOut: true },
      ))?.name;
      this.assertCurrent(session);
      if (!group) return undefined;
      const intentGroups = new Map<ToolIntent, ToolDescriptor[]>();
      for (const tool of groups.get(group) ?? []) {
        const intent = tool._meta['io.rtepl.gazette/intent'];
        intentGroups.set(intent, [...(intentGroups.get(intent) ?? []), tool]);
      }
      const intents = orderedIntents(intentGroups);
      const intent = intents.length === 1 ? intents[0] : (await this.host.window.showQuickPick(
        intents.map(intent => {
          const tools = intentGroups.get(intent) ?? [];
          const common = tools.filter(tool => tool._meta['io.rtepl.gazette/prominence'] === 'common').length;
          return {
            label: title(intent),
            description: `${tools.length} operation${tools.length === 1 ? '' : 's'}`,
            detail: `${common} common · ${tools.length - common} advanced`,
            intent,
          };
        }),
        { title: `Gazette ${group}: purpose`, ignoreFocusOut: true },
      ))?.intent;
      this.assertCurrent(session);
      if (!intent) return undefined;
      const tools = orderedTools(intentGroups.get(intent) ?? []);
      selected = tools.length === 1 ? tools[0] : (await this.host.window.showQuickPick(tools.map(tool => ({
        label: tool.title,
        description: effectHints(tool),
        detail: `${title(tool._meta['io.rtepl.gazette/prominence'])} · ${tool.description ?? ''}`,
        tool,
      })),
        { title: `Gazette ${group}: ${title(intent)}`, ignoreFocusOut: true }))?.tool;
    } else {
      selected = { name };
    }
    this.assertCurrent(session);
    if (!selected) {
      if (name !== undefined) throw new Error('The current native catalog does not contain this tool.');
      return undefined;
    }
    const binding = bindOperationRequest(catalog.tools, selected.name);
    return this.openRequest(session, binding, catalog.tools);
  }

  private async chooseCommand(name?: unknown): Promise<vscode.Uri | undefined> {
    if (name !== undefined) {
      const route = applicationCommandRoutes.find(route => route.cli === name);
      if (!route) throw new EditorActionError('Choose a command from Gazette: Run Command.');
      return this.createRoutedRequest(route);
    }
    const selected = await this.host.window.showQuickPick(
      applicationCommandRoutes.map(route => ({ label: title(route.cli), description: `gazette ${route.cli}`, route })),
      { title: 'Gazette: Run Command', placeHolder: 'Choose a command to prepare its request', matchOnDescription: true },
    );
    return selected ? this.createRoutedRequest(selected.route) : undefined;
  }

  private async createRoutedRequest(route: ApplicationCommandRoute): Promise<vscode.Uri | undefined> {
    const session = await this.selected();
    if (!session) return undefined;
    const catalog = await this.discover(session);
    this.assertCurrent(session);
    const bindings = orderedTools(route.tools.map(name => bindOperationRequest(catalog.tools, name).tool))
      .map(tool => bindOperationRequest(catalog.tools, tool.name));
    const binding = bindings.length === 1 ? bindings[0] : (await this.host.window.showQuickPick(
      bindings.map(binding => ({
        label: binding.tool.name,
        description: effectHints(binding.tool),
        detail: binding.tool.description ?? '',
        binding,
      })),
      { title: `Gazette ${route.cli} operation`, ignoreFocusOut: true },
    ))?.binding;
    this.assertCurrent(session);
    if (!binding) return undefined;
    return this.openRequest(session, binding, catalog.tools);
  }

  private async openRequest(
    session: Session,
    binding: ReturnType<typeof bindOperationRequest>,
    catalog: readonly ToolDescriptor[],
  ): Promise<vscode.Uri | undefined> {
    for (const [key, request] of this.requests) if (request.document.isClosed) this.requests.delete(key);
    if (this.requests.size >= 32) throw new Error('Close an earlier operation request before creating another.');
    const plan = guidedRequest(binding.tool.inputSchema);
    const mode = plan.defaultMode === 'advanced'
      ? { mode: 'advanced' as const }
      : await this.host.window.showQuickPick([
        {
          label: 'Guided request',
          description: 'Recommended',
          detail: `Enter ${plan.requiredLeafCount} required live-schema field${plan.requiredLeafCount === 1 ? '' : 's'} in a bounded sequence; Gazette remains the validator.`,
          mode: 'guided' as const,
        },
        {
          label: 'Advanced JSON',
          description: 'Explicit escape hatch',
          detail: 'Open the complete schema-derived inert structure for direct editing.',
          mode: 'advanced' as const,
        },
      ], { title: `${binding.tool.title}: request mode`, ignoreFocusOut: true });
    this.assertCurrent(session);
    if (!mode) return undefined;
    const content = mode.mode === 'guided'
      ? await this.guidedText(session, binding.tool, plan)
      : binding.text;
    this.assertCurrent(session);
    if (content === undefined) return undefined;
    const document = await this.host.workspace.openTextDocument({
      language: 'json',
      content,
    });
    this.assertCurrent(session);
    await this.host.window.showTextDocument(document, { preview: false });
    this.assertCurrent(session);
    if (this.requests.size >= 32) throw new Error('Prepared request ceiling reached while the editor opened.');
    this.requests.set(document.uri.toString(), { session, tool: binding.tool, catalog, document });
    return document.uri;
  }

  private assertRequest(request: Request, version: number, text: string) {
    this.assertCurrent(request.session);
    const document = request.document;
    if (document.isClosed || this.host.window.activeTextEditor?.document !== document
        || this.requests.get(document.uri.toString()) !== request
        || !this.host.workspace.textDocuments.includes(document)
        || document.version !== version || document.getText() !== text) {
      throw new EditorActionError('The exact prepared request buffer changed before dispatch.');
    }
  }

  async executeRequest(): Promise<vscode.TextDocument | vscode.Uri | undefined> {
    const document = this.host.window.activeTextEditor?.document;
    const request = document && this.requests.get(document.uri.toString());
    if (!document || !request || request.document !== document) throw new EditorActionError('Select an associated Gazette request buffer.');
    const version = document.version;
    const text = document.getText();
    this.assertRequest(request, version, text);
    if (request.session.undisplayed) throw new EditorActionError('Show the previous application result before another operation.');
    if (requiresDispatchConfirmation(request.tool)) {
      const choice = await this.host.window.showWarningMessage(
        `Run the exact current JSON request for ${request.tool.name} in ${request.session.folder.name}? Live native hints: ${effectHints(request.tool)}. Hints do not grant permission; effects depend on your explicit arguments. After dispatch, closing this dialog or editor does not cancel effects.`,
        { modal: true }, 'Run',
      );
      if (choice !== 'Run') return undefined;
    }
    this.assertRequest(request, version, text);
    if (request.session.undisplayed) throw new EditorActionError('Show the previous application result before another operation.');
    // Do not parse/stringify arguments: duplicate keys and integer/string lexemes reach Rust unchanged.
    await this.dispatch(request.session, request.tool.name, text);
    // A changed editor/folder after dispatch cannot erase completed native evidence.
    return this.postcondition(request);
  }

  private async dispatch(session: Session, operation: string, text: string): Promise<RetainedReceipt> {
    let receipt: RetainedReceipt;
    try {
      const result = await session.native.call(operation, text);
      receipt = {
        ...result,
        operation,
        disposition: result.isError ? 'failed-with-evidence' : 'success',
      };
    }
    catch (error) {
      if (!(error instanceof ApplicationError) || error.raw === undefined) throw error;
      receipt = {
        raw: error.raw,
        isError: true,
        disposition: error.effect === 'not-dispatched' ? 'not-dispatched' : 'outcome-unknown',
      };
    }
    session.receipt = receipt;
    session.undisplayed = true;
    return receipt;
  }

  private followups(request: Request): readonly {
    readonly kind: ToolRelationKind | 'terminal result';
    readonly tool: ToolDescriptor;
  }[] {
    const metadata = request.tool._meta;
    const edges: { readonly kind: ToolRelationKind | 'terminal result'; readonly operation: string }[] = [];
    const terminal = metadata['io.rtepl.gazette/terminalResultOwner'];
    if (terminal) edges.push({ kind: 'terminal result', operation: terminal });
    edges.push(...(metadata['io.rtepl.gazette/relations'] ?? []));
    const seen = new Set<string>();
    return edges.flatMap(edge => {
      if (seen.has(edge.operation)) return [];
      seen.add(edge.operation);
      return [{ kind: edge.kind, tool: bindOperationRequest(request.catalog, edge.operation).tool }];
    });
  }

  private failureFollowups(request: Request): readonly {
    readonly kind: ToolRelationKind;
    readonly tool: ToolDescriptor;
  }[] {
    return this.followups(request).flatMap(followup =>
      followup.kind !== 'terminal result'
        && ['inspect', 'observe', 'reconcile', 'recover'].includes(followup.kind)
        ? [{ kind: followup.kind, tool: followup.tool }]
        : []);
  }

  private async postcondition(request: Request): Promise<vscode.TextDocument | vscode.Uri | undefined> {
    const { session, tool } = request;
    if (!session.receipt) throw new EditorActionError('No completed native response is retained.');
    const receipt = session.receipt;
    const failureFollowups = this.failureFollowups(request);
    const failureNames = failureFollowups.map(followup => followup.tool.name).join(', ');
    const message = receipt.disposition === 'outcome-unknown'
      ? `${tool.title} has an unknown outcome. Do not retry it. The full private response and process custody are retained.${failureNames ? ` Canonical evidence or recovery-related operations are ${failureNames}; these relations are guidance, not safety or authority, and this message does not run them.` : ' No canonical evidence or recovery-related follow-up is declared.'}`
      : receipt.disposition === 'not-dispatched'
        ? `${tool.title} was not dispatched. The full private refusal is retained; correct the request or session state before trying again.`
        : receipt.disposition === 'failed-with-evidence'
          ? `${tool.title} completed with a reported failure. The full private receipt is retained; no retry was attempted.${failureNames ? ` Canonical evidence or recovery-related operations are ${failureNames}; their relations do not make them safe or authorized.` : ''}`
          : tool._meta['io.rtepl.gazette/terminalResultOwner']
            ? `${tool.title} returned successfully. Its held lifecycle remains active; ${tool._meta['io.rtepl.gazette/terminalResultOwner']} owns the terminal result. The full native receipt is retained. No follow-up effect ran automatically.`
            : tool._meta['io.rtepl.gazette/closure'] === 'related'
              ? `${tool.title} completed this operation successfully. Related lifecycle operations remain separately selectable; the full native receipt is retained.`
              : `${tool.title} completed successfully. The full native receipt is retained.`;
    const followups = receipt.disposition === 'success'
      ? this.followups(request)
      : receipt.disposition === 'failed-with-evidence'
        ? failureFollowups
        : [];
    const followupAction = receipt.disposition === 'failed-with-evidence'
      ? 'Prepare Evidence/Recovery Follow-up…'
      : 'Prepare Canonical Follow-up…';
    const action = receipt.disposition === 'success'
      ? await this.host.window.showInformationMessage(message, 'Open Full Receipt',
        ...(tool.name === 'render_sites' ? ['Open Build Input…'] : []),
        ...(followups.length ? [followupAction] : []))
      : await this.host.window.showWarningMessage(message, 'Open Full Receipt',
        ...(followups.length ? [followupAction] : []));
    if (session.receipt !== receipt || !action) return undefined;
    if (action === 'Open Full Receipt') return this.deliver(session);
    if (action === 'Open Build Input…') { await this.showBuildInput(session); return undefined; }
    if (action !== followupAction) return undefined;
    await this.deliver(session, true);
    const followup = followups.length === 1 ? followups[0] : (await this.host.window.showQuickPick(
      followups.map(followup => ({
        label: followup.tool.title,
        description: followup.kind === 'terminal result' ? 'Terminal result owner' : `${relationTitle(followup.kind)} relation`,
        detail: `${effectHints(followup.tool)} · prepares an inert request only`,
        followup,
      })),
      {
        title: `${tool.title}: ${receipt.disposition === 'failed-with-evidence' ? 'evidence/recovery' : 'canonical'} follow-up`,
        ignoreFocusOut: true,
      },
    ))?.followup;
    this.assertCurrent(session);
    if (!followup) return undefined;
    return this.openRequest(session, bindOperationRequest(request.catalog, followup.tool.name), request.catalog);
  }

  private async deliver(session: Session, preserveFocus = false): Promise<vscode.TextDocument> {
    if (!session.receipt) throw new EditorActionError('No completed native response is retained.');
    const receipt = session.receipt;
    const document = await this.showRaw(receipt.raw, preserveFocus);
    if (session.receipt === receipt) session.undisplayed = false;
    return document;
  }

  private async discover(session: Session) {
    if (session.undisplayed) throw new Error('Show the previous application result before discovery.');
    try { return await session.native.discover(); }
    catch (error) {
      if (error instanceof ApplicationError && error.raw !== undefined) {
        session.receipt = {
          raw: error.raw,
          isError: true,
          disposition: error.effect === 'not-dispatched' ? 'not-dispatched' : 'outcome-unknown',
        };
        session.undisplayed = true;
      }
      throw error;
    }
  }

  private async retained(): Promise<Session | undefined> {
    const entries = [...this.sessions.values()];
    return entries.length === 1 ? entries[0] : (await this.host.window.showQuickPick(
      entries.map(session => ({ label: session.folder.name, description: session.folder.uri.fsPath, session })),
      { title: 'Retained Gazette application session', ignoreFocusOut: true },
    ))?.session;
  }

  async showResult(): Promise<vscode.TextDocument | undefined> {
    const session = await this.retained();
    return session && this.deliver(session);
  }

  private async showBuildInput(selected?: Session): Promise<void> {
    const session = selected ?? await this.retained();
    if (!session) return;
    const receipt = session.receipt;
    if (!receipt || receipt.operation !== 'render_sites' || receipt.disposition !== 'success') {
      throw new EditorActionError('Run Gazette render_sites in this workspace to inspect its retained build inputs.');
    }
    await openBuildInput(this.host, session.folder, receipt.raw, () => {
      this.assertCurrent(session);
      if (session.receipt !== receipt) throw new EditorActionError('The Gazette build receipt changed; choose its current input again.');
    });
    if (session.receipt === receipt) session.undisplayed = false;
  }

  private async retire(session: Session) {
    if (this.sessions.get(session.folder.uri.toString()) !== session || session.retiring) throw new Error('Session changed.');
    if (session.undisplayed) throw new Error('Show the retained result before retiring this session.');
    session.retiring = true;
    try {
      await session.native.shutdown();
      this.sessions.delete(session.folder.uri.toString());
      for (const [key, request] of this.requests) if (request.session === session) this.requests.delete(key);
    } finally {
      session.retiring = false;
    }
  }

  async stopSession(): Promise<void> {
    const session = await this.retained();
    if (!session) return;
    const choice = await this.host.window.showWarningMessage(
      'Stop this Gazette application session? Close its native handles first. A failed or uncertain cleanup keeps this session in custody; no replacement is started.',
      { modal: true }, 'Stop',
    );
    if (choice === 'Stop') await this.retire(session);
  }

  async shutdown(): Promise<void> {
    this.stopping = true;
    const failures: unknown[] = [];
    for (const session of this.sessions.values()) {
      try { await this.retire(session); } catch (error) { failures.push(error); }
    }
    if (failures.length) throw new AggregateError(failures, 'Application cleanup remains unproven.');
    for (const subscription of this.subscriptions.splice(0)) subscription.dispose();
  }
}
