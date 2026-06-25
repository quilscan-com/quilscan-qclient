import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const sourcePath = path.join(root, 'client/cmd/node/prover/manage_actions.go')
const source = fs.readFileSync(sourcePath, 'utf8')

const checks = [
  ['manage RPC timeout is 120 seconds', /const\s+rpcTimeout\s*=\s*120\s*\*\s*time\.Second/.test(source)],
]

const failed = checks.filter(([, ok]) => !ok)
if (failed.length) {
  for (const [label] of failed) console.error(`missing: ${label}`)
  process.exit(1)
}

console.log('qclient manage timeout check passed')
