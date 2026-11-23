import { constants } from 'node:fs';
import { open, lstat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { requireThat } from './contract.mjs';

function same(a, b) {
  return a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeNs === b.mtimeNs && a.ctimeNs === b.ctimeNs;
}

// Like the native executable pin, these are paired observations in a trusted namespace.
export async function verifiedFile(path, pin) {
  const named = await lstat(path, { bigint: true });
  requireThat(named.isFile() && named.size === BigInt(pin.bytes), 'asset-binding');
  const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const identity = await file.stat({ bigint: true });
    requireThat(same(named, identity), 'asset-binding');
    const hash = createHash('sha256'), chunks = [];
    let length = 0;
    for (;;) {
      const buffer = Buffer.alloc(Math.min(65536, pin.bytes - length + 1));
      const { bytesRead } = await file.read(buffer);
      if (!bytesRead) break;
      length += bytesRead;
      requireThat(length <= pin.bytes, 'asset-binding');
      const chunk = buffer.subarray(0, bytesRead); hash.update(chunk); chunks.push(chunk);
    }
    requireThat(length === pin.bytes && hash.digest('hex') === pin.sha256, 'asset-binding');
    requireThat(same(identity, await file.stat({ bigint: true })) && same(identity, await lstat(path, { bigint: true })), 'asset-binding');
    return Buffer.concat(chunks, length);
  } finally { await file.close(); }
}

export async function retainedAsset(root, asset) {
  const parts = asset.entry.split('/');
  for (let i = 1; i < parts.length; i++) {
    const directory = await lstat(join(root, ...parts.slice(0, i)));
    requireThat(directory.isDirectory() && !directory.isSymbolicLink(), 'asset-binding');
  }
  return verifiedFile(join(root, asset.entry), asset);
}
