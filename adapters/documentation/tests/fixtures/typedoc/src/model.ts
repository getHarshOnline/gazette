/** A public generic envelope.
 * @example
 * ```ts
 * const envelope: Envelope<number> = { value: 42 };
 * ```
 */
export interface Envelope<T> {
  /** Original payload, without conversion. */
  readonly value: T;
}

/** A closed public choice. */
export type Choice = "first" | "second";
