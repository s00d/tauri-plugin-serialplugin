#!/usr/bin/env node
/**
 * Sync root Cargo.toml [package].version with the release-it / package.json version.
 * Only rewrites the first `version = "..."` line (the crate package version).
 */
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const version = process.argv[2] || require(path.join(root, 'package.json')).version;
const cargoPath = path.join(root, 'Cargo.toml');
const before = fs.readFileSync(cargoPath, 'utf8');
if (!/^version = "[^"]+"/m.test(before)) {
  console.error('sync-cargo-version: did not find package version line in Cargo.toml');
  process.exit(1);
}
const after = before.replace(/^version = "[^"]+"/m, `version = "${version}"`);
if (before !== after) {
  fs.writeFileSync(cargoPath, after);
}
console.log(`Cargo.toml version -> ${version}`);
