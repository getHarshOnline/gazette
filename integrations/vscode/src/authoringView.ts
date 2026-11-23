import * as vscode from 'vscode';
import type { WorkspaceSession } from './workspaceSession';
import type { ApplicationCommands, EditorInspection } from './applicationCommands';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { TextDecoder } from 'node:util';
import { exportEditorContext } from './editorSnapshot';

interface Item { label: string; value?: unknown; children?: Item[]; range?: vscode.Range; uri?: vscode.Uri; description?: string; command?: vscode.Command }
function children(value: unknown): Item[] {
  if (value === null || value === undefined) return [{label: 'None'}];
  if (Array.isArray(value)) return value.map((value, index) => ({label: String(index + 1), value}));
  if (typeof value === 'object') return Object.entries(value).map(([label, value]) => ({label: label.replaceAll('_', ' '), value}));
  return [{label: String(value)}];
}

function inspectionLabel(receipt: EditorInspection): string {
  if (receipt.tool === 'authoring_diagnostics' && Array.isArray(receipt.result.value)) {
    const count = receipt.result.value.length;
    return `MCP inspection · ${count} Gazette diagnostic${count === 1 ? '' : 's'}`;
  }
  return `MCP inspection · ${receipt.tool.replace(/^authoring_/, '').replaceAll('_', ' ')}`;
}

export function inspectionContext(receipt: EditorInspection, document: vscode.TextDocument): Item {
  const value = receipt.result.value;
  let items = children(value);
  if (receipt.tool === 'authoring_diagnostics' && Array.isArray(value)) {
    const bytes = Buffer.from(document.getText(), 'utf8');
    const digest = createHash('sha256').update(bytes).digest('hex');
    const source = receipt.sources.find(source => source.id === document.uri.toString());
    const decoder = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true});
    items = value.map(row => {
      const item: Item = {label: String(row.message ?? row.code), description: [row.code, row.field].filter(Boolean).join(' · ')};
      const span = row.location?.span;
      if (source?.path === row.path && source?.source_sha256 === digest && row.source_sha256 === digest
          && span && Number.isSafeInteger(span.start) && Number.isSafeInteger(span.end)
          && span.start >= 0 && span.end >= span.start && span.end <= bytes.length) {
        try {
          const start = document.positionAt(decoder.decode(bytes.subarray(0, span.start)).length);
          const end = document.positionAt(decoder.decode(bytes.subarray(0, span.end)).length);
          item.range = new vscode.Range(start, end);
          item.uri = document.uri;
        } catch { /* Invalid UTF-8 boundary has no editor action. */ }
      }
      return item;
    });
  }
  return {label: inspectionLabel(receipt), description: `generation ${receipt.editor_generation} · ${receipt.sources.length} selected`,
    children: [...items, {label: 'Export full inspection receipt',
      command: {command: 'gazette.exportEditorInspection', title: 'Export full inspection receipt'}}]};
}

/** Native facts rendered as an accessible outline; no TOML or Markdown interpretation. */
export class AuthoringView implements vscode.TreeDataProvider<Item>, vscode.Disposable {
  private readonly changes = new vscode.EventEmitter<Item | undefined>();
  readonly onDidChangeTreeData = this.changes.event;
  private items: Item[] = [{label: 'Open a selected Gazette source to inspect its context.'}];
  private inspection: {receipt: EditorInspection; current: () => Promise<boolean>} | undefined;
  private sequence = 0;
  private captureRunning = false;
  private activeSource: boolean | undefined;
  private inspectionCancellation: vscode.CancellationTokenSource | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly subscriptions: vscode.Disposable[];
  private readonly status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 90);
  private readonly cancelStatus = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 91);
  constructor(private readonly owner: (uri: vscode.Uri) => WorkspaceSession | undefined, application: ApplicationCommands) {
    this.status.command = 'gazette.showAuthoringContext'; this.status.text = '$(book) Gazette';
    this.cancelStatus.command = 'gazette.cancelEditorInspection'; this.cancelStatus.text = '$(close) Cancel Gazette inspection';
    this.subscriptions = [
      vscode.commands.registerCommand('gazette.cancelEditorInspection', () => this.inspectionCancellation?.cancel()),
      vscode.window.registerTreeDataProvider('gazette.authoringContext', this),
      vscode.window.onDidChangeActiveTextEditor(() => this.schedule()),
      vscode.window.onDidChangeTextEditorSelection(() => this.schedule()),
      vscode.workspace.onDidChangeTextDocument(() => this.schedule()),
      vscode.commands.registerCommand('gazette.showAuthoringContext', async () => {
        await this.refresh(); await vscode.commands.executeCommand('gazette.authoringContext.focus');
      }),
      vscode.commands.registerCommand('gazette.revealAuthoringField', async (uri: vscode.Uri, range: vscode.Range) => {
        await vscode.window.showTextDocument(uri, {selection: range, preview: false});
      }),
      vscode.commands.registerCommand('gazette.revealRetainedSource', async (owner: vscode.Uri, path: string) => {
        try {
          const session = this.owner(owner);
          if (!session) throw new Error('The owning Gazette composition is no longer active.');
          await vscode.commands.executeCommand('vscode.open', await session.sourceUri(path));
        } catch (error) { await vscode.window.showErrorMessage(error instanceof Error ? error.message : String(error)); }
      }),
      vscode.commands.registerCommand('gazette.exportEditorContext', () => this.captureOnce(async () => {
        const editor = vscode.window.activeTextEditor;
        const session = editor && this.owner(editor.document.uri);
        if (!editor || !session) { void vscode.window.showInformationMessage('Open a source selected by an active Gazette composition first.'); return; }
        try {
          const destination = await vscode.window.showSaveDialog({title: 'Export current Gazette editor context',
            defaultUri: vscode.Uri.file(join(tmpdir(), `gazette-editor-context-${Date.now()}.json`)), filters: {'Gazette editor context': ['json']}});
          if (!destination) return;
          if (vscode.window.activeTextEditor !== editor) throw new Error('The active editor changed; select its current source and try again.');
          const snapshot = await vscode.window.withProgress({location: vscode.ProgressLocation.Window, title: 'Gazette: exporting current editor context'}, async progress => {
            const offset = Buffer.byteLength(editor.document.getText(new vscode.Range(new vscode.Position(0, 0), editor.selection.active)), 'utf8');
            const snapshot = await session.snapshot(editor.document, (completed, total) => progress.report({message: `Reading current source ${completed} of ${total}`}));
            progress.report({message: 'Writing editor context'});
            const text = await exportEditorContext(snapshot, session.folder.uri.toString(), editor.document.uri.toString(), offset);
            if (destination.scheme !== 'file') throw new Error('Choose a local file for the explicit editor handoff.');
            // Fresh files only: neither a selected source nor a symlink alias can
            // be overwritten by an export, even after the save dialog returns.
            await writeFile(destination.fsPath, text, {encoding: 'utf8', flag: 'wx', mode: 0o600});
            return snapshot;
          });
          // A success toast can remain open indefinitely. It must not hold the
          // completed export's progress notification open.
          void vscode.window.showInformationMessage(snapshot.nativeSelection
            ? `Gazette context exported: ${snapshot.overlays.length} unsaved documents, generation ${snapshot.generation}. Import requires permission to read the saved workspace.`
            : `Gazette context exported: ${snapshot.manifest.sources.length} sources, generation ${snapshot.generation}. The file contains selected source text and retained assets.`);
          return destination;
        } catch (error) { void vscode.window.showErrorMessage(error instanceof Error ? error.message : String(error)); }
      })),
      vscode.commands.registerCommand('gazette.inspectEditorWithMcp', () => this.captureOnce(async () => {
        const editor = vscode.window.activeTextEditor;
        const session = editor && this.owner(editor.document.uri);
        if (!editor || !session) { void vscode.window.showInformationMessage('Open a source selected by an active Gazette composition first.'); return; }
        try {
          const tool = await application.chooseEditorInspection();
          if (!tool) return;
          if (vscode.window.activeTextEditor !== editor) throw new Error('The active editor changed; select its current source and try again.');
          const cancellation = new vscode.CancellationTokenSource();
          this.inspectionCancellation = cancellation;
          this.cancelStatus.show();
          const receipt = await vscode.window.withProgress({location: vscode.ProgressLocation.Window, title: 'Gazette: inspecting current editor through MCP'}, async progress => {
            const position = editor.selection.active;
            const offset = Buffer.byteLength(editor.document.getText(new vscode.Range(new vscode.Position(0, 0), position)), 'utf8');
            const snapshot = await session.snapshot(editor.document, (completed, total) => {
              if (cancellation.token.isCancellationRequested) throw new vscode.CancellationError();
              progress.report({message: `Reading current source ${completed} of ${total}`});
            });
            progress.report({message: 'Running native MCP query'});
            if (cancellation.token.isCancellationRequested) return;
            const receipt = await application.inspectEditor(session.folder, snapshot, editor.document.uri.toString(), offset, tool, cancellation.token);
            if (!receipt || cancellation.token.isCancellationRequested) return;
            if (!await snapshot.current() || vscode.window.activeTextEditor !== editor) {
              throw new Error('The current editor changed; capture it again to inspect its context.');
            }
            this.inspection = {receipt, current: snapshot.current};
            await this.refresh();
            return receipt;
          });
          if (receipt) {
            void vscode.window.showInformationMessage(inspectionLabel(receipt), 'Show in Authoring').then(action => {
              if (action === 'Show in Authoring') return vscode.commands.executeCommand('gazette.showAuthoringContext');
            });
          }
        } catch (error) {
          if (!(error instanceof vscode.CancellationError)) void vscode.window.showErrorMessage(error instanceof Error ? error.message : String(error));
        } finally {
          this.inspectionCancellation?.dispose();
          this.inspectionCancellation = undefined;
          this.cancelStatus.hide();
        }
      })),
      vscode.commands.registerCommand('gazette.exportEditorInspection', async () => {
        const inspection = this.inspection;
        const editor = vscode.window.activeTextEditor;
        if (!inspection || editor?.document.uri.toString() !== inspection.receipt.document) return;
        try {
          const destination = await vscode.window.showSaveDialog({title: 'Export Gazette inspection receipt',
            defaultUri: vscode.Uri.file(join(tmpdir(), `gazette-inspection-${Date.now()}.json`)), filters: {'Gazette receipt': ['json']}});
          if (!destination) return;
          if (destination.scheme !== 'file') throw new Error('Choose a local file for the inspection receipt.');
          if (!await inspection.current() || this.inspection !== inspection || vscode.window.activeTextEditor !== editor) {
            throw new Error('The current editor changed; inspect it again before exporting.');
          }
          await writeFile(destination.fsPath, JSON.stringify(inspection.receipt, null, 2) + '\n',
            {encoding: 'utf8', flag: 'wx', mode: 0o600});
        } catch (error) { await vscode.window.showErrorMessage(error instanceof Error ? error.message : String(error)); }
      }),
    ];
    this.schedule();
  }
  private async captureOnce(operation: () => Promise<unknown>): Promise<unknown> {
    if (this.captureRunning) return;
    this.captureRunning = true;
    try { return await operation(); } finally { this.captureRunning = false; }
  }
  private showForSource(selected: boolean) {
    if (selected) this.status.show(); else this.status.hide();
    if (this.activeSource === selected) return;
    this.activeSource = selected;
    void vscode.commands.executeCommand('setContext', 'gazette.activeSource', selected);
  }
  invalidate() { this.schedule(); }
  private schedule() {
    this.sequence++;
    const editor = vscode.window.activeTextEditor;
    const selected = !!(editor && this.owner(editor.document.uri));
    this.showForSource(selected);
    this.items = [{label: selected ? 'Reading page details…' : 'Open a Gazette page or configuration file to see its details.'}];
    this.status.text = '$(sync) Gazette';
    this.changes.fire(undefined);
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => { void this.refresh(); }, 250);
  }
  async refresh() {
    const sequence = ++this.sequence;
    const editor = vscode.window.activeTextEditor;
    const session = editor && this.owner(editor.document.uri);
    this.showForSource(!!session);
    if (!editor || !session) {
      this.items = [{label: 'Open a Gazette page or configuration file to see its details.'}];
      this.changes.fire(undefined); return;
    }
    try {
      const result = await session.context(editor.document, editor.selection.active) as Record<string, unknown> | null;
      if (sequence !== this.sequence) return;
      if (!result) throw new Error('Editor changed during analysis; move the cursor to refresh the current context.');
      const uri = editor.document.uri;
      const fields = (result.fields as {field: {name: string; value?: string; schema?: {description?: string; presence: string; value_kinds: string[]}}; range: {start: vscode.Position; end: vscode.Position}}[]).map(({field, range}) => ({
        label: field.name, description: field.value ?? '', uri,
        range: new vscode.Range(range.start.line, range.start.character, range.end.line, range.end.character),
        children: [{label: 'Source line', value: range.start.line + 1}, ...children(field.schema ?? {meaning: 'Schema detail is unavailable at this source position.'})],
      }));
      this.items = [
        {label: session.folder.name, description: `generation ${result.generation} · ${result.structurally_valid ? 'valid structure' : 'needs repair'}`},
        {label: 'Fields and frontmatter', children: fields.length ? fields : [{label: String(result.structure_status).replaceAll('_', ' ')}]},
        {label: 'Diagnostics', value: result.diagnostics},
        {label: 'Help at cursor', value: result.assistance},
        ...((result.component as {status?: string})?.status === 'available'
          ? [{label: 'Component at cursor', value: (result.component as {value?: unknown}).value}] : []),
        referenceContext(result.navigation),
        {label: 'Page identity and inherited settings', value: result.identity},
        pageContext(result.page_package, editor.document.uri),
      ];
      const inspection = this.inspection;
      if (inspection?.receipt.document === uri.toString() && inspection.receipt.editor_generation === result.generation) {
        if (await inspection.current().catch(() => false) && sequence === this.sequence) {
          this.items.splice(1, 0, inspectionContext(inspection.receipt, editor.document));
        }
        if (sequence !== this.sequence) return;
      }
      this.status.text = `$(book) Gazette: ${result.structurally_valid ? 'ready' : 'needs attention'}`;
      this.status.tooltip = `${session.folder.name} · generation ${result.generation} · click for page details, problems and inherited settings`;
    } catch (error) {
      if (sequence !== this.sequence) return;
      const message = error instanceof Error ? error.message : String(error);
      this.items = [{label: 'Current context unavailable', description: message}];
      this.status.text = '$(warning) Gazette'; this.status.tooltip = message;
    }
    this.changes.fire(undefined);
  }
  getChildren(item?: Item): Item[] { return item ? item.children ?? children(item.value) : this.items; }
  getTreeItem(item: Item): vscode.TreeItem {
    const expandable = item.children !== undefined ? item.children.length > 0 : item.value !== undefined && item.value !== null && typeof item.value === 'object';
    const leaf = !expandable && item.value !== undefined ? `: ${String(item.value)}` : '';
    const result = new vscode.TreeItem(item.label + leaf, expandable ? (item.label === 'Fields and frontmatter' ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.Collapsed) : vscode.TreeItemCollapsibleState.None);
    if (item.description !== undefined) result.description = item.description;
    result.tooltip = item.description ?? item.label;
    if (item.range && item.uri) result.command = {command: 'gazette.revealAuthoringField', title: 'Go to field', arguments: [item.uri, item.range]};
    if (item.command) result.command = item.command;
    return result;
  }
  dispose() { this.showForSource(false); if (this.timer) clearTimeout(this.timer); this.sequence++; this.inspectionCancellation?.cancel(); this.inspectionCancellation?.dispose(); this.cancelStatus.dispose(); this.status.dispose(); this.changes.dispose(); for (const disposable of this.subscriptions) disposable.dispose(); }
}

function referenceContext(value: unknown): Item {
  const reference = value as {status?: string; value?: {uri: string; range: {start: vscode.Position; end: vscode.Position}}};
  if (reference?.status !== 'available' || !reference.value) return {label:'Reference at cursor', children:[{label:
    reference?.status === 'not_applicable' ? 'Place the cursor on a reference to inspect its local source.' : 'Current reference resolution is unavailable; inspect diagnostics.'}]};
  const target = reference.value;
  return {label:'Open referenced source', description:vscode.Uri.parse(target.uri).fsPath,
    command:{command:'gazette.revealAuthoringField',title:'Open referenced source',arguments:[vscode.Uri.parse(target.uri),new vscode.Range(target.range.start.line,target.range.start.character,target.range.end.line,target.range.end.character)]}};
}

/** Present compiler-owned membership as navigable context, not a diagnostic. */
export function pageContext(value: unknown, owner: vscode.Uri): Item {
  const projection = value as {status?: string; value?: {content: string; role: {kind: string}; package: {document: string; companions: {path: string; relative: string; kind: string; source_sha256: string}[]}; resources: unknown[]}};
  if (projection?.status !== 'available' || !projection.value) return {label: 'Page and companions', children: [{label:
    projection?.status === 'not_applicable' ? 'This file has no page-package membership.' : 'Unavailable until the current source selection is valid.'}]};
  const page = projection.value;
  const link = (label: string, path: string, description: string, children?: Item[]): Item => ({label, description, ...(children ? {children} : {}),
    command: {command: 'gazette.revealRetainedSource', title: 'Open retained source', arguments: [owner, path]}});
  return {label: 'Page and companions', description: `${page.package.companions.length} companion${page.package.companions.length === 1 ? '' : 's'}`, children: [
    {label: 'Content identity', value: page.content},
    {label: 'This file', value: page.role.kind === 'document' ? 'Owns this page' : 'Companion of this page'},
    link('Page source', page.package.document, page.package.document),
    ...(page.package.companions.length ? page.package.companions.map(companion => link(companion.relative, companion.path,
      companion.kind.replaceAll('-', ' '), [{label: 'Retained source hash', value: companion.source_sha256}])) : [{label: 'No companion files in this retained selection.'}]),
    {label: 'Compiled resources', value: page.resources},
  ]};
}
