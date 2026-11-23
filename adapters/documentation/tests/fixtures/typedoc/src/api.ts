import type { Envelope } from "./model.js";

/** Retain a value in an {@link Envelope}.
 * @typeParam T - The original value type.
 * @param value - Original value.
 * @returns The unchanged typed envelope.
 */
export function wrap<T>(value: T): Envelope<T> {
  return { value };
}

/** Render a textual value. */
export function describe(value: string): string;
/** Render a numeric value. */
export function describe(value: number): string;
export function describe(value: string | number): string {
  return String(value);
}
