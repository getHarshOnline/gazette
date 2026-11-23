// SPDX-License-Identifier: Apache-2.0
// Small JSON carrier checks, not a publication schema or a source resolver.
const encoder = new TextEncoder();
const own = (value, key) => Object.prototype.hasOwnProperty.call(value, key);

export class WebMcpError extends Error {
  constructor(code, cause) {
    super(code, cause === undefined ? undefined : { cause });
    this.name = 'WebMcpError';
    this.code = code;
  }
}

export function fail(code, cause) { throw new WebMcpError(code, cause); }

export function keys(value, allowed, required = allowed, code = 'webmcp_options') {
  if (value === null || typeof value !== 'object' || Array.isArray(value)
      || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) fail(code);
  const names = Reflect.ownKeys(value);
  if (names.length > allowed.length || names.some(name => typeof name !== 'string'
      || !allowed.includes(name) || !own(Object.getOwnPropertyDescriptor(value, name), 'value'))
      || required.some(name => !own(value, name))) fail(code);
}

export function integer(value, maximum, code = 'webmcp_limit') {
  if (!Number.isSafeInteger(value) || value < 1 || value > maximum) fail(code);
  return value;
}

/** Whole immutable JSON snapshot; getters, toJSON, cycles and partial output are refused. */
export function snapshot(value, limits, maximum) {
  let bytes = maximum;
  let nodes = limits.maxNodes;
  const ancestors = new Set();
  const debit = size => { bytes -= size; if (bytes < 0) fail('webmcp_limit'); };
  const text = value => {
    if (value.length > bytes) fail('webmcp_limit');
    debit(encoder.encode(JSON.stringify(value)).byteLength);
  };
  const visit = (value, depth) => {
    if (depth > limits.maxDepth || --nodes < 0) fail('webmcp_limit');
    if (value === null || typeof value === 'boolean') {
      debit(value === null || value === true ? 4 : 5); return value;
    }
    if (typeof value === 'number') {
      if (!Number.isFinite(value)) fail('webmcp_data');
      debit(String(value).length); return value;
    }
    if (typeof value === 'string') { text(value); return value; }
    const array = Array.isArray(value);
    if (value === null || typeof value !== 'object' || ancestors.has(value)
        || (!array && ![Object.prototype, null].includes(Object.getPrototypeOf(value)))) {
      fail('webmcp_data');
    }
    const names = Reflect.ownKeys(value).filter(name => !(array && name === 'length'));
    if (names.length > nodes) fail('webmcp_limit');
    if (array && (names.length !== value.length || names.some((name, i) => name !== String(i)))) {
      fail('webmcp_data');
    }
    debit(2 + Math.max(0, names.length - 1));
    ancestors.add(value);
    const result = array ? [] : Object.create(null);
    for (const name of names) {
      const descriptor = Object.getOwnPropertyDescriptor(value, name);
      if (typeof name !== 'string' || !own(descriptor, 'value') || !descriptor.enumerable) {
        fail('webmcp_data');
      }
      if (!array) { text(name); debit(1); }
      Object.defineProperty(result, name, { value: visit(descriptor.value, depth + 1), enumerable: true });
    }
    ancestors.delete(value);
    return Object.freeze(result);
  };
  const captured = visit(value, 0);
  // Verify the exact encoded carrier, including number and escaping behavior.
  if (encoder.encode(JSON.stringify(captured)).byteLength > maximum) fail('webmcp_limit');
  return captured;
}
