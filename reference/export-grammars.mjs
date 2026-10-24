import fs from 'node:fs';
import path from 'node:path';
import { theme } from './node_modules/@wooorm/starry-night/lib/theme.js';

const out = process.argv[2];
const langDir = 'node_modules/@wooorm/starry-night/lang';
const roots = [
  'source.shell', 'source.json', 'source.json.comments', 'source.yaml', 'source.toml', 'source.diff', 'text.md',
  'source.c', 'source.c++', 'source.objc', 'source.objc++', 'source.java', 'source.kotlin', 'source.scala', 'source.swift',
  'source.lua', 'source.perl', 'source.r', 'source.sql', 'source.makefile', 'source.dockerfile', 'source.ini',
  'source.graphql', 'text.xml', 'source.haskell', 'source.ocaml', 'source.elixir', 'source.erlang', 'source.clojure',
  'source.julia', 'source.dart', 'source.powershell', 'source.proto', 'source.hcl', 'source.hcl.terraform', 'source.nix',
  'source.viml', 'source.zig', 'source.regexp', 'source.mermaid', 'text.tex.latex', 'source.groovy', 'source.gitconfig',
  'source.gitignore', 'source.nginx', 'source.css.less', 'source.css.scss', 'source.vue', 'source.svelte', 'source.batchfile',
  'source.fsharp', 'source.cs', 'source.solidity', 'source.prisma', 'source.jq', 'source.awk', 'source.cmake', 'source.just',
  'source.fish', 'source.tsql', 'source.plpgsql', 'source.astro', 'source.groq', 'source.vb', 'source.coffee', 'source.d',
  'source.crystal', 'source.nim', 'source.v', 'source.odin', 'source.gleam', 'source.terraform', 'text.html.erb',
  'source.ruby', 'source.css', 'text.html.basic', 'source.js', 'source.ts', 'source.tsx', 'source.python', 'source.go', 'source.rust',
];

const KEEP = ['name', 'scopeName', 'patterns', 'repository', 'injections', 'injectionSelector', 'fileTypes'];
const seen = new Map();
const missing = new Set();

async function load(scope) {
  if (seen.has(scope) || missing.has(scope)) return;
  const file = path.join(langDir, `${scope}.js`);
  if (!fs.existsSync(file)) { missing.add(scope); return; }
  const g = (await import('./' + file)).default;
  seen.set(scope, g);
  const deps = new Set();
  JSON.stringify(g, (k, v) => {
    if (k === 'include' && typeof v === 'string' && /^(source|text)\./.test(v)) deps.add(v.split('#')[0]);
    return v;
  });
  for (const d of deps) await load(d);
}

for (const r of roots) await load(r);
fs.mkdirSync(path.join(out, 'grammars'), { recursive: true });
const aliases = {};
for (const [scope, g] of seen) {
  const clean = Object.fromEntries(KEEP.filter((k) => g[k] !== undefined).map((k) => [k, g[k]]));
  clean.name = scope;
  fs.writeFileSync(path.join(out, 'grammars', `${scope}.json`), JSON.stringify(clean));
  aliases[scope] = [...new Set([...(g.names || []), ...(g.extensions || []).map((e) => e.replace(/^\./, ''))])].map((s) => s.toLowerCase());
}
fs.writeFileSync(path.join(out, 'aliases.json'), JSON.stringify(aliases));
const { classes } = await import('./node_modules/@wooorm/starry-night/lib/theme.js');
const scopes = {};
for (const s of theme.settings) {
  if (!s.scope) continue;
  const v = s.settings;
  const cls = v.fontStyle ? 'pl-s' : classes[Number((v.foreground || v.background).slice(1))];
  for (const sc of s.scope) scopes[sc] = cls;
}
fs.writeFileSync(path.join(out, 'scopes.json'), JSON.stringify(scopes));
console.log(JSON.stringify({ grammars: seen.size, missing: [...missing] }));
