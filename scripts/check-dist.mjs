// Smoke check of the built package: both entry points load and export the same API.
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const cjs = require('../dist-js/index.cjs')
const esm = await import('../dist-js/index.js')

const names = (m) => Object.keys(m).filter((k) => k !== 'default').sort()
const [a, b] = [names(cjs), names(esm)]
if (a.join() !== b.join()) throw new Error(`CJS/ESM exports differ:\n${a}\n${b}`)
if (typeof esm.SerialPort !== 'function') throw new Error('missing export: SerialPort')
for (const fn of ['available_ports', 'managed_ports', 'watchAvailablePorts', 'closeAll', 'getCapabilities']) {
  if (typeof esm.SerialPort[fn] !== 'function') throw new Error(`missing static: SerialPort.${fn}`)
}
console.log(`dist ok: ${a.length} exports`)
