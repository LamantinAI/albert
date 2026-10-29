// Produce an independently verifiable static bundle; Cargo never runs Node.
import { createHash } from 'node:crypto'
import { readdir, readFile, mkdtemp, writeFile, mkdir, rm, cp } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'vite'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const target = resolve(root, '../ui-dist')
const temporary = await mkdtemp(join(tmpdir(), 'albert-ui-build-'))
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex')
async function files(directory) {
  const out = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isSymbolicLink()) throw new Error(`Symlink is not a build input: ${path}`)
    if (entry.isDirectory()) out.push(...await files(path))
    else if (entry.isFile()) out.push(path)
  }
  return out.sort()
}
async function fingerprints(directory) {
  return Object.fromEntries(await Promise.all((await files(directory)).map(async (path) => [relative(directory, path).replaceAll('\\', '/'), hash(await readFile(path))])))
}
try {
  const inputs = [...await files(join(root, 'src')), ...await files(join(root, 'public')), ...['index.html', 'package.json', 'package-lock.json', 'tsconfig.json', 'vite.config.ts', 'scripts/build-embedded.mjs'].map((name) => join(root, name))].sort()
  const digest = createHash('sha256')
  for (const path of inputs) { digest.update(relative(root, path).replaceAll('\\', '/')); digest.update(await readFile(path)) }
  await build({ root, configFile: join(root, 'vite.config.ts'), logLevel: 'warn', build: { outDir: temporary, emptyOutDir: true, sourcemap: false } })
  const assets = await fingerprints(temporary)
  for (const path of Object.keys(assets)) if (!/\.(html|js|css|svg|woff2|png|ico|txt)$/.test(path)) throw new Error(`Unexpected embedded asset: ${path}`)
  await writeFile(join(temporary, 'manifest.json'), JSON.stringify({ source_sha256: digest.digest('hex'), assets }, null, 2) + '\n')
  if (process.argv.includes('--check')) {
    if (JSON.stringify(await fingerprints(target)) !== JSON.stringify(await fingerprints(temporary))) throw new Error('Embedded frontend is stale; run npm run build:embedded')
    console.log('Embedded frontend matches sources and lockfile.')
  } else {
    await rm(target, { recursive: true, force: true })
    await mkdir(target, { recursive: true })
    await cp(temporary, target, { recursive: true })
    console.log(`Embedded ${Object.keys(assets).length} assets.`)
  }
} finally { await rm(temporary, { recursive: true, force: true }) }
