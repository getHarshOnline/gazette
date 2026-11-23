import type * as VSCode from 'vscode';
import { sourceSelection, sourcePath, SOURCE_COUNT, type FileSelection } from './settings';

export { SOURCE_COUNT } from './settings';
export class MissingSourceRoots extends Error {
  constructor() { super('Gazette selected roots must be explicitly opened native folders. Open the generated .code-workspace file for this publication to load its owner folders.'); }
}
export interface NativeLocator { root: string; entry: string; path: string }
export interface CapturedSources {
  discovery?: true;
  uris: VSCode.Uri[];
  owners: ReadonlyMap<string, string>;
  configurationUris: ReadonlySet<string>;
  rootUris: readonly string[];
  initializationOptions: {
    native_selection: {
      allow_read: true;
      discover: true;
      workspace: string;
      manuals: {source: string; path: string}[];
    } | {
      allow_read: true;
      roots: { id: string; uri: string }[];
      workspace: NativeLocator;
      workspace_documents: NativeLocator[];
      documents: NativeLocator[];
      manuals: (NativeLocator & { source: string })[];
      page_inputs: NativeLocator[];
      package_baselines: (NativeLocator & { kind: 'theme' | 'plugin'; package: string })[];
    };
  };
}

// Locators only: preserve headroom in the unchanged native 1 MiB JSON frame.
export const TRANSFER_BYTES = 512 * 1024;

/** Select exact native files. Native capture alone owns byte retention and safety checks. */
export async function captureSources(vscode: typeof VSCode, folder: VSCode.WorkspaceFolder): Promise<CapturedSources | undefined> {
  if (!vscode.workspace.isTrusted) throw new Error('Gazette requires a trusted workspace.');
  const settings = vscode.workspace.getConfiguration('gazette', folder.uri);
  const explicit = ['roots', 'configurationDocuments', 'documents', 'manuals', 'pagePackageInputs', 'packageBaselines']
    .some(key => { const value = settings.get<unknown>(`workspace.${key}`, []); return !Array.isArray(value) || value.length > 0; });
  if (settings.get<boolean>('workspace.discovery', true) && !explicit) {
    if (folder.uri.scheme !== 'file' || folder.uri.query || folder.uri.fragment) return undefined;
    const workspace = sourcePath(settings.get<unknown>('workspace.configuration', 'gazette.toml'));
    const uri = vscode.Uri.joinPath(folder.uri, ...workspace.split('/'));
    try { await vscode.workspace.fs.stat(uri); }
    catch (error) {
      if (error && typeof error === 'object' && 'code' in error && error.code === 'FileNotFound') return undefined;
      throw error;
    }
    const support = settings.get<unknown>('workspace.manualSupport', []);
    if (!Array.isArray(support) || support.length > SOURCE_COUNT) throw new Error('Manual support requires a bounded list of exact files.');
    const manuals = support.map(value => {
      if (!value || typeof value !== 'object' || Array.isArray(value)
          || Object.keys(value).some(key => !['source', 'path'].includes(key))
          || typeof value.source !== 'string' || !value.source || /\p{Cc}/u.test(value.source)) {
        throw new Error('Each manual support file needs its configured source and relative path.');
      }
      return {source: value.source, path: sourcePath(value.path)};
    });
    const initializationOptions: CapturedSources['initializationOptions'] = {
      native_selection: {allow_read: true, discover: true, workspace, manuals},
    };
    if (Buffer.byteLength(JSON.stringify(initializationOptions), 'utf8') > TRANSFER_BYTES) throw new Error('Gazette initialization transfer size exceeded.');
    return {discovery: true, uris: [uri], owners: new Map([[uri.toString(), folder.uri.toString()]]),
      configurationUris: new Set([uri.toString()]), rootUris: [folder.uri.toString()], initializationOptions};
  }
  const selected = sourceSelection(vscode.workspace, folder);
  const roots = new Map(selected.roots.map(root => {
    const owner = vscode.workspace.workspaceFolders?.find(value => value.uri.toString() === root.uri);
    if (!owner || owner.uri.scheme !== 'file' || owner.uri.query || owner.uri.fragment) {
      throw new MissingSourceRoots();
    }
    return [root.id, owner.uri] as const;
  }));
  const uris: VSCode.Uri[] = [];
  const owners = new Map<string, string>();
  const configurationUris = new Set<string>();
  const locator = (source: FileSelection, text = true, configuration = false): NativeLocator => {
    const root = roots.get(source.root);
    if (!root) throw new Error('Gazette selected source requires a declared root.');
    const uri = vscode.Uri.joinPath(root, ...source.file.split('/'));
    const identity = uri.toString();
    const owner = vscode.workspace.getWorkspaceFolder(uri);
    const prefix = root.path.replace(/\/$/, '') + '/';
    if (uri.scheme !== 'file' || uri.authority !== root.authority || !uri.path.startsWith(prefix)
        || uri.query || uri.fragment || owner?.uri.toString() !== root.toString() || owners.has(identity)) {
      throw new Error('Gazette source selection is ambiguous or outside its explicit root.');
    }
    owners.set(identity, root.toString());
    if (configuration) configurationUris.add(identity);
    if (text) uris.push(uri);
    return { root: source.root, entry: source.file, path: source.path };
  };
  const initializationOptions: CapturedSources['initializationOptions'] = { native_selection: {
    allow_read: true,
    roots: selected.roots,
    workspace: locator(selected.configuration, true, true),
    workspace_documents: selected.configurationDocuments.map(source => locator(source, true, true)),
    documents: selected.documents.map(source => locator(source)),
    manuals: selected.manuals.map(source => ({ ...locator(source), source: source.source })),
    page_inputs: selected.pagePackageInputs.map(source => locator(source, false)),
    package_baselines: selected.packageBaselines.map(source => ({ ...locator(source, true, true), kind: source.kind, package: source.package })),
  } };
  if (owners.size > SOURCE_COUNT || Buffer.byteLength(JSON.stringify(initializationOptions), 'utf8') > TRANSFER_BYTES) {
    throw new Error('Gazette initialization locator transfer size exceeded.');
  }
  return { uris, owners, configurationUris, rootUris: selected.roots.map(root => root.uri), initializationOptions };
}
