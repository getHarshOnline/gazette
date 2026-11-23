// Copy one built library byte-for-byte to one fresh explicit local .node path.
import { constants } from 'node:fs';
import { copyFile, lstat, readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { isAbsolute, extname } from 'node:path';

const [input, output] = process.argv.slice(2);
if (process.argv.length !== 4 || !input || !output || !isAbsolute(input) || !isAbsolute(output) || extname(output) !== '.node') {
  throw new Error('Supply exact absolute built-library and fresh .node output paths');
}
if (!(await lstat(input)).isFile()) throw new Error('Built library must be a regular file, not a symlink');
await copyFile(input, output, constants.COPYFILE_EXCL);
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const source = digest(await readFile(input));
const projected = digest(await readFile(output));
if (source !== projected) throw new Error('Native projection verification failed; preserve and inspect output');
process.stdout.write(`${JSON.stringify({ sha256: projected, operation: 'local-native-projection' })}\n`);
