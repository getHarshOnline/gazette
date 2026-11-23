import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

// These utilities are intentionally coupled to the package-lock's exact formatter version.
const entry = pathToFileURL(createRequire(import.meta.url).resolve('typedoc-plugin-markdown'));
export const { backTicks, codeBlock } = await import(new URL('./libs/markdown/index.js', entry));
export const { getPropertyName } = await import(new URL('./theme/lib/index.js', entry));
export const { escapeChars, unEscapeChars } = await import(new URL('./libs/utils/index.js', entry));
