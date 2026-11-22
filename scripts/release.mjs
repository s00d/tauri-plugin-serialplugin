#!/usr/bin/env node
// Release steps of the CI `release` job (PUBLISHING.md).
//
//   node scripts/release.mjs prep X.Y.Z    versions, lockfiles, CHANGELOG `## Unreleased` -> X.Y.Z
//   node scripts/release.mjs notes X.Y.Z   the CHANGELOG section (GitHub release body)
//   node scripts/release.mjs pending       crates not on crates.io at their version, in publish order
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const PLUGIN = 'tauri-plugin-serialplugin';
/** Dependency order. The plugin's version is the release version; the others are bumped by hand. */
const CRATES = ['serial-codec', 'android-usb-serial', 'serialplugin-core', PLUGIN];

const read = (file) => readFileSync(join(root, file), 'utf8');
const write = (file, text) => writeFileSync(join(root, file), text);
const run = (cmd, ...args) =>
  execFileSync(cmd, args, { cwd: root, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'inherit'] });
const fail = (message) => {
  console.error(`::error::${message}`);
  process.exit(1);
};

function packages() {
  const all = JSON.parse(run('cargo', 'metadata', '--no-deps', '--format-version=1')).packages;
  return CRATES.map((name) => all.find((p) => p.name === name) ?? fail(`no crate ${name} in the workspace`));
}

/** Sparse index: one JSON line per published version; 404 is a crate never published. */
async function isPublished({ name, version }) {
  const res = await fetch(`https://index.crates.io/${name.slice(0, 2)}/${name.slice(2, 4)}/${name}`);
  if (res.status === 404) return false;
  if (!res.ok) fail(`crates.io index: ${res.status} for ${name}`);
  const lines = (await res.text()).trim().split('\n');
  return lines.some((line) => JSON.parse(line).vers === version);
}

/** A changed crate must have a new version, and its dependents must require it. */
async function checkCrates() {
  const pkgs = packages();
  const lastTag = run('git', 'describe', '--tags', '--abbrev=0', '--match', 'v*').trim();
  for (const pkg of pkgs) {
    const dir = `crates/${pkg.name}`;
    if (pkg.name !== PLUGIN && (await isPublished(pkg))) {
      const changed = run('git', 'diff', '--name-only', lastTag, '--', `${dir}/src`, `${dir}/Cargo.toml`).trim();
      if (changed) {
        fail(`${pkg.name} ${pkg.version} is on crates.io but changed since ${lastTag}, bump it: ${changed.replaceAll('\n', ' ')}`);
      }
    }
    for (const dep of pkg.dependencies.filter((d) => d.path)) {
      const { version } = pkgs.find((p) => p.name === dep.name);
      if (dep.req !== `^${version}`) fail(`${pkg.name} requires ${dep.name} ${dep.req}, must be ${version}`);
    }
  }
}

function section(changelog, heading) {
  const start = changelog.match(heading);
  if (!start) return null;
  const bodyAt = start.index + start[0].length;
  const next = changelog.slice(bodyAt).search(/^## /m);
  const end = next < 0 ? changelog.length : bodyAt + next;
  return { start: start.index, bodyAt, body: changelog.slice(bodyAt, end).trim() };
}

function replaceOnce(file, pattern, text) {
  const before = read(file);
  if (!pattern.test(before)) fail(`${file}: no ${pattern}`);
  write(file, before.replace(pattern, text));
}

async function prep(version) {
  const previous = JSON.parse(read('package.json')).version;
  if (previous === version) fail(`the version is already ${version}`);

  const changelog = read('CHANGELOG.md');
  const unreleased = section(changelog, /^## Unreleased[^\n]*\n/m);
  if (!unreleased?.body) fail("CHANGELOG.md: write the notes under '## Unreleased' first");
  await checkCrates();

  const compare = `https://github.com/s00d/${PLUGIN}/compare/v${previous}...v${version}`;
  const date = new Date().toISOString().slice(0, 10);
  write(
    'CHANGELOG.md',
    `${changelog.slice(0, unreleased.start)}## [${version}](${compare}) (${date})\n${changelog.slice(unreleased.bodyAt)}`,
  );

  replaceOnce('Cargo.toml', /^version = "[^"]+"/m, `version = "${version}"`);
  replaceOnce('package.json', /^ {2}"version": "[^"]+"/m, `  "version": "${version}"`);
  // Rewrites the workspace members' versions in the tracked lockfiles, nothing else.
  run('cargo', 'metadata', '--format-version=1');
  run('cargo', 'metadata', '--format-version=1', '--manifest-path', 'examples/serialport-test/src-tauri/Cargo.toml');
  console.log(`prepared ${version} (was ${previous})`);
}

const [command, version] = process.argv.slice(2);
if (command !== 'pending' && !/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version ?? '')) {
  fail(`'${version ?? ''}' is not a version like 4.2.3`);
}
switch (command) {
  case 'prep':
    await prep(version);
    break;
  case 'notes': {
    const found = section(read('CHANGELOG.md'), new RegExp(`^## \\[${version.replaceAll('.', '\\.')}\\][^\\n]*\\n`, 'm'));
    if (!found?.body) fail(`CHANGELOG.md has no section for ${version}`);
    console.log(found.body);
    break;
  }
  case 'pending':
    for (const pkg of packages()) if (!(await isPublished(pkg))) console.log(pkg.name);
    break;
  default:
    fail(`unknown command '${command ?? ''}'`);
}
