import { createHash } from 'node:crypto';
import { TextDecoder } from 'node:util';

interface Source {
  id: string; path: string; version: number | null; bytes: number; source_sha256: string;
  baseline_sha256?: string;
  role: { kind: string; source?: string; package?: string };
}
interface Manifest { generation: number; structurally_valid: boolean; modular: boolean; sources: Source[]; asset_observations?: Record<string, unknown> }
interface Transport { sendRequest<R>(method: string, params: unknown): Promise<R> }
export interface EditorSnapshot {
  generation: number;
  manifest: Manifest;
  start: Record<string, unknown>;
  overlays: { id: string; version: number; text: string }[];
  nativeSelection?: Record<string, unknown>;
  current(): Promise<boolean>;
}

/** Explicit portable handoff; never substitutes disk content for the editor. */
export async function editorContext(snapshot: EditorSnapshot, workspace: string, document: string, offset: number) {
  const active = snapshot.manifest.sources.find(source => source.id === document && source.role.kind !== 'page_input');
  if (!active || !Number.isSafeInteger(offset) || offset < 0 || offset > active.bytes) throw new Error('The active editor is not in this snapshot.');
  if (!await snapshot.current()) throw new Error('Editor content changed before export; capture the current content again.');
  return {format: snapshot.nativeSelection ? 'gazette-editor-context/2' : 'gazette-editor-context/1', workspace, generation: snapshot.generation,
    active: {document, offset, version: active.version, source_sha256: active.source_sha256},
    manifest: snapshot.manifest, ...(snapshot.nativeSelection ? {native_selection: snapshot.nativeSelection} : {start: snapshot.start}), overlays: snapshot.overlays};
}

export async function exportEditorContext(snapshot: EditorSnapshot, workspace: string, document: string, offset: number): Promise<string> {
  return JSON.stringify(await editorContext(snapshot, workspace, document, offset), null, 2) + '\n';
}

/** Copy the native retained selection, never rediscover or reinterpret source files. */
export async function captureEditorSnapshot(client: Transport, progress?: (completed: number, total: number) => void, requireNative = false): Promise<EditorSnapshot> {
  const {native_selection: nativeSelection, ...manifest} = await client.sendRequest<Manifest & {native_selection?: Record<string, unknown>}>('gazette/editorSnapshot', {});
  if (nativeSelection !== undefined && (!nativeSelection || typeof nativeSelection !== 'object' || Array.isArray(nativeSelection))) {
    throw new Error('Invalid native editor selection. No partial selection was sent.');
  }
  if (requireNative && !nativeSelection) throw new Error('The language server cannot provide an unsaved-only editor handoff. Use a Gazette build that supports native editor context version 2.');
  const start: Record<string, unknown> = { documents: [], manuals: [], page_inputs: [], package_baselines: [] };
  if (manifest.modular) start.workspace_documents = [];
  const overlays: EditorSnapshot['overlays'] = [];
  let total = 0;
  let completed = 0;
  const transferred = nativeSelection ? manifest.sources.filter(source => {
    if (!source.baseline_sha256 || !/^[a-f0-9]{64}$/.test(source.baseline_sha256)) throw new Error('Native editor baseline identity is missing.');
    const dirty = source.baseline_sha256 !== source.source_sha256;
    if (dirty && (source.version === null || source.role.kind === 'page_input')) throw new Error('Native editor source changed without an editable overlay.');
    return dirty;
  }) : manifest.sources;
  progress?.(0, transferred.length);
  for (const source of transferred) {
    if (!Number.isSafeInteger(source.bytes) || source.bytes < 0 || (total += source.bytes) > 12 * 1024 * 1024) {
      throw new Error('The editor text to transfer exceeds the MCP handoff limit. No partial selection was sent.');
    }
    const chunks: Buffer[] = [];
    for (let offset = 0; offset < source.bytes;) {
      const chunk = await client.sendRequest<{generation: number; document: string; offset: number; bytes_base64: string}>(
        'gazette/editorSource', {generation: manifest.generation, document: source.id, offset, length: 65536});
      const bytes = Buffer.from(chunk.bytes_base64, 'base64');
      if (chunk.generation !== manifest.generation || chunk.document !== source.id || chunk.offset !== offset
          || !bytes.length || bytes.length > Math.min(65536, source.bytes - offset)
          || bytes.toString('base64') !== chunk.bytes_base64) throw new Error('Incomplete native editor snapshot.');
      chunks.push(bytes); offset += bytes.length;
    }
    const bytes = Buffer.concat(chunks);
    if (createHash('sha256').update(bytes).digest('hex') !== source.source_sha256) throw new Error('Editor source digest mismatch.');
    const base = {id: source.id, path: source.path};
    if (source.role.kind === 'page_input') {
      (start.page_inputs as unknown[]).push({...base, bytes_base64: bytes.toString('base64')});
      progress?.(++completed, transferred.length);
      continue;
    }
    const text = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(bytes);
    if (nativeSelection) {
      overlays.push({id: source.id, version: source.version!, text});
      progress?.(++completed, transferred.length);
      continue;
    }
    const input = {...base, text};
    switch (source.role.kind) {
      case 'workspace': if (start.workspace) throw new Error('Duplicate native configuration root.'); start.workspace = input; break;
      case 'workspace_document': (start.workspace_documents as unknown[]).push(input); break;
      case 'document': (start.documents as unknown[]).push(input); break;
      case 'manual': (start.manuals as unknown[]).push({...input, source: source.role.source}); break;
      case 'theme': case 'plugin': (start.package_baselines as unknown[]).push({...input, kind: source.role.kind, package: source.role.package}); break;
      default: throw new Error('Unsupported native editor source role.');
    }
    if (source.version !== null) overlays.push({id: source.id, version: source.version, text});
    progress?.(++completed, transferred.length);
  }
  if (!nativeSelection && !start.workspace) throw new Error('The native editor snapshot has no configuration root.');
  const current = async () => (await client.sendRequest<Manifest>('gazette/editorSnapshot', {})).generation === manifest.generation;
  if (!await current()) throw new Error('Editor content changed during snapshot capture.');
  return {generation: manifest.generation, manifest, start, overlays, current, ...(nativeSelection ? {nativeSelection} : {})};
}
