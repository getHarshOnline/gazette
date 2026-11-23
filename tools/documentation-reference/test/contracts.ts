/** A read-only key map with intentionally writable values. */
export interface KeyMap {
  readonly [key: string]: { value: number };
}

/** A mutable key map is a distinct contract. */
export interface MutableMap {
  [key: string]: { value: number };
}

/** Nested properties, callbacks, quoted names and accessors stay distinct. */
export interface Contract {
  readonly optional?: ReadonlyArray<{ readonly id: string; mutable: number }>;
  readonly callback?: (value: string) => number;
  readonly 'quoted.name'?: string;
  get current(): string;
  set writable(value: string);
}

/** Literal branches and nested indexes retain their own modifiers. */
export type Variant =
  | { readonly accepted: true; payload: { readonly [key: string]: number } }
  | { readonly accepted: false; readonly reason: string; mutable?: number };

/** Inline callback properties cannot inherit their container's mutability. */
export type Inline = {
  readonly callback?: (value: number) => string;
  mutable: { value: number };
  readonly 'quoted.name'?: string;
};

/** Generic title constraints and defaults remain part of the visible contract. */
export interface NamedRecord {
  readonly id: string;
}
export interface GenericResult<R extends NamedRecord = NamedRecord> {
  readonly record: R;
}

/** Literal Markdown characters in generic types remain literal title text. */
export interface LiteralResult<T extends '*' | '[raw]' | `item-${string}` = '*'> {
  readonly value: T;
}
