import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const aboutVersion = '0.8.4';
const products = new Set(['client', 'installer', 'supernode', 'android']);

export function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, character => ({
        '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
    })[character]);
}

export function renderLicenses(data, projectLicense = '', revision = '', embeddedNotices = []) {
    if (!Array.isArray(data.licenses) || data.licenses.length === 0) {
        throw new Error('No resolved license texts were generated');
    }
    const sections = data.licenses.map(license => {
        if (!license.text?.trim() || !license.used_by?.length) {
            throw new Error(`Incomplete license evidence: ${license.id}`);
        }
        const crates = license.used_by.map(({ crate }) => {
            let source;
            if (crate.source === 'registry+https://github.com/rust-lang/crates.io-index') {
                source = `https://crates.io/api/v1/crates/${encodeURIComponent(crate.name)}/${encodeURIComponent(crate.version)}/download`;
            } else if (!crate.source && crate.manifest_path && revision) {
                const path = relative(root, dirname(crate.manifest_path)).split(sep).map(encodeURIComponent).join('/');
                source = `https://github.com/DoubleSlashSpace/DoubleSlash/tree/${encodeURIComponent(revision)}/${path}`;
            }
            return `<li>${escapeHtml(crate.name)} ${escapeHtml(crate.version)}${source ? ` (<a href="${escapeHtml(source)}">corresponding source</a>)` : ''}</li>`;
        }).join('\n');
        return `<section><h2>${escapeHtml(license.name)}</h2><ul>${crates}</ul><pre>${escapeHtml(license.text)}</pre></section>`;
    });
    return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>DoubleSlash Third-Party Licenses</title>
<style>body { max-width: 76rem; margin: 2rem auto; padding: 0 1rem; font-family: sans-serif; } pre { white-space: pre-wrap; overflow-wrap: anywhere; }</style>
</head><body><h1>DoubleSlash Third-Party Licenses</h1>
<p>Rust dependencies for the target and features in inventory.json. Other bundled components have separate notices in this directory.</p>
<h2>DoubleSlash License</h2><pre>${escapeHtml(projectLicense)}</pre>
${sections.join('\n')}
${embeddedNotices.map(notice => `<section><h2>${escapeHtml(notice.name)}</h2><p><a href="${escapeHtml(notice.source)}">Source archive</a></p><pre>${escapeHtml(notice.text)}</pre></section>`).join('\n')}
</body></html>\n`;
}

function run(command, args, cwd = root) {
    const result = spawnSync(command, args, { cwd, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 });
    if (result.error) throw result.error;
    if (result.status !== 0) {
        throw new Error(`${command} ${args.join(' ')} failed:\n${result.stderr || result.stdout}`);
    }
    if (result.stderr) process.stderr.write(result.stderr);
    return result.stdout;
}

// Extra notice files that accompany a product beyond its Rust dependencies -
// Qt, Chromium, FFmpeg, the MSVC redistributables. They are plain texts copied
// into the package as they are. Convention over configuration: a build finds
// them by target and product, so a release cannot ship without the notices
// because someone forgot to set an environment variable.
export function supplementDirectory(product, target, override) {
    const directory = override ?? join(root, 'packaging/licenses', target, product);
    return existsSync(directory) ? directory : null;
}

export function main(args) {
    const { values } = parseArgs({ args, options: {
        product: { type: 'string' }, target: { type: 'string' }, output: { type: 'string' },
        features: { type: 'string', default: '' }, supplement: { type: 'string' },
    } });
    const { product, target, output, features } = values;
    if (!products.has(product) || !target || !output) {
        throw new Error('Usage: node scripts/generate_licenses.mjs --product client|installer|supernode|android --target TRIPLE --output DIRECTORY [--features FEATURES] [--supplement DIRECTORY]');
    }
    const workspace = ['client', 'android'].includes(product) ? `rust/doubleslash-${product}` : 'rust';
    const lockHash = createHash('sha256').update(readFileSync(join(root, workspace, 'Cargo.lock'))).digest('hex');
    const supplement = supplementDirectory(product, target, values.supplement);
    const version = run('cargo', ['about', '--version']).trim();
    if (version !== `cargo-about ${aboutVersion}`) {
        throw new Error(`Expected cargo-about ${aboutVersion}; install with cargo install cargo-about --version ${aboutVersion} --locked`);
    }
    const manifest = join(root, 'rust', `doubleslash-${product}`, 'Cargo.toml');
    const command = ['about', 'generate', '--manifest-path', manifest, '--config',
        join(root, 'scripts/licenses/about.toml'), '--locked', '--fail', '--target', target, '--format', 'json'];
    if (features) command.push('--features', features.replaceAll(',', ' '));
    const temporary = mkdtempSync(join(tmpdir(), 'doubleslash-licenses-'));
    let data;
    try {
        const json = join(temporary, 'licenses.json');
        run('cargo', [...command, '--output-file', json], dirname(manifest));
        data = JSON.parse(readFileSync(json, 'utf8'));
    } finally {
        rmSync(temporary, { recursive: true, force: true });
    }
    const revision = run('git', ['rev-parse', 'HEAD']).trim();
    const embeddedNotices = [];
    const fonts = data.licenses.flatMap(license => license.used_by.map(entry => entry.crate))
        .find(crate => crate.name === 'epaint_default_fonts');
    if (fonts) {
        for (const file of ['Hack-Regular.txt', 'emoji-icon-font-mit-license.txt', 'OFL.txt', 'UFL.txt']) {
            embeddedNotices.push({ name: `epaint_default_fonts ${fonts.version}: ${file}`,
                text: readFileSync(join(dirname(fonts.manifest_path), 'fonts', file), 'utf8'),
                source: `https://crates.io/api/v1/crates/epaint_default_fonts/${fonts.version}/download` });
        }
    }
    // First-party and patched crates link to the public tree at the revision
    // that was built. Nothing in any product's graph obliges source delivery:
    // the permissive licences are notice-only, the one MPL-2.0 crate is
    // unmodified from crates.io and links there, and Qt's LGPL source is
    // published by The Qt Company, named in the supplement notices.
    const html = renderLicenses(data, readFileSync(join(root, 'LICENSE'), 'utf8'), revision, embeddedNotices);
    const destination = resolve(output);
    mkdirSync(destination, { recursive: true });
    copyFileSync(join(root, 'LICENSE'), join(destination, 'DoubleSlash-LICENSE.txt'));
    writeFileSync(join(destination, 'rust-licenses.html'), html);
    const crates = [...new Map(data.licenses.flatMap(license => license.used_by.map(({ crate }) =>
        [crate.id, { name: crate.name, version: crate.version, license: crate.license, source: crate.source, repository: crate.repository }]))).values()]
        .sort((left, right) => `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`));
    const native = [];
    if (['client', 'android'].includes(product)) {
        const files = [
            ['rust/doubleslash-opus/opus/COPYING', 'opus-COPYING.txt'],
            ['rust/doubleslash-opus/opus/LICENSE_PLEASE_READ.txt', 'opus-patent-information.txt'],
            ['rust/doubleslash-vpx/libvpx/LICENSE', 'libvpx-LICENSE.txt'],
            ['rust/doubleslash-vpx/libvpx/PATENTS', 'libvpx-PATENTS.txt'],
        ];
        for (const [source, name] of files) {
            copyFileSync(join(root, source), join(destination, name));
            native.push({ source, notice: name });
        }
    }
    const supplementNotices = [];
    if (supplement) {
        for (const file of readdirSync(supplement).sort()) {
            if (file === 'README.md' || file === 'review.json') continue;
            if (!statSync(join(supplement, file)).isFile()) continue;
            const targetFile = join(destination, 'supplement', file);
            mkdirSync(dirname(targetFile), { recursive: true });
            copyFileSync(join(supplement, file), targetFile);
            supplementNotices.push(file);
        }
    }
    const inventory = {
        product, target, features, dependencyLockSha256: lockHash, generator: version,
        sourceRevision: revision, crates, native, supplement: supplementNotices,
    };
    writeFileSync(join(destination, 'inventory.json'), JSON.stringify(inventory, null, 2) + '\n');
    console.log(`Generated ${crates.length} Rust dependency entries for ${product} (${target}) in ${destination}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try { main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
