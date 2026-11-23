import { isAbsolute, join } from 'node:path';
import { accessSync, constants, statSync } from 'node:fs';
import type * as vscode from 'vscode';
import type { DocumentSelector, Executable } from 'vscode-languageclient/node';

/** Use the installed program unless the host selects an explicit User override. */
export function executable(workspace: typeof vscode.workspace, folder: vscode.WorkspaceFolder): string {
  if (!workspace.isTrusted) throw new Error('Gazette requires a trusted workspace.');
  if (!folder || folder.uri.scheme !== 'file' || !workspace.workspaceFolders?.some(value => value.uri.toString() === folder.uri.toString())) {
    throw new Error('Gazette requires a local file workspace.');
  }
  const setting = workspace.getConfiguration('gazette', folder.uri).inspect('server.path');
  if (setting?.workspaceValue !== undefined || setting?.workspaceFolderValue !== undefined
      || setting?.workspaceLanguageValue !== undefined || setting?.workspaceFolderLanguageValue !== undefined) {
    throw new Error('Gazette executable settings cannot come from a workspace.');
  }
  const value = setting?.globalValue;
  if (value === undefined || value === '') {
    const bundled = join(__dirname, '..', 'server', process.platform === 'win32' ? 'gazette.exe' : 'gazette');
    try {
      if (!statSync(bundled).isFile()) throw new Error('not a file');
      accessSync(bundled, constants.X_OK);
      return bundled;
    } catch {
      throw new Error('This extension has no runnable bundled Gazette program. Install the package for this host, or set gazette.server.path to an absolute executable in user settings.');
    }
  }
  if (typeof value !== 'string' || !isAbsolute(value) || value.includes('\0')) {
    throw new Error('Set gazette.server.path to an exact absolute executable in user settings.');
  }
  return value;
}

/** Admit literal relative names before any native read; Rust owns EntryPath/role meaning. */
export function sourcePath(value: unknown): string {
  if (typeof value !== 'string' || /[\\\p{Cc}]/u.test(value)
      || value.split('/').some(part => part === '' || part === '.' || part === '..')) {
    throw new Error('Select an exact nonempty folder-relative source path.');
  }
  return value;
}

export const SOURCE_COUNT = 4096;
export interface RootSelection { id: string; uri: string }
export interface FileSelection { root: string; file: string; path: string }
export interface PackageBaselineSelection extends FileSelection { kind: 'theme' | 'plugin'; package: string }
export interface ManualSelection extends FileSelection { source: string }
export interface SourceSelection {
  roots: RootSelection[];
  configuration: FileSelection;
  configurationDocuments: FileSelection[];
  documents: FileSelection[];
  manuals: ManualSelection[];
  pagePackageInputs: FileSelection[];
  packageBaselines: PackageBaselineSelection[];
}

function object(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).some(key => !keys.includes(key))) {
    throw new Error('Gazette selected-source fields must be exact named objects.');
  }
  return value as Record<string, unknown>;
}

function identity(value: unknown): string {
  if (typeof value !== 'string' || value.length === 0 || /\p{Cc}/u.test(value)) {
    throw new Error('Gazette selected identities must be nonempty strings.');
  }
  return value;
}

/** Source locators are transport data, not another Gazette configuration parser. */
export function sourceSelection(workspace: typeof vscode.workspace, folder: vscode.WorkspaceFolder): SourceSelection {
  const settings = workspace.getConfiguration('gazette', folder.uri);
  const array = (key: string): unknown[] => {
    const value = settings.get<unknown>(key, []);
    if (!Array.isArray(value) || value.length > SOURCE_COUNT) {
      throw new Error('Gazette sources require an explicit bounded selection array.');
    }
    return value;
  };
  const roots = array('workspace.roots').map(value => {
    const root = object(value, ['id', 'uri']);
    return { id: identity(root.id), uri: identity(root.uri) };
  });
  roots.unshift({ id: 'workspace', uri: folder.uri.toString() });
  if (roots.length > SOURCE_COUNT || new Set(roots.map(root => root.id)).size !== roots.length
      || new Set(roots.map(root => root.uri)).size !== roots.length) {
    throw new Error('Gazette selected root identities and URIs must be unique and bounded.');
  }
  const local = (value: unknown): FileSelection => {
    const file = sourcePath(value);
    return { root: 'workspace', file, path: file };
  };
  const file = (value: unknown): FileSelection => {
    if (typeof value === 'string') return local(value);
    const input = object(value, ['root', 'file', 'path']);
    return { root: identity(input.root), file: sourcePath(input.file), path: sourcePath(input.path) };
  };
  const roleFile = (input: Record<string, unknown>): FileSelection => {
    const entry = sourcePath(input.file);
    return { root: input.root === undefined ? 'workspace' : identity(input.root), file: entry,
      path: input.path === undefined ? entry : sourcePath(input.path) };
  };
  const configuration = local(settings.get<unknown>('workspace.configuration', 'gazette.toml'));
  const configurationDocuments = array('workspace.configurationDocuments').map(local);
  const documents = array('workspace.documents').map(file);
  const manuals = array('workspace.manuals').map(value => {
    const input = object(value, ['source', 'root', 'file', 'path']);
    return { ...roleFile(input), source: identity(input.source) };
  });
  const pagePackageInputs = array('workspace.pagePackageInputs').map(file);
  const packageBaselines: PackageBaselineSelection[] = array('workspace.packageBaselines').map(value => {
    const input = object(value, ['kind', 'package', 'root', 'file', 'path']);
    if (input.kind !== 'theme' && input.kind !== 'plugin') throw new Error('Gazette package baseline kind must be theme or plugin.');
    return { ...roleFile(input), kind: input.kind, package: identity(input.package) };
  });
  if (new Set(packageBaselines.map(value => `${value.kind}\0${value.package}`)).size !== packageBaselines.length) {
    throw new Error('Gazette typed package baseline identities must be unique.');
  }
  const selected = [configuration, ...configurationDocuments, ...documents, ...manuals, ...pagePackageInputs, ...packageBaselines];
  if (selected.length > SOURCE_COUNT) throw new Error('Gazette selected-source transfer count exceeded.');
  const rootIds = new Set(roots.map(root => root.id));
  if (selected.some(source => !rootIds.has(source.root))
      || new Set(selected.map(source => `${source.root}\0${source.file}`)).size !== selected.length) {
    throw new Error('Gazette selected files require known roots and unique physical identities across roles.');
  }
  // Manuals have native source-qualified identities; Rust owns that qualification.
  const logical = [configuration, ...configurationDocuments, ...documents, ...pagePackageInputs, ...packageBaselines];
  if (new Set(logical.map(source => source.path)).size !== logical.length
      || new Set(manuals.map(source => `${source.source}\0${source.path}`)).size !== manuals.length) {
    throw new Error('Gazette selected logical identities must be unique.');
  }
  return { roots, configuration, configurationDocuments, documents, manuals, pagePackageInputs, packageBaselines };
}

/** Literal paths shared by document selection and exact source watches. */
export function literalGlob(path: string): string { return path.replace(/[?*\[\]{}]/g, value => `[${value}]`); }

export function documentSelector(uris: readonly vscode.Uri[]): DocumentSelector {
  return uris.map(uri => ({ scheme: 'file', pattern: literalGlob(uri.fsPath.replaceAll('\\', '/')) }));
}

/** An Executable's omitted transport means stdio without an injected --stdio argument. */
export function serverOptions(workspace: typeof vscode.workspace, folder: vscode.WorkspaceFolder): Executable {
  return { command: executable(workspace, folder), args: ['lsp'], options: { shell: false, cwd: folder.uri.fsPath } };
}
