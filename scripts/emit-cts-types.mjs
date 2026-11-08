// CJS entry needs its own declarations: copy every dist-js/*.d.ts to *.d.cts
// and point relative imports at the .cjs siblings (node16 resolution).
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'

const dir = new URL('../dist-js/', import.meta.url)
for (const f of readdirSync(dir).filter((n) => n.endsWith('.d.ts'))) {
  const src = readFileSync(new URL(f, dir), 'utf8')
  writeFileSync(new URL(f.replace(/\.d\.ts$/, '.d.cts'), dir), src.replace(/(from '\.\/[\w-]+)\.js'/g, "$1.cjs'"))
}
