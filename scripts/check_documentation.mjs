// Check repository-owned Markdown links and GitHub-style heading anchors.
// Vendored upstream documentation is intentionally outside this check.
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { dirname, resolve, relative, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repositoryPaths = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'], {
  cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
}).split('\0').filter(Boolean);
const files = repositoryPaths.filter(path => /\.md$/i.test(path)
  && !/^(rust\/patches\/|rust\/doubleslash-opus\/opus\/|rust\/doubleslash-vpx\/libvpx\/|\.claude\/)/.test(path));

function prose(text) {
  let fence;
  return text.split(/\r?\n/).map(line => {
    const marker = line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if (marker) {
      if (!fence) fence = marker[1];
      else if (marker[1][0] === fence[0] && marker[1].length >= fence.length) fence = undefined;
      return '';
    }
    return fence ? '' : line;
  }).join('\n');
}

const anchorCache = new Map();
function anchors(path) {
  if (anchorCache.has(path)) return anchorCache.get(path);
  const text = prose(readFileSync(path, 'utf8'));
  const result = new Set();
  const counts = new Map();
  for (const line of text.split('\n')) {
    const heading = line.match(/^ {0,3}#{1,6}\s+(.+?)(?:\s+#+)?\s*$/);
    if (heading) {
      const slug = heading[1].replace(/<[^>]+>/g, '').toLowerCase()
        .replace(/[^\p{L}\p{N}\p{M}_\-\s]/gu, '').replace(/\s/g, '-');
      const count = counts.get(slug) ?? 0;
      counts.set(slug, count + 1);
      result.add(count ? `${slug}-${count}` : slug);
    }
  }
  for (const match of text.matchAll(/\b(?:id|name)=["']([^"']+)["']/g)) result.add(match[1]);
  anchorCache.set(path, result);
  return result;
}

function exactCase(path) {
  const local = relative(root, path).replaceAll('\\', '/');
  if (local.startsWith('..')) return true;
  // Git's spelling is authoritative on a case-insensitive Windows checkout.
  const entries = repositoryPaths.filter(entry => entry.toLowerCase() === local.toLowerCase()
    || entry.toLowerCase().startsWith(`${local.toLowerCase()}/`));
  return !entries.length || entries.some(entry => entry === local || entry.startsWith(`${local}/`));
}

let checked = 0;
const errors = [];
for (const file of [...new Set(files)].sort()) {
  const path = resolve(root, file);
  const lines = prose(readFileSync(path, 'utf8')).split('\n');
  for (const [index, line] of lines.entries()) {
    const targets = [...line.matchAll(/!?\[[^\]]*\]\(\s*(<[^>]+>|[^\s)]+)(?:\s+["'][^"']*["'])?\s*\)/g)]
      .map(match => match[1].replace(/^<|>$/g, ''));
    const reference = line.match(/^\s{0,3}\[[^\]]+\]:\s*(<[^>]+>|\S+)/);
    if (reference) targets.push(reference[1].replace(/^<|>$/g, ''));
    for (const target of targets) {
      if (/^(?:[a-z][\w+.-]*:|\/\/)/i.test(target)) continue;
      checked++;
      const [rawPath, rawAnchor] = target.split('#');
      const dest = rawPath ? resolve(dirname(path), decodeURIComponent(rawPath)) : path;
      const where = `${file}:${index + 1}: ${target}`;
      if (!existsSync(dest)) errors.push(`${where} (missing target)`);
      else if (!exactCase(dest)) errors.push(`${where} (path capitalization differs)`);
      else if (rawAnchor && extname(dest).toLowerCase() === '.md'
        && !anchors(dest).has(decodeURIComponent(rawAnchor))) errors.push(`${where} (missing anchor)`);
    }
  }
}
if (errors.length) {
  process.stderr.write(errors.join('\n') + '\n');
  process.exitCode = 1;
}
process.stdout.write(`Checked ${checked} local links in ${new Set(files).size} Markdown files; ${errors.length} errors.\n`);
