// Writes what crates/aave-abi vendors from an unpacked @aave-dao/aave-address-book:
// `node scripts/extract-aave-abi.mts <package dir> <out dir>`.

import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';

const [packageArgument, outArgument] = process.argv.slice(2);
if (packageArgument === undefined || outArgument === undefined) {
  console.error('usage: extract-aave-abi.mts <package dir> <out dir>');
  process.exit(2);
}
const packageDir = resolve(packageArgument);
const outDir = resolve(outArgument);

// A field the book renamed fails here, rather than writing `undefined` into a vendored file.
function at(value: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((current, key) => {
    if (typeof current !== 'object' || current === null || !Object.hasOwn(current, key)) {
      throw new Error(`the address book has no ${path}`);
    }
    return Reflect.get(current, key);
  }, value);
}

function text(value: unknown, path: string): string {
  const found = at(value, path);
  if (typeof found !== 'string') throw new Error(`the address book's ${path} is not a string`);
  return found;
}

function abi(value: unknown, path: string): unknown[] {
  const found = at(value, path);
  if (!Array.isArray(found)) throw new Error(`the address book's ${path} is not an ABI`);
  return found;
}

// The two entry points require only the package's own chunks, so an unpacked tarball loads
// without an install.
const load: (id: string) => unknown = createRequire(join(packageDir, 'package.json'));
const manifest: unknown = JSON.parse(readFileSync(join(packageDir, 'package.json'), 'utf8'));
const abis = load('./dist/abis/index.js');
const ethereum = load('./dist/AaveV4Ethereum.js');

// Read in full before anything is written, so a failure leaves the vendored files as they were.
const files: Record<string, unknown> = {
  'ISpokeV4.json': abi(abis, 'ISpokeV4_ABI'),
  'IHubV4.json': abi(abis, 'IHubV4_ABI'),
  'IAaveOracleV4.json': abi(abis, 'IAaveOracleV4_ABI'),
  'deployment.json': {
    source: `${text(manifest, 'name')}@${text(manifest, 'version')}`,
    chainId: at(ethereum, 'CHAIN_ID'),
    mainSpoke: text(ethereum, 'SPOKES.MAIN_SPOKE'),
    mainSpokeOracle: text(ethereum, 'SPOKES.MAIN_SPOKE_ORACLE'),
    coreHub: text(ethereum, 'HUBS.CORE_HUB'),
  },
};

for (const [file, value] of Object.entries(files)) {
  writeFileSync(join(outDir, file), `${JSON.stringify(value, null, 2)}\n`);
}
