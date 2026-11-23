import type * as vscode from 'vscode';
import {createHash} from 'node:crypto';
import {constants} from 'node:fs';
import {open, realpath} from 'node:fs/promises';
import {isAbsolute, join, resolve} from 'node:path';
import {sourceSelection} from './settings';

/** A fixed operator-facing reason, without native path or receipt contents. */
export class BuildInputError extends Error {}

type Origin = {source: string; entry: string; sha256: string};
type Resource = {
  resource: {owner: {kind: string; package: string}; resource: string};
  selection: {scope: string; field: {kind: string}};
  recipe: {entry?: string};
  output: string;
  output_sha256: string;
  url: string;
  input_origins: Record<string, Origin>;
};
type Source = {
  location: {kind: string; root: string; subdirectory?: string};
};
type Report = {
  status: string;
  compilation: {workspace: {sources: Record<string, Source>}};
  sources: Record<string, {observation?: {entries: {path: string; kind: string; bytes?: number; sha256?: string}[]}}>;
  sites: Record<string, {evidence?: {theme: {theme: string}; resources: Record<string, Resource>}}>;
};

function relativeEntry(value: string): boolean {
  return !!value && !value.includes('\\') && !value.includes('\0')
    && value.split('/').every(part => part !== '' && part !== '.' && part !== '..');
}

/** Present the existing private render evidence and open one exact local input. */
export async function openBuildInput(host: typeof vscode, folder: vscode.WorkspaceFolder, raw: string,
    current: () => void): Promise<void> {
  const report = JSON.parse(raw).result?.structuredContent as Report | undefined;
  if (!report || report.status !== 'local_set_rendered') throw new BuildInputError('A successful local render receipt is required.');
  const site = await host.window.showQuickPick(Object.entries(report.sites).flatMap(([label, {evidence}]) => evidence
    ? [{label, description: evidence.theme.theme, evidence}] : []), {title: 'Gazette build inputs · publication'});
  if (!site) return;
  const output = await host.window.showQuickPick(Object.values(site.evidence.resources).map(resource => ({
    label: resource.output, description: `${resource.resource.owner.package} · ${resource.resource.resource}`,
    detail: `${resource.selection.scope} · ${resource.url} · ${resource.output_sha256}`, resource,
  })), {title: 'Gazette build inputs · compiled resource', matchOnDescription: true});
  if (!output) return;
  const input = await host.window.showQuickPick(Object.entries(output.resource.input_origins).map(([virtual, origin]) => ({
    label: origin.entry, description: virtual === output.resource.recipe.entry ? `Entry · ${origin.source}` : origin.source,
    detail: `${virtual} · ${origin.sha256}`, origin,
  })), {title: 'Gazette build inputs · open exact local input', matchOnDescription: true});
  if (!input) return;
  current();
  const uri = await verifiedBuildInput(host, folder, report, input.origin);
  current();
  await host.commands.executeCommand('vscode.open', uri);
  void host.window.showInformationMessage(`Verified Gazette build input for ${output.resource.output} · ${input.origin.source}`);
}

async function verifiedBuildInput(host: typeof vscode, folder: vscode.WorkspaceFolder, report: Report, origin: Origin) {
  const location = report.compilation.workspace.sources[origin.source]?.location;
  const entry = report.sources[origin.source]?.observation?.entries.find(entry =>
    entry.path === origin.entry && entry.kind === 'file' && entry.sha256 === origin.sha256);
  if (!host.workspace.isTrusted || !location || !['filesystem', 'filtered-filesystem'].includes(location.kind)
      || !isAbsolute(location.root) || (location.subdirectory && !relativeEntry(location.subdirectory))
      || !relativeEntry(origin.entry) || !entry || !Number.isSafeInteger(entry.bytes) || entry.bytes! < 0
      || entry.bytes! > 64 * 1024 * 1024) {
    throw new BuildInputError('The receipt does not identify a bounded local build input in the trusted workspace.');
  }
  const root = host.Uri.file(resolve(location.root, location.subdirectory ?? ''));
  const admitted = sourceSelection(host.workspace, folder).roots.some(selected => selected.uri === root.toString());
  if (!admitted || !host.workspace.workspaceFolders?.some(owner => owner.uri.toString() === root.toString())) {
    throw new BuildInputError('Open the exact Gazette-selected build input folder in this trusted workspace first.');
  }
  const path = join(root.fsPath, origin.entry);
  const uri = host.Uri.file(path);
  if (host.workspace.getWorkspaceFolder(uri)?.uri.toString() !== root.toString()
      || await realpath(path) !== join(await realpath(root.fsPath), origin.entry)) {
    throw new BuildInputError('The build input no longer resolves within its selected root.');
  }
  const editor = host.workspace.textDocuments.find(document => document.uri.toString() === uri.toString());
  if (editor?.isDirty && createHash('sha256').update(editor.getText(), 'utf8').digest('hex') !== origin.sha256) {
    throw new BuildInputError('The open editor has unsaved changes to this build input. Save and rebuild to verify those bytes.');
  }
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.size !== entry.bytes) throw changed();
    const bytes = Buffer.alloc(stat.size + 1);
    let used = 0;
    while (used < bytes.length) {
      const {bytesRead} = await file.read(bytes, used, bytes.length - used, used);
      if (!bytesRead) break;
      used += bytesRead;
    }
    if (used !== stat.size || createHash('sha256').update(bytes.subarray(0, used)).digest('hex') !== origin.sha256) {
      throw changed();
    }
  } finally {
    await file.close();
  }
  return uri;
}

function changed(): Error {
  return new BuildInputError('The local build input differs from the retained receipt. Rebuild before opening it as a verified input.');
}
