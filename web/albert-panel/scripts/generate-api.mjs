// Generate every contract from Rust's OpenAPI; split declarations by concern so
// generated files obey the same bounded-file convention as the implementation.
import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { dirname, join, resolve } from 'node:path'
import openapiTS, { astToString } from 'openapi-typescript'
import ts from 'typescript'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const workspace = resolve(root, '../..')
const exported = spawnSync('cargo', ['run', '--quiet', '--locked', '-p', 'xtask', '--', 'openapi'], { cwd: workspace, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 })
if (exported.status !== 0) throw new Error(exported.stderr || exported.error?.message || 'Schema export failed')
const contract = Buffer.from(exported.stdout)
const source = astToString(await openapiTS(JSON.parse(contract.toString())))
const ast = ts.createSourceFile('schema.d.ts', source, ts.ScriptTarget.Latest, true)
const paths = ast.statements.find((node) => ts.isInterfaceDeclaration(node) && node.name.text === 'paths')
const components = ast.statements.find((node) => ts.isInterfaceDeclaration(node) && node.name.text === 'components')
const schemas = components.members.find((node) => node.name.getText(ast) === 'schemas')
const output = new Map()
const banner = '/** Generated from the Rust Utoipa schema via xtask. Do not edit. */\n'
function split(name, members) {
  const groups = [[]]
  let count = 0
  for (const member of members) {
    const text = member.getFullText(ast).trim()
    const lines = text.split('\n').length
    if (count + lines > 420 && groups.at(-1).length) { groups.push([]); count = 0 }
    groups.at(-1).push(text); count += lines
  }
  return groups.map((group, index) => {
    const type = `${name}${index}`
    output.set(`generated/${type}.d.ts`, `${banner}import type { components, operations } from '../schema'\n\nexport interface ${type} {\n${group.join('\n')}\n}\n`)
    return type
  })
}
const pathNames = split('Paths', paths.members)
const schemaNames = split('Schemas', schemas.type.members)
const imports = [...pathNames, ...schemaNames].map((name) => `import type { ${name} } from './generated/${name}'`).join('\n')
const otherComponents = components.members.filter((member) => member !== schemas).map((member) => member.getFullText(ast)).join('\n')
const otherNodes = ast.statements.filter((node) => node !== paths && node !== components).map((node) => node.getText(ast)).join('\n')
output.set('schema.d.ts', `${banner}// Source SHA256: ${createHash('sha256').update(contract).digest('hex')}\n${imports}\n\nexport type paths = ${pathNames.join(' & ')}\nexport interface components {\n  schemas: ${schemaNames.join(' & ')}\n${otherComponents}\n}\n${otherNodes}\n`)
const api = join(root, 'src/api')
if (process.argv.includes('--check')) {
  const actual = (await readdir(join(api, 'generated'))).sort()
  const expected = [...output.keys()].filter((name) => name.startsWith('generated/')).map((name) => name.slice('generated/'.length)).sort()
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error('Unexpected generated API declarations; run npm run generate')
  for (const [name, text] of output) if (await readFile(join(api, name), 'utf8').catch(() => '') !== text) throw new Error('API types are stale; run npm run generate')
} else {
  await rm(join(api, 'generated'), { recursive: true, force: true })
  await mkdir(join(api, 'generated'), { recursive: true })
  for (const [name, text] of output) await writeFile(join(api, name), text)
}
console.log(`API contract checked: ${output.size} generated declaration files.`)
