import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { cwd } from 'node:process'
import typescript from '@rollup/plugin-typescript'

const pkg = JSON.parse(readFileSync(join(cwd(), 'package.json'), 'utf8'))

const esmOut =
    typeof pkg.exports.import === 'string'
        ? pkg.exports.import
        : pkg.exports.import.default

export default {
    input: 'guest-js/index.ts',
    output: [
        {
            file: esmOut,
            format: 'esm'
        },
        {
            file: pkg.exports.require,
            format: 'cjs'
        }
    ],
    plugins: [
        typescript({
            declaration: true,
            declarationDir: dirname(esmOut)
        })
    ],
    external: [
        /^@tauri-apps\/api/,
        ...Object.keys(pkg.dependencies || {}),
        ...Object.keys(pkg.peerDependencies || {})
    ]
}
