const MAX_DEPTH = 32;
const MAX_NODES = 4096;
const MAX_BYTES = 1024 * 1024;

/** Keep native prompt sequences short; larger live schemas use the inert JSON editor. */
export const GUIDED_REQUIRED_FIELD_LIMIT = 8;

type Schema = Readonly<Record<string, unknown>>;

export interface GuidedField {
  readonly path: readonly string[];
  readonly label: string;
  readonly description?: string;
  readonly required: boolean;
  readonly schema: Schema;
}

export interface GuidedRequest {
  readonly value: Readonly<Record<string, unknown>>;
  readonly fields: readonly GuidedField[];
  readonly requiredLeafCount: number;
  readonly defaultMode: 'guided' | 'advanced';
}

function object(value: unknown): value is Schema {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function pointerToken(token: string): string {
  return token.replaceAll('~1', '/').replaceAll('~0', '~');
}

function localReference(root: Schema, reference: string): unknown {
  if (!reference.startsWith('#/')) return undefined;
  let value: unknown = root;
  for (const token of reference.slice(2).split('/').map(pointerToken)) {
    if (!object(value) || !(token in value)) return undefined;
    value = value[token];
  }
  return value;
}

function resolved(schema: unknown, root: Schema, references: ReadonlySet<string>): Schema | undefined {
  if (!object(schema)) return undefined;
  if (typeof schema.$ref !== 'string') return schema;
  if (references.has(schema.$ref)) return undefined;
  const target = localReference(root, schema.$ref);
  return resolved(target, root, new Set([...references, schema.$ref]));
}

function fieldLabel(path: readonly string[]): string {
  return path.join(' › ');
}

function guide(
  schema: unknown,
  root: Schema,
  path: readonly string[],
  required: boolean,
  references: ReadonlySet<string>,
  depth: number,
  budget: { nodes: number },
  base: Record<string, unknown>,
  fields: GuidedField[],
): void {
  budget.nodes++;
  if (budget.nodes > MAX_NODES || fields.length >= 128) {
    throw new Error('Gazette operation schema is too large to guide safely. Use the explicit advanced JSON editor.');
  }
  if (depth > MAX_DEPTH) {
    fields.push({ path, label: fieldLabel(path), required, schema: {} });
    return;
  }
  if (object(schema) && typeof schema.$ref === 'string') {
    if (references.has(schema.$ref)) {
      fields.push({ path, label: fieldLabel(path), required, schema });
      return;
    }
    const target = localReference(root, schema.$ref);
    if (target === undefined) {
      fields.push({ path, label: fieldLabel(path), required, schema });
      return;
    }
    guide(target, root, path, required, new Set([...references, schema.$ref]), depth + 1, budget, base, fields);
    return;
  }
  const current = object(schema) ? schema : undefined;
  if (!current) {
    fields.push({ path, label: fieldLabel(path), required, schema: object(schema) ? schema : {} });
    return;
  }
  const properties = current.properties;
  if (!object(properties)) {
    const field: GuidedField = {
      path,
      label: fieldLabel(path),
      required,
      schema: current,
    };
    fields.push(typeof current.description === 'string'
      ? { ...field, description: current.description }
      : field);
    return;
  }

  // Keep an optional object atomic. Flattening it would mislabel its schema-required
  // descendants as independently optional and could assemble a predictably invalid object.
  if (!required && path.length) {
    const field: GuidedField = {
      path,
      label: fieldLabel(path),
      required: false,
      schema: current,
    };
    fields.push(typeof current.description === 'string'
      ? { ...field, description: current.description }
      : field);
    return;
  }

  if (required && path.length) setRequestValue(base, path, {});
  const requiredNames = new Set(Array.isArray(current.required)
    ? current.required.filter((name): name is string => typeof name === 'string')
    : []);
  for (const [name, child] of Object.entries(properties)) {
    guide(child, root, [...path, name], required && requiredNames.has(name), references, depth + 1, budget, base, fields);
  }
}

/**
 * Derive a bounded field guide from the exact live schema. This is presentation
 * only: it does not apply defaults or reproduce Gazette validation.
 */
export function guidedRequest(schema: Schema): GuidedRequest {
  const value: Record<string, unknown> = {};
  const fields: GuidedField[] = [];
  const current = resolved(schema, schema, new Set());
  if (current && object(current.properties)) {
    const budget = { nodes: 0 };
    const requiredNames = new Set(Array.isArray(current.required)
      ? current.required.filter((name): name is string => typeof name === 'string')
      : []);
    for (const [name, child] of Object.entries(current.properties)) {
      guide(child, schema, [name], requiredNames.has(name), new Set(), 1, budget, value, fields);
    }
  }
  const requiredLeafCount = fields.filter(field => field.required).length;
  return {
    value,
    fields,
    requiredLeafCount,
    defaultMode: requiredLeafCount > GUIDED_REQUIRED_FIELD_LIMIT ? 'advanced' : 'guided',
  };
}

/** Set one explicitly entered value without interpreting Gazette semantics. */
export function setRequestValue(target: Record<string, unknown>, path: readonly string[], value: unknown): void {
  if (!path.length) throw new Error('Gazette guided fields require a named schema path.');
  let owner = target;
  for (const part of path.slice(0, -1)) {
    const existing = owner[part];
    if (!object(existing)) owner[part] = {};
    owner = owner[part] as Record<string, unknown>;
  }
  owner[path[path.length - 1]!] = value;
}

/** A syntax-checked fragment, retained verbatim for native semantic validation. */
export class RequestFragment {
  private constructor(readonly source: string) {}
  static parse(source: string): RequestFragment {
    if (Buffer.byteLength(source, 'utf8') > MAX_BYTES) throw new Error('Gazette operation request exceeds the editor byte bound.');
    JSON.parse(source); // Syntax only; never use the lossy JavaScript value.
    return new RequestFragment(source);
  }
}

export function requestText(value: unknown): string {
  let nodes = 0;
  const encode = (item: unknown, depth: number): string => {
    if (++nodes > MAX_NODES || depth > MAX_DEPTH) throw new Error('Gazette operation request exceeds the editor structure bound.');
    if (item instanceof RequestFragment) return item.source;
    if (Array.isArray(item)) return `[${item.map(child => encode(child, depth + 1)).join(',')}]`;
    if (object(item)) return `{${Object.entries(item).map(([key, child]) => `${JSON.stringify(key)}:${encode(child, depth + 1)}`).join(',')}}`;
    const encoded = JSON.stringify(item);
    if (encoded === undefined || (typeof item === 'number' && !Number.isFinite(item))) throw new Error('Invalid guided request value.');
    return encoded;
  };
  const text = `${encode(value, 0)}\n`;
  if (Buffer.byteLength(text, 'utf8') > MAX_BYTES) throw new Error('Gazette operation request exceeds the editor byte bound.');
  return text;
}

function shape(
  schema: unknown,
  root: Schema,
  references: ReadonlySet<string>,
  depth: number,
  budget: { nodes: number },
): unknown {
  budget.nodes++;
  if (budget.nodes > MAX_NODES) throw new Error('Gazette operation schema is too large to project safely.');
  if (depth > MAX_DEPTH || !object(schema)) return null;

  if (typeof schema.$ref === 'string') {
    if (references.has(schema.$ref)) return null;
    const target = localReference(root, schema.$ref);
    if (target === undefined) return null;
    return shape(target, root, new Set([...references, schema.$ref]), depth + 1, budget);
  }

  const properties = schema.properties;
  if (object(properties)) {
    const result: Record<string, unknown> = {};
    for (const [name, property] of Object.entries(properties)) {
      result[name] = shape(property, root, references, depth + 1, budget);
    }
    return result;
  }
  if (schema.type === 'object') return {};
  if (schema.type === 'array') return [];
  return null;
}

/**
 * Project only JSON structure from the live native schema. Leaf values remain
 * null so the editor never chooses a path, provider, permission or effect.
 */
export function requestSkeleton(schema: Schema): string {
  const projected = shape(schema, schema, new Set(), 0, { nodes: 0 });
  const value = object(projected) ? projected : {};
  return requestText(value);
}
