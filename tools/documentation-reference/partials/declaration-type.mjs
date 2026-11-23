// Derived from typedoc-plugin-markdown 4.14.1; see ../UPSTREAM-LICENSE.
import { backTicks, getPropertyName } from '../provider.mjs';
export function declarationType(model, options) {
    const shouldFormat = this.options.getValue('useCodeBlocks');
    const md = ['\\{'];
    if (model.indexSignatures?.length) {
        const indexSignatureMd = [];
        model.indexSignatures.forEach((indexSignature) => {
            const key = indexSignature.parameters
                ? indexSignature.parameters.map((param) => `\\[${backTicks(param.name)}: ${this.partials.someType(param.type)}\\]`)
                : '';
            const obj = this.partials.someType(indexSignature.type);
            const modifier = indexSignature.flags?.isReadonly ? `${backTicks('readonly')} ` : '';
            indexSignatureMd.push(`${modifier}${key}: ${obj};`);
        });
        md.push(indexSignatureMd.join('\n'));
    }
    if (model.signatures) {
        md.push(`${this.partials.functionType(model.signatures, {
            typeSeparator: ': ',
            forceCollapse: options?.forceCollapse,
        })};`);
    }
    if (model.children) {
        const children = model.children;
        const getName = (name) => shouldFormat ? getPropertyName(name) : name;
        const declarationTypes = children &&
            children.flatMap((obj) => {
                if (obj.signatures?.length) {
                    const displayObjectName = getName(obj.name) + (obj.flags?.isOptional ? '?' : '');
                    return obj.signatures.map((signature) => {
                        const modifier = obj.flags?.isReadonly ? `${backTicks('readonly')} ` : '';
                        const signatureString = `${modifier}${backTicks(displayObjectName)}${this.partials.functionType([signature], { typeSeparator: ': ', forceCollapse: options?.forceCollapse })}`;
                        if (shouldFormat) {
                            return `  ${indentBlock(signatureString)};`;
                        }
                        return `${indentBlock(signatureString)};`;
                    });
                }
                const name = [];
                if (obj.getSignature) {
                    name.push('get');
                    name.push(backTicks(getName(obj.name)) + '()');
                }
                else if (obj.setSignature) {
                    name.push('set');
                    const params = obj.setSignature.parameters
                        ? this.partials.signatureParameters(obj.setSignature.parameters, {
                            forceExpandParameters: true,
                        })
                        : '()';
                    name.push(backTicks(getName(obj.name)) + params);
                }
                else {
                    const displayObjectName = getName(obj.name) + (obj.flags?.isOptional ? '?' : '');
                    if (obj.flags?.isReadonly) name.push(backTicks('readonly'));
                    name.push(backTicks(displayObjectName));
                }
                const theType = this.helpers.getDeclarationType(obj);
                const typeString = this.partials.someType(theType, options);
                if (shouldFormat) {
                    return `  ${name.join(' ')}: ${indentBlock(typeString)};`;
                }
                return `${name.join(' ')}: ${indentBlock(typeString)};`;
            });
        if (declarationTypes) {
            md.push(`${shouldFormat ? `${declarationTypes.join('\n')}` : ` ${declarationTypes.join(' ')}`}`);
        }
    }
    md.push(`${shouldFormat ? '' : ' '}\\}`);
    return md.join(shouldFormat ? '\n' : '');
}
function indentBlock(content) {
    const lines = content.split(`${'\n'}`);
    return lines
        .filter((line) => Boolean(line.length))
        .map((line, i) => {
        if (i === 0) {
            return line;
        }
        if (i === lines.length - 1) {
            return line.trim().startsWith('}') ? line : `  ${line}`;
        }
        return `   ${line}`;
    })
        .join(`${`\n`}`);
}
