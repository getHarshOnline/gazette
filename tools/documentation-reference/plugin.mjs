import { MarkdownTheme } from 'typedoc-plugin-markdown';
import { declarationType } from './partials/declaration-type.mjs';
import { indexSignature } from './partials/index-signature.mjs';
import { escapeChars, unEscapeChars } from './provider.mjs';

// Only formatting changes: the producer's model, routing and other partials stay owned by TypeDoc.
class ContractMarkdownTheme extends MarkdownTheme {
  getRenderContext(page) {
    const context = super.getRenderContext(page);
    context.partials.declarationType = declarationType.bind(context);
    context.partials.indexSignature = indexSignature.bind(context);
    // Keep the complete formatter-owned title as plain Markdown text. The target
    // can render it once without dropping generic constraints or authored body links.
    const pageTitle = context.partials.pageTitle;
    context.partials.pageTitle = () => escapeChars(unEscapeChars(pageTitle()));
    return context;
  }
}

/** Register an explicitly selected local theme; loading this plugin does not select it. */
export function load(app) {
  app.renderer.defineTheme('contract-markdown', ContractMarkdownTheme);
}
