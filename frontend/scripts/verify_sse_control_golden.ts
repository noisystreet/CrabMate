/**
 * 与 `cargo test control_dispatch_mirror::golden` 共用 `fixtures/sse_control_golden.jsonl`。
 * 运行：`cd frontend && npm run verify-sse-contract`
 */
import * as fs from 'node:fs'
import * as path from 'node:path'
import { fileURLToPath } from 'node:url'
import { classifySseControlPayloadParsed } from '../src/sse_control_dispatch'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const goldenPath = path.resolve(__dirname, '../../fixtures/sse_control_golden.jsonl')

function main() {
  const raw = fs.readFileSync(goldenPath, 'utf8')
  const lines = raw.split('\n')
  let failures = 0
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const t = line.trim()
    if (!t || t.startsWith('#')) continue
    const parts = t.split('\t')
    if (parts.length !== 3) {
      console.error(`${goldenPath}:${i + 1}: expected 3 tab-separated columns`)
      failures += 1
      continue
    }
    const jsonLine = parts[1].trim()
    const want = parts[2].trim() as 'stop' | 'handled' | 'plain'
    let obj: unknown
    try {
      obj = JSON.parse(jsonLine)
    } catch (e) {
      console.error(`${goldenPath}:${i + 1}: JSON.parse failed: ${e}\n${jsonLine}`)
      failures += 1
      continue
    }
    if (obj === null || typeof obj !== 'object' || Array.isArray(obj)) {
      console.error(`${goldenPath}:${i + 1}: expected JSON object`)
      failures += 1
      continue
    }
    const got = classifySseControlPayloadParsed(obj as Parameters<typeof classifySseControlPayloadParsed>[0])
    if (got !== want) {
      console.error(
        `${goldenPath}:${i + 1}: expected ${want}, got ${got}\n  json: ${jsonLine}`,
      )
      failures += 1
    }
  }
  if (failures > 0) {
    process.exit(1)
  }
  console.log('sse_control_golden: ok')
}

main()
