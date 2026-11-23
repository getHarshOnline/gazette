import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { isAbsolute } from 'node:path';
import { TextDecoder } from 'node:util';

/** Parsed discovery is presentation-only; raw retains the exact native schemas. */
export interface ToolAnnotations {
  readonly readOnlyHint: boolean;
  readonly destructiveHint: boolean;
  readonly idempotentHint: boolean;
  readonly openWorldHint: boolean;
}

export type ToolCapability = 'authoring' | 'build' | 'documentation' | 'observation'
  | 'preview' | 'recovery' | 'release' | 'store' | 'workspace';
export type ToolIntent = 'inspect' | 'act' | 'observe' | 'verify' | 'reconcile' | 'recover';
export type ToolEffect = 'read' | 'local' | 'external';
export type ToolPotentialEffect = 'memory-state' | 'filesystem-read' | 'filesystem-write'
  | 'sqlite' | 'process' | 'git' | 'listener' | 'network';
export type ToolProminence = 'common' | 'advanced';
export type ToolClosure = 'none' | 'related' | 'terminal-owner' | 'self-terminal';
export type ToolRelationKind = 'inspect' | 'verify' | 'observe' | 'reconcile' | 'recover' | 'contrast';

export interface ToolRelation {
  readonly kind: ToolRelationKind;
  readonly operation: string;
}

/** Exact presentation metadata projected by the native operation owner. */
export interface ToolMetadata {
  readonly 'io.rtepl.gazette/capability': ToolCapability;
  readonly 'io.rtepl.gazette/intent': ToolIntent;
  readonly 'io.rtepl.gazette/effect': ToolEffect;
  readonly 'io.rtepl.gazette/potentialEffects': readonly ToolPotentialEffect[];
  readonly 'io.rtepl.gazette/prominence': ToolProminence;
  readonly 'io.rtepl.gazette/closure': ToolClosure;
  readonly 'io.rtepl.gazette/relations'?: readonly ToolRelation[];
  readonly 'io.rtepl.gazette/terminalResultOwner'?: string;
}

export interface ToolDescriptor {
  readonly name: string;
  readonly title: string;
  readonly description?: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
  readonly outputSchema: Readonly<Record<string, unknown>>;
  readonly annotations: ToolAnnotations;
  readonly _meta: ToolMetadata;
}

export interface ApplicationResult { readonly raw: string; readonly isError: boolean }

/** RPC failures retain private native evidence without copying it into the message. */
export class ApplicationError extends Error {
  constructor(message: string, readonly effect: 'not-dispatched' | 'unknown', readonly raw?: string) {
    super(message);
    this.name = 'ApplicationError';
  }
}

type Reply = { raw: string; result: Record<string, unknown> };
type RequestMethod = 'tools/list' | 'tools/call';
type Pending = {
  id: number;
  method: RequestMethod;
  resolve: (reply: Reply) => void;
  reject: (error: ApplicationError) => void;
};
const REQUEST_BYTES = 16 * 1024 * 1024;
const RESPONSE_BYTES = 8 * 1024 * 1024;
const DISCOVERY_PAGE_TOOLS = 8;
const DISCOVERY_PAGES = 32;
const DISCOVERY_TOOLS = 256;
const DISCOVERY_BYTES = 16 * 1024 * 1024;
const CAPABILITIES: readonly ToolCapability[] = [
  'workspace', 'authoring', 'build', 'preview', 'documentation',
  'store', 'release', 'observation', 'recovery',
];
const META = JSON.stringify({
  'io.modelcontextprotocol/protocolVersion': '2026-07-28',
  'io.modelcontextprotocol/clientCapabilities': {},
});
// Native unfiltered discovery deliberately returns one complete catalog. Ask
// explicitly for its capability-paged contract; every page keeps the selection.
const DISCOVERY_META = JSON.stringify({
  ...JSON.parse(META),
  'io.rtepl.gazette/discovery': { capabilities: CAPABILITIES },
});

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function dispatchEffect(error: Record<string, unknown>): 'not-dispatched' | 'unknown' {
  const data = error.data;
  return object(data) && data['io.rtepl.gazette/dispatch'] === 'not-dispatched'
    ? 'not-dispatched'
    : 'unknown';
}

function annotations(value: unknown): value is ToolAnnotations {
  return object(value)
    && typeof value.readOnlyHint === 'boolean'
    && typeof value.destructiveHint === 'boolean'
    && typeof value.idempotentHint === 'boolean'
    && typeof value.openWorldHint === 'boolean';
}

function oneOf(value: unknown, values: readonly string[]): value is string {
  return typeof value === 'string' && values.includes(value);
}

function operationMetadata(value: unknown): value is ToolMetadata {
  const potentialEffectOrder = [
    'memory-state', 'filesystem-read', 'filesystem-write', 'sqlite',
    'process', 'git', 'listener', 'network',
  ] as const;
  if (!object(value)
      || !oneOf(value['io.rtepl.gazette/capability'], CAPABILITIES)
      || !oneOf(value['io.rtepl.gazette/intent'], ['inspect', 'act', 'observe', 'verify', 'reconcile', 'recover'])
      || !oneOf(value['io.rtepl.gazette/effect'], ['read', 'local', 'external'])
      || !Array.isArray(value['io.rtepl.gazette/potentialEffects'])
      || value['io.rtepl.gazette/potentialEffects'].some(effect => !oneOf(effect, potentialEffectOrder))
      || value['io.rtepl.gazette/potentialEffects'].some((effect, index, effects) =>
        index > 0 && potentialEffectOrder.indexOf(effect) <= potentialEffectOrder.indexOf(effects[index - 1]!))
      || !oneOf(value['io.rtepl.gazette/prominence'], ['common', 'advanced'])
      || !oneOf(value['io.rtepl.gazette/closure'], ['none', 'related', 'terminal-owner', 'self-terminal'])) return false;
  const relations = value['io.rtepl.gazette/relations'];
  const seen = new Set<string>();
  if (relations !== undefined && (!Array.isArray(relations) || !relations.length || !relations.every(relation => {
    if (!object(relation)
        || !oneOf(relation.kind, ['inspect', 'verify', 'observe', 'reconcile', 'recover', 'contrast'])
        || typeof relation.operation !== 'string' || !relation.operation) return false;
    const key = `${relation.kind}\0${relation.operation}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  }))) return false;
  const terminal = value['io.rtepl.gazette/terminalResultOwner'];
  if (terminal !== undefined && (typeof terminal !== 'string' || !terminal)) return false;
  const closure = value['io.rtepl.gazette/closure'];
  const related = Array.isArray(relations) && relations.length > 0;
  return closure === 'terminal-owner' ? terminal !== undefined
    : closure === 'related' ? related && terminal === undefined
      : !related && terminal === undefined;
}

/** Validate the coarse compatibility class, orthogonal facets and native hints together. */
export function coherentEffectProfile(tool: ToolDescriptor): boolean {
  const hints = tool.annotations;
  const effect = tool._meta['io.rtepl.gazette/effect'];
  const potential = tool._meta['io.rtepl.gazette/potentialEffects'];
  const network = potential.includes('network');
  const localEffect = potential.some(value =>
    !['filesystem-read', 'network'].includes(value));
  const mutableState = potential.some(value =>
    ['memory-state', 'filesystem-write', 'sqlite', 'git', 'listener'].includes(value));
  if (hints.destructiveHint && hints.readOnlyHint) return false;
  if (network && !hints.openWorldHint) return false;
  if (mutableState && hints.readOnlyHint) return false;
  if (effect === 'read') return hints.readOnlyHint && !mutableState && !network;
  if (effect === 'local') return !hints.readOnlyHint && localEffect && !network;
  return network;
}

/** JSON syntax is already validated. Only transport whitespace changes, never value tokens. */
function singleLine(json: string): string {
  return json.replace(/"(?:\\[\s\S]|[^"\\])*"|[ \t\r\n]+/g, token => token.startsWith('"') ? token : '');
}

/** One exact executable/process owns all returned handles. No queue, retry or cancellation fiction. */
export class ApplicationSession {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly closed: Promise<void>;
  private pending: Pending | undefined;
  private nextId = 1;
  private retired = false;
  private uncertain = false;
  private stopping = false;
  private exited = false;
  private frame: Buffer | undefined;
  private bytes = 0;

  constructor(executable: string, cwd: string) {
    if (!isAbsolute(executable) || !isAbsolute(cwd) || executable.includes('\0') || cwd.includes('\0')) {
      throw new ApplicationError('Gazette requires exact absolute executable and workspace paths.', 'not-dispatched');
    }
    // Reserve envelope headroom beyond the native tool-result ceiling.
    this.child = spawn(executable, ['mcp', '--max-frame-bytes', String(REQUEST_BYTES), '--max-response-bytes', String(RESPONSE_BYTES - 1024)],
      { cwd, shell: false, windowsHide: true, stdio: 'pipe' });
    this.closed = new Promise(resolve => {
      this.child.once('close', () => {
        this.exited = true;
        this.retire('Gazette process exited. Any unresolved operation has an unknown outcome.');
        resolve();
      });
    });
    this.child.on('error', () => this.retire('Gazette process failed. Any dispatched operation has an unknown outcome.'));
    this.child.stdin.on('error', () => this.retire('Gazette request delivery failed; the operation outcome is unknown.'));
    this.child.stdout.on('error', () => this.retire('Gazette response delivery failed; the operation outcome is unknown.'));
    this.child.stdout.on('data', (chunk: Buffer) => this.receive(chunk));
    this.child.stdout.on('end', () => {
      if (!this.stopping || this.pending || this.bytes) this.retire('Gazette response stream ended without complete operation evidence.');
    });
    // Drain without retaining or logging potentially private native payloads.
    this.child.stderr.resume();
    this.child.stderr.on('error', () => this.retire('Gazette diagnostic stream failed; process custody remains unresolved.'));
  }

  async discover(): Promise<{ tools: readonly ToolDescriptor[]; raw: string }> {
    const tools: ToolDescriptor[] = [];
    const rawPages: string[] = [];
    const names = new Set<string>();
    const cursors = new Set<string>();
    let cursor: string | undefined;
    let cacheScope: string | undefined;
    let aggregateBytes = 0;
    const invalid = (raw?: string): never => {
      this.retire('Gazette returned an invalid or incomplete operation catalog.');
      throw new ApplicationError(
        'Gazette returned an invalid or incomplete operation catalog; no application operation was dispatched.',
        'not-dispatched',
        raw,
      );
    };
    for (let page = 0; ; page++) {
      if (page >= DISCOVERY_PAGES) invalid(rawPages.at(-1));
      const reply = await this.request('tools/list', cursor === undefined ? '' : `,"cursor":${JSON.stringify(cursor)}`);
      rawPages.push(reply.raw);
      aggregateBytes += Buffer.byteLength(reply.raw);
      if (aggregateBytes + rawPages.length + 2 > DISCOVERY_BYTES) invalid(reply.raw);
      if (reply.result.resultType !== 'complete') invalid(reply.raw);
      const ttlMs = reply.result.ttlMs;
      if (typeof ttlMs !== 'number' || !Number.isSafeInteger(ttlMs) || ttlMs < 0) invalid(reply.raw);
      const scopeValue = reply.result.cacheScope;
      if (!oneOf(scopeValue, ['private', 'public'])) invalid(reply.raw);
      const scope = scopeValue as 'private' | 'public';
      if (cacheScope !== undefined && scope !== cacheScope) invalid(reply.raw);
      const pageValue = reply.result.tools;
      if (!Array.isArray(pageValue)) invalid(reply.raw);
      const pageTools = pageValue as unknown[];
      if (pageTools.length === 0
          || pageTools.length > DISCOVERY_PAGE_TOOLS) invalid(reply.raw);
      const nextValue = reply.result.nextCursor;
      if (nextValue !== undefined && (typeof nextValue !== 'string' || !nextValue)) invalid(reply.raw);
      const next = nextValue as string | undefined;
      if (next !== undefined && pageTools.length !== DISCOVERY_PAGE_TOOLS) invalid(reply.raw);
      cacheScope = scope;
      for (const tool of pageTools) {
        if (!object(tool) || typeof tool.name !== 'string' || !tool.name || names.has(tool.name)
            || typeof tool.title !== 'string' || !tool.title
            || (tool.description !== undefined && typeof tool.description !== 'string')
            || !object(tool.inputSchema) || !object(tool.outputSchema) || !annotations(tool.annotations)
            || !operationMetadata(tool._meta)) invalid(reply.raw);
        const descriptor = tool as unknown as ToolDescriptor;
        if (!coherentEffectProfile(descriptor)) invalid(reply.raw);
        names.add(descriptor.name);
        tools.push(descriptor);
        if (tools.length > DISCOVERY_TOOLS) invalid(reply.raw);
      }
      if (next === undefined) break;
      if (cursors.has(next)) invalid(reply.raw);
      cursors.add(next);
      cursor = next;
    }
    if (tools.some(tool => {
      const metadata = (tool as ToolDescriptor)._meta;
      const owner = metadata['io.rtepl.gazette/terminalResultOwner'];
      return (owner !== undefined && !names.has(owner))
        || (metadata['io.rtepl.gazette/relations'] ?? []).some(relation => !names.has(relation.operation));
    })) {
      invalid(rawPages.at(-1));
    }
    return { tools, raw: rawPages.length === 1 ? (rawPages[0] ?? invalid()) : `[\n${rawPages.join(',\n')}\n]` };
  }

  async call(name: string, argumentsJson: string): Promise<ApplicationResult> {
    if (typeof name !== 'string' || !name || typeof argumentsJson !== 'string'
        || Buffer.byteLength(name) + Buffer.byteLength(argumentsJson) > REQUEST_BYTES) {
      throw new ApplicationError('Use a bounded operation name and JSON object; no request was dispatched.', 'not-dispatched');
    }
    try {
      // Validate framing only. Never serialize this parsed value: native numbers and duplicate keys stay intact.
      if (!object(JSON.parse(argumentsJson))) throw new Error('object required');
    } catch {
      throw new ApplicationError('Operation arguments must be a JSON object; no request was dispatched.', 'not-dispatched');
    }
    const reply = await this.request('tools/call', `,"name":${JSON.stringify(name)},"arguments":${singleLine(argumentsJson)}`);
    if (!Array.isArray(reply.result.content) || (reply.result.isError !== undefined && typeof reply.result.isError !== 'boolean')) {
      this.uncertain = true;
      this.retire('Gazette returned an invalid tool outcome.');
      throw new ApplicationError('Gazette returned an invalid tool outcome; the effect is unknown.', 'unknown', reply.raw);
    }
    return { raw: reply.raw, isError: reply.result.isError === true };
  }

  private request(method: RequestMethod, fields: string): Promise<Reply> {
    if (this.retired || this.stopping || this.exited) return Promise.reject(new ApplicationError('Gazette session is unavailable; do not retry unresolved effects.', 'not-dispatched'));
    if (this.pending) return Promise.reject(new ApplicationError('Gazette already has an in-flight operation; no request was queued.', 'not-dispatched'));
    if (!Number.isSafeInteger(this.nextId)) return Promise.reject(new ApplicationError('Gazette request identity is exhausted; explicitly close this session.', 'not-dispatched'));
    const id = this.nextId++;
    const meta = method === 'tools/list' ? DISCOVERY_META : META;
    const frame = `{"jsonrpc":"2.0","id":${id},"method":${JSON.stringify(method)},"params":{"_meta":${meta}${fields}}}\n`;
    const bytes = Buffer.from(frame, 'utf8');
    if (bytes.length > REQUEST_BYTES || bytes.toString('utf8') !== frame) {
      return Promise.reject(new ApplicationError('Gazette request exceeds the byte bound or contains invalid Unicode; no request was dispatched.', 'not-dispatched'));
    }
    return new Promise((resolve, reject) => {
      this.pending = { id, method, resolve, reject };
      try {
        this.child.stdin.write(bytes, error => {
          if (error) this.retire('Gazette request delivery failed; the operation outcome is unknown.');
        });
      } catch { this.retire('Gazette request delivery failed; the operation outcome is unknown.'); }
    });
  }

  private receive(chunk: Buffer): void {
    if (this.retired) return;
    let start = 0;
    while (start < chunk.length && !this.retired) {
      if (!this.pending) { this.retire('Gazette sent an unsolicited response; session integrity is unproven.'); return; }
      const newline = chunk.indexOf(10, start);
      const end = newline < 0 ? chunk.length : newline;
      const part = chunk.subarray(start, end);
      if (this.bytes + part.length > RESPONSE_BYTES) { this.retire('Gazette response exceeded its byte bound; the operation outcome is unknown.'); return; }
      // One capped allocation regardless of fragment count. Only the written prefix is decoded.
      if (!this.frame) this.frame = Buffer.allocUnsafe(RESPONSE_BYTES);
      part.copy(this.frame, this.bytes);
      this.bytes += part.length;
      if (newline < 0) return;
      const frame = this.frame.subarray(0, this.bytes);
      this.frame = undefined;
      this.bytes = 0;
      this.response(frame);
      start = newline + 1;
    }
  }

  private response(frame: Buffer): void {
    try {
      const raw = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(frame);
      const response: unknown = JSON.parse(raw);
      const pending = this.pending;
      if (!pending || !object(response) || response.jsonrpc !== '2.0'
          || !Number.isSafeInteger(response.id) || response.id !== pending.id
          || Object.hasOwn(response, 'result') === Object.hasOwn(response, 'error')) throw new Error('envelope');
      if (Object.hasOwn(response, 'error')) {
        if (!object(response.error) || !Number.isSafeInteger(response.error.code) || typeof response.error.message !== 'string') throw new Error('error');
        const effect = pending.method === 'tools/list'
          ? 'not-dispatched'
          : dispatchEffect(response.error);
        this.pending = undefined;
        if (pending.method === 'tools/list') {
          this.retired = true;
        } else if (effect === 'unknown') {
          this.uncertain = true;
          this.retired = true;
        }
        pending.reject(new ApplicationError(
          pending.method === 'tools/list'
            ? 'Gazette catalog discovery failed; no application operation was dispatched. Retire this catalog session before rediscovery.'
            : effect === 'not-dispatched'
            ? 'Gazette refused the request before dispatch; inspect its private response before correcting the request.'
            : 'Gazette returned an RPC error without proof that the operation was not dispatched; the outcome is unknown.',
          effect,
          raw,
        ));
      } else {
        if (!object(response.result)) throw new Error('result');
        this.pending = undefined;
        pending.resolve({ raw, result: response.result });
      }
    } catch {
      this.retire('Gazette returned malformed, invalid UTF-8 or mismatched evidence; the operation outcome is unknown.');
    }
  }

  private retire(message: string): void {
    this.retired = true;
    this.frame = undefined;
    this.bytes = 0;
    const pending = this.pending;
    this.pending = undefined;
    if (pending) {
      const effect = pending.method === 'tools/call' ? 'unknown' : 'not-dispatched';
      if (effect === 'unknown') this.uncertain = true;
      pending.reject(new ApplicationError(
        effect === 'unknown'
          ? message
          : 'Gazette catalog discovery failed; no application operation was dispatched. Retire this catalog session before rediscovery.',
        effect,
      ));
    }
  }

  /** EOF is graceful shutdown, not cancellation. An active/uncertain operation is never killed. */
  async shutdown(): Promise<void> {
    if (this.exited) return;
    if (this.pending || this.uncertain) {
      throw new ApplicationError('Gazette has active or uncertain work. No process was terminated; retain this session until exit/read-back is established.', 'unknown');
    }
    this.stopping = true;
    this.child.stdin.end();
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([this.closed, new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new ApplicationError('Gazette process exit is unproven. Retain custody and do not admit a replacement.', 'unknown')), 5000);
      })]);
    } finally { if (timer) clearTimeout(timer); }
  }
}
