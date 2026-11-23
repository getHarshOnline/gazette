// Derived from typedoc-plugin-markdown 4.14.1; see ../UPSTREAM-LICENSE.
import { backTicks, codeBlock } from '../provider.mjs';
export function indexSignature(model, options) {
    const useCodeBlocks = this.options.getValue('useCodeBlocks');
    const md = [];
    const readonly = model.flags?.isReadonly;
    const params = model.parameters
        ? model.parameters.map((parameter) => {
            return parameter.type
                ? `${useCodeBlocks ? parameter.name : backTicks(parameter.name)}: ${this.partials.someType(parameter.type)}`
                : '';
        })
        : [];
    if (model.type) {
        if (this.options.getValue('useCodeBlocks')) {
            md.push(codeBlock(`${readonly ? 'readonly ' : ''}[${params.join('')}]: ${this.partials.someType(model.type)}`));
        }
        else {
            md.push(`${readonly ? `${backTicks('readonly')} ` : ''}\\[${params.join('')}\\]: ${this.partials.someType(model.type)}`);
        }
    }
    if (model.comment) {
        md.push(this.partials.comment(model.comment, {
            headingLevel: options?.headingLevel,
        }));
    }
    // A blank line ends the blockquote the caller puts the signature in, so the
    // comment (and any modifier flags) is not pulled into it.
    return md.join('\n\n');
}
