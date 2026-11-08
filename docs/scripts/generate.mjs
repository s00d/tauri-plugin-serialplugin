// Builds the reference pages from the repository sources before `nuxt dev` / `nuxt generate`.
// Output (gitignored, rewritten on every run):
//   content/3.reference/  JS API (guest-js), Tauri commands (src/commands.rs), permissions (permissions/)
//   content/9.changelog.md  CHANGELOG.md with frontmatter and site links
// Edit the sources, never the generated pages.
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { parse as parseToml } from 'smol-toml'

const root = fileURLToPath(new URL('../../', import.meta.url))
const content = fileURLToPath(new URL('../content/', import.meta.url))
const refDir = `${content}3.reference/`
const repo = 'https://github.com/s00d/tauri-plugin-serialplugin/blob/main/'

const read = (rel) => readFileSync(root + rel, 'utf8')
const page = (title, description, source, body) =>
  `---\ntitle: ${JSON.stringify(title)}\ndescription: ${JSON.stringify(description)}\ngenerated: true\n---\n\n` +
  `<!-- Generated from ${source} by docs/scripts/generate.mjs. Edit the source, not this file. -->\n\n${body.trim()}\n`
// Inline code inside a table cell: one line, `|` escaped for GFM.
const cell = (text) => (text ? '`' + text.replace(/\s+/g, ' ').replace(/\|/g, '\\|') + '`' : '')
const oneLine = (text) => (text || '').replace(/\s*\n\s*/g, ' ').replace(/\|/g, '\\|').trim()
function check(cond, message) {
  if (!cond) throw new Error(`generate.mjs: ${message}`)
}

// ---------------------------------------------------------------- JS API (TypeScript compiler API)

function sourceFile(rel) {
  return ts.createSourceFile(rel, read(rel), ts.ScriptTarget.Latest, true)
}

function jsdoc(node) {
  const docs = ts.getJSDocCommentsAndTags(node).filter(ts.isJSDoc)
  const doc = docs.at(-1)
  if (!doc) return { text: '', tags: {} }
  const tags = {}
  for (const tag of doc.tags ?? []) tags[tag.tagName.text] = ts.getTextOfJSDocComment(tag.comment) ?? ''
  return { text: ts.getTextOfJSDocComment(doc.comment) ?? '', tags }
}

function docBlock({ text, tags }) {
  const out = []
  if ('deprecated' in tags) out.push(`**Deprecated.** ${tags.deprecated}`.trim())
  if (text) out.push(text)
  return out.join('\n\n')
}

// Declaration text without its body, whitespace collapsed: `async read(options?: ReadOptions): Promise<string>`.
function signature(node, sf) {
  const end = node.body ? node.body.getStart(sf) : node.end
  return sf.text
    .slice(node.getStart(sf), end)
    .replace(/\s+/g, ' ')
    .replace(/\(\s/g, '(')
    .replace(/,?\s\)/g, ')')
    .replace(/;$/, '')
    .trim()
}

const hasModifier = (node, kind) => node.modifiers?.some((m) => m.kind === kind) ?? false
const memberName = (node) => node.name?.getText() ?? 'constructor'

// Public surface = what guest-js/index.ts re-exports.
const exported = new Set()
for (const st of sourceFile('guest-js/index.ts').statements) {
  if (ts.isExportDeclaration(st) && st.exportClause && ts.isNamedExports(st.exportClause)) {
    for (const el of st.exportClause.elements) exported.add(el.name.text)
  }
}
check(exported.has('SerialPort'), 'guest-js/index.ts does not export SerialPort')

function serialPortPage() {
  const sf = sourceFile('guest-js/serial-port.ts')
  const cls = sf.statements.find((st) => ts.isClassDeclaration(st) && st.name?.text === 'SerialPort')
  check(cls, 'class SerialPort not found in guest-js/serial-port.ts')

  const groups = { ctor: [], statics: [], props: [], methods: [] }
  for (const m of cls.members) {
    if (hasModifier(m, ts.SyntaxKind.PrivateKeyword) || hasModifier(m, ts.SyntaxKind.ProtectedKeyword)) continue
    if (m.name && ts.isPrivateIdentifier(m.name)) continue
    const doc = jsdoc(m)
    if ('internal' in doc.tags) continue
    const isStatic = hasModifier(m, ts.SyntaxKind.StaticKeyword)
    const name = memberName(m)
    const entry = { name, isStatic, sig: signature(m, sf), doc: docBlock(doc) }
    if (ts.isConstructorDeclaration(m)) groups.ctor.push(entry)
    else if (ts.isPropertyDeclaration(m) || ts.isGetAccessorDeclaration(m)) groups.props.push(entry)
    else if (ts.isMethodDeclaration(m)) (isStatic ? groups.statics : groups.methods).push(entry)
  }
  check(groups.methods.length > 0 && groups.statics.length > 0, 'no SerialPort methods found')

  const section = (title, entries, label) =>
    entries.length
      ? `## ${title}\n\n` +
        entries
          .map((e) => `### \`${label(e)}\`\n\n\`\`\`ts\n${e.sig}\n\`\`\`\n${e.doc ? `\n${e.doc}\n` : ''}`)
          .join('\n')
      : ''

  const summary = [...groups.statics, ...groups.methods]
    .map((e) => `| ${cell((e.isStatic ? 'SerialPort.' : '') + e.sig.replace(/^(static\s+)?(async\s+)?/, ''))} | ${oneLine(e.doc.split('\n\n')[0])} |`)
    .join('\n')

  const body = [
    'The `SerialPort` class exported by `tauri-plugin-serialplugin-api`. Signatures and descriptions come',
    'straight from [`guest-js/serial-port.ts`](' + repo + 'guest-js/serial-port.ts); for how the pieces fit',
    'together see the [JavaScript API guide](/guides/javascript-api). Option and result types are on the',
    '[types page](/reference/types).',
    '',
    '| Method | Description |',
    '|--------|-------------|',
    summary,
    '',
    section('Constructor', groups.ctor, () => 'new SerialPort()'),
    section('Properties', groups.props, (e) => e.name),
    section('Static methods', groups.statics, (e) => `SerialPort.${e.name}()`),
    section('Instance methods', groups.methods, (e) => `${e.name}()`),
  ].join('\n')

  return page('SerialPort', 'Every public member of the SerialPort class, generated from guest-js/serial-port.ts.', 'guest-js/serial-port.ts', body)
}

function typesPage() {
  const sections = { constants: [], enums: [], interfaces: [], aliases: [] }
  const seen = new Set()
  const codeBlocks = new Map()

  for (const rel of ['guest-js/types.ts', 'guest-js/logger.ts']) {
    const sf = sourceFile(rel)
    for (const st of sf.statements) {
      const names = ts.isVariableStatement(st)
        ? st.declarationList.declarations.map((d) => d.name.getText())
        : st.name
          ? [st.name.text]
          : []
      const name = names[0]
      if (!name || !exported.has(name)) continue
      const doc = docBlock(jsdoc(st))
      const head = `### \`${name}\`\n\n${doc ? `${doc}\n\n` : ''}`

      if (ts.isInterfaceDeclaration(st)) {
        const rows = st.members.map((m) => {
          const d = oneLine(docBlock(jsdoc(m)))
          const opt = m.questionToken ? '?' : ''
          if (ts.isMethodSignature(m)) return `| ${cell(signature(m, sf))} | | ${d} |`
          return `| ${cell(m.name.getText() + opt)} | ${cell(m.type?.getText(sf) ?? 'unknown')} | ${d} |`
        })
        sections.interfaces.push(`${head}| Property | Type | Description |\n|----------|------|-------------|\n${rows.join('\n')}\n`)
      } else if (ts.isEnumDeclaration(st)) {
        const rows = st.members.map((m) => `| ${cell(m.name.getText())} | ${cell(m.initializer?.getText(sf) ?? '')} |`)
        sections.enums.push(`${head}| Member | Value |\n|--------|-------|\n${rows.join('\n')}\n`)
      } else if (ts.isTypeAliasDeclaration(st) || ts.isVariableStatement(st)) {
        // `LogLevel` is a const object plus a type of the same name: one code block with both.
        const existing = codeBlocks.get(name)
        if (existing) existing.code.push(st.getText(sf))
        else {
          const entry = { head, code: [st.getText(sf)] }
          codeBlocks.set(name, entry)
          ;(ts.isVariableStatement(st) ? sections.constants : sections.aliases).push(entry)
        }
      }
      seen.add(name)
    }
  }
  check(sections.interfaces.length > 0 && sections.enums.length > 0, 'no exported types found in guest-js/types.ts')

  const missing = [...exported].filter((n) => n !== 'SerialPort' && !seen.has(n))
  if (missing.length) console.warn(`generate.mjs: exported but not documented (declared elsewhere?): ${missing.join(', ')}`)

  const render = (e) => (typeof e === 'string' ? e : `${e.head}\`\`\`ts\n${e.code.join('\n\n')}\n\`\`\`\n`)
  const group = (title, list) => (list.length ? `## ${title}\n\n${list.map(render).join('\n')}` : '')
  const body = [
    'Everything `tauri-plugin-serialplugin-api` exports besides `SerialPort`, generated from',
    `[\`guest-js/types.ts\`](${repo}guest-js/types.ts) and [\`guest-js/logger.ts\`](${repo}guest-js/logger.ts).`,
    'Only names re-exported by `guest-js/index.ts` are listed.',
    '',
    group('Constants', sections.constants),
    group('Enums', sections.enums),
    group('Interfaces', sections.interfaces),
    group('Type aliases', sections.aliases),
  ].join('\n')
  return page('Types', 'Enums, option and result types of tauri-plugin-serialplugin-api, generated from guest-js/types.ts.', 'guest-js/types.ts', body)
}

// ---------------------------------------------------------------- Permissions (permissions/**)

function tomlFiles(dir) {
  return readdirSync(root + dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? tomlFiles(`${dir}/${e.name}`) : e.name.endsWith('.toml') ? [`${dir}/${e.name}`] : [],
  )
}

const permissions = [] // { identifier, description, allow: [], deny: [] }
for (const file of tomlFiles('permissions/autogenerated').sort()) {
  for (const p of parseToml(read(file)).permission ?? []) {
    permissions.push({ identifier: p.identifier, description: p.description ?? '', allow: p.commands?.allow ?? [], deny: p.commands?.deny ?? [] })
  }
}
const defaults = parseToml(read('permissions/default.toml')).default ?? { description: '', permissions: [] }
check(permissions.length > 0, 'no permissions found under permissions/autogenerated')
const permissionIds = new Set(permissions.map((p) => p.identifier))

function permissionsPage() {
  const inDefault = new Set(defaults.permissions)
  const rows = permissions
    .map((p) => `| ${cell(`serialplugin:${p.identifier}`)} | ${oneLine(p.description)} | ${inDefault.has(p.identifier) ? '✓' : ''} |`)
    .join('\n')
  const body = [
    `Generated from [\`permissions/default.toml\`](${repo}permissions/default.toml) and`,
    `[\`permissions/autogenerated/\`](${repo}permissions/autogenerated), which Tauri writes from the command list.`,
    'How to use them in a capability: [Permissions guide](/guides/permissions).',
    '',
    '## Default permission set',
    '',
    'Identifier: `serialplugin:default`',
    '',
    defaults.description.trim(),
    '',
    `It includes ${defaults.permissions.length} permissions:`,
    '',
    defaults.permissions.map((p) => `\`${p}\``).join(', '),
    '',
    '## All permissions',
    '',
    '| Identifier | Description | In default |',
    '|------------|-------------|------------|',
    rows,
  ].join('\n')
  return page('Permissions', 'Every permission identifier of the plugin, generated from permissions/.', 'permissions/', body)
}

// ---------------------------------------------------------------- Tauri commands (src/commands.rs)

// Splits `a: T<X, Y>, b: U` on top-level commas.
function splitTopLevel(text) {
  const parts = []
  let depth = 0
  let cur = ''
  for (const ch of text) {
    if ('(<['.includes(ch)) depth++
    if (')>]'.includes(ch)) depth--
    if (ch === ',' && depth === 0) {
      parts.push(cur)
      cur = ''
    } else cur += ch
  }
  if (cur.trim()) parts.push(cur)
  return parts.map((p) => p.trim()).filter(Boolean)
}

const camel = (s) => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase())

function parseCommands(src) {
  const lines = src.split('\n')
  const commands = []
  let doc = []
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim()
    if (line.startsWith('///')) {
      doc.push(line.replace(/^\/\/\/ ?/, ''))
      continue
    }
    if (line !== '#[tauri::command]') {
      if (!line.startsWith('#[')) doc = []
      continue
    }
    // Skip further attributes, then read the signature up to the body's `{`.
    let j = i + 1
    while (lines[j].trim().startsWith('#[')) j++
    let sig = ''
    while (!sig.includes('{')) sig += lines[j++] + '\n'
    sig = sig.slice(0, sig.indexOf('{'))
    const m = sig.match(/pub\s+(async\s+)?fn\s+(\w+)\s*(<[^(]*>)?\s*\(/)
    check(m, `cannot parse command signature near line ${i + 1} of src/commands.rs`)
    const open = sig.indexOf('(', m.index + m[0].length - 1)
    let depth = 0
    let close = open
    for (; close < sig.length; close++) {
      if (sig[close] === '(') depth++
      if (sig[close] === ')' && --depth === 0) break
    }
    const params = splitTopLevel(sig.slice(open + 1, close))
      .map((p) => {
        const [name, ...type] = p.split(':')
        return { name: name.trim(), type: type.join(':').trim() }
      })
      // Injected by Tauri, not passed from JS.
      .filter((p) => !/^(AppHandle|State|Window|WebviewWindow|Webview)\b/.test(p.type))
    const ret = sig.slice(close + 1).replace(/^\s*->\s*/, '').trim()
    const ok = ret.match(/^Result<([\s\S]*),\s*Error>$/)?.[1].trim() ?? ret
    commands.push({
      name: m[2],
      async: Boolean(m[1]),
      params,
      returns: ok.replace(/\s+/g, ' '),
      // "See <site>/reference/commands" lines point back at this very page.
      doc: doc.filter((l) => !/^See (docs\/|https:\/\/s00d\.github\.io\/)/.test(l)).join('\n').trim(),
    })
    doc = []
    i = j - 1
  }
  return commands
}

function commandsPage() {
  const commands = parseCommands(read('src/commands.rs'))
  check(commands.length > 0, 'no #[tauri::command] functions found in src/commands.rs')

  const noPermission = commands.filter((c) => !permissionIds.has(`allow-${c.name.replace(/_/g, '-')}`)).map((c) => c.name)
  if (noPermission.length) console.warn(`generate.mjs: commands without an allow-* permission: ${noPermission.join(', ')}`)

  const summary = commands.map((c) => `| [${cell(c.name)}](#${c.name}) | ${oneLine(c.doc.split('\n')[0])} |`).join('\n')
  const details = commands
    .map((c) => {
      const permission = `allow-${c.name.replace(/_/g, '-')}`
      const params = c.params.length
        ? '| Argument (JS) | Rust parameter | Type |\n|---------------|----------------|------|\n' +
          c.params.map((p) => `| ${cell(camel(p.name))} | ${cell(p.name)} | ${cell(p.type)} |`).join('\n')
        : 'No arguments.'
      return [
        `### \`${c.name}\``,
        '',
        c.doc,
        '',
        '```ts',
        `invoke('plugin:serialplugin|${c.name}'${c.params.length ? `, { ${c.params.map((p) => camel(p.name)).join(', ')} }` : ''})`,
        '```',
        '',
        params,
        '',
        `Returns ${cell(c.returns)}. ${c.async ? 'Rust: `async fn`.' : 'Rust: plain (sync) `fn`.'}` +
          (permissionIds.has(permission) ? ` Permission: ${cell(`serialplugin:${permission}`)}.` : ''),
        '',
      ].join('\n')
    })
    .join('\n')

  const body = [
    `The \`#[tauri::command]\` functions in [\`src/commands.rs\`](${repo}src/commands.rs), which the JS package calls`,
    'with `invoke(\'plugin:serialplugin|<name>\', args)`. You normally use the [`SerialPort` class](/reference/serial-port)',
    'instead; this list is for calling commands directly or from Rust (see [Rust](/guides/rust)).',
    'Arguments are sent in camelCase; `AppHandle` and `State` parameters are injected by Tauri and not listed.',
    '',
    '| Command | Description |',
    '|---------|-------------|',
    summary,
    '',
    '## Commands',
    '',
    details,
  ].join('\n')
  return page('Tauri commands', 'Every Tauri command of the plugin with its arguments, generated from src/commands.rs.', 'src/commands.rs', body)
}

// ---------------------------------------------------------------- Changelog (CHANGELOG.md)

function changelogPage() {
  let body = read('CHANGELOG.md').replace(/^# .*\n/, '')
  // Relative links would 404 on the site.
  body = body.replace(/\]\((?!https?:|#|\/|mailto:)([^)]+)\)/g, (_, target) => {
    if (target === 'docs/migration.md') return '](/migration)'
    return `](${repo}${target})`
  })
  return page('Changelog', 'All notable changes to tauri-plugin-serialplugin and tauri-plugin-serialplugin-api.', 'CHANGELOG.md', body)
}

// ---------------------------------------------------------------- write

rmSync(refDir, { recursive: true, force: true })
mkdirSync(refDir, { recursive: true })
const out = {
  [`${refDir}.navigation.yml`]: 'title: API reference\nicon: i-lucide-file-code\n',
  [`${refDir}1.serial-port.md`]: serialPortPage(),
  [`${refDir}2.types.md`]: typesPage(),
  [`${refDir}3.commands.md`]: commandsPage(),
  [`${refDir}4.permissions.md`]: permissionsPage(),
  [`${content}9.changelog.md`]: changelogPage(),
}
for (const [file, text] of Object.entries(out)) {
  writeFileSync(file, text)
  console.log(`generated ${file.slice(root.length)}`)
}
