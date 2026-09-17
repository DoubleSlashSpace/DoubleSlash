import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { reviewInputs, archiveFiles, sha256 } from './licenses/review_inputs.mjs';

export const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const aboutVersion = '0.8.4';
const products = new Set(['client', 'installer', 'supernode', 'android']);

export function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, character => ({
        '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
    })[character]);
}

export function renderLicenses(data, projectLicense = '', revision = '', hasSourceSnapshot = false, embeddedNotices = []) {
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
            } else if (!crate.source && hasSourceSnapshot) {
                source = 'corresponding-source.tar.gz';
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

export function validateSupplement(directory, product, target, features, lockHash, inputsHash) {
    const manifest = JSON.parse(readFileSync(join(directory, 'review.json'), 'utf8'));
    if (manifest.product !== product || manifest.target !== target ||
        manifest.features !== features || manifest.dependencyLockSha256 !== lockHash || !manifest.reviewedBy?.trim() ||
        !manifest.reviewedAt?.trim() || !Array.isArray(manifest.components) ||
        manifest.components.length === 0) {
        throw new Error('Supplement must identify the reviewed product, target, features, reviewer, date, and components');
    }
    if (!inputsHash || manifest.inputsSha256 !== inputsHash) {
        throw new Error('Supplement has missing or stale native/build/asset input evidence; regenerate review-inputs.json and review the changes');
    }
    if (!Array.isArray(manifest.bundledNativeFiles)) throw new Error('Supplement must inventory the deployed native files');
    for (const file of manifest.bundledNativeFiles) {
        if (!file.path || !/^[a-f0-9]{64}$/.test(file.sha256) ||
            !manifest.components.some(component => component.name === file.component)) {
            throw new Error('Each deployed native file needs a SHA-256 and a reviewed component');
        }
    }
    if (!/^\d{4}-\d{2}-\d{2}$/.test(manifest.reviewedAt) || !Number.isFinite(Date.parse(manifest.reviewedAt)) ||
        manifest.reviewedAt > new Date().toISOString().slice(0, 10)) throw new Error('Invalid supplement review date');
    for (const component of manifest.components) {
        if (!component.name?.trim() || !component.version?.trim() ||
            !component.source?.trim() || !component.obligations?.trim() ||
            !Array.isArray(component.files) || component.files.length === 0) {
            throw new Error('Each supplemental component requires version, source, obligations, and notice files');
        }
        for (const file of component.files) {
            const resolved = resolve(directory, file);
            const local = relative(resolve(directory), resolved);
            if (isAbsolute(file) || local === '..' || local.startsWith(`..${sep}`) || isAbsolute(local)) {
                throw new Error(`Supplement file escapes its directory: ${file}`);
            }
            const actual = relative(realpathSync(directory), realpathSync(resolved));
            if (actual === '..' || actual.startsWith(`..${sep}`) || isAbsolute(actual)) throw new Error(`Supplement symlink escapes its directory: ${file}`);
            if (!readFileSync(resolved, 'utf8').trim()) throw new Error(`Empty supplement: ${file}`);
        }
    }
    return manifest;
}

export function main(args) {
    const { values } = parseArgs({ args, options: {
        product: { type: 'string' }, target: { type: 'string' }, output: { type: 'string' },
        features: { type: 'string', default: '' }, supplement: { type: 'string' },
        'rust-only': { type: 'boolean', default: false },
        'runtime-inventory': { type: 'string' }, variant: { type: 'string' },
    } });
    const { product, target, output, features } = values;
    if (!products.has(product) || !target || !output) {
        throw new Error('Usage: node scripts/generate_licenses.mjs --product client|installer|supernode|android --target TRIPLE --output DIRECTORY [--features FEATURES] [--supplement DIRECTORY] [--rust-only]');
    }
    if (values['rust-only'] && values.supplement) throw new Error('Choose --rust-only or --supplement');
    if (['client', 'android'].includes(product) && !values['rust-only'] && !values.supplement) {
        throw new Error('Distribution requires a reviewed native/runtime/asset supplement. See docs/LICENSING.md. --rust-only is for auditing, not packaging.');
    }
    const workspace = ['client', 'android'].includes(product) ? `rust/doubleslash-${product}` : 'rust';
    const lockHash = createHash('sha256').update(readFileSync(join(root, workspace, 'Cargo.lock'))).digest('hex');
    const inputs = reviewInputs();
    const supplement = values.supplement ? validateSupplement(values.supplement, product, target, features, lockHash, inputs.inputsSha256) : null;
    const runtimeInventoryHash = values['runtime-inventory'] ? sha256(readFileSync(values['runtime-inventory'])) : null;
    if (product === 'android' && supplement && (!runtimeInventoryHash || !values.variant ||
        supplement.runtimeInventorySha256 !== runtimeInventoryHash || supplement.buildVariant !== values.variant)) {
        throw new Error('Android supplement must match the resolved runtime inventory and debug/release variant');
    }
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
    // The installer's HTML also ships by itself. Its first-party MIT code has
    // no corresponding-source obligation; avoid a broken archive link there.
    const html = renderLicenses(data, readFileSync(join(root, 'LICENSE'), 'utf8'), '', ['client', 'supernode'].includes(product), embeddedNotices);
    const destination = resolve(output);
    mkdirSync(destination, { recursive: true });
    // Archive the actual working-tree sources, including modified vendored code
    // and ignored model arrays. Never substitute HEAD for a dirty dependency.
    const snapshotList = join(temporary, 'source-files.txt');
    mkdirSync(temporary, { recursive: true });
    let sourceSnapshotSha256;
    try {
        const files = archiveFiles(product);
        const archived = new Set(files);
        for (const crate of data.licenses.flatMap(license => license.used_by.map(entry => entry.crate))) {
            if (!crate.source && crate.manifest_path) {
                const path = relative(root, crate.manifest_path).replaceAll('\\', '/');
                if (!archived.has(path)) throw new Error(`Local dependency sources are outside the captured inventory: ${crate.name} (${path})`);
            }
        }
        writeFileSync(snapshotList, files.join('\n') + '\n');
        const snapshot = join(destination, 'corresponding-source.tar.gz');
        run('tar', ['-czf', snapshot, '-C', root, '-T', snapshotList]);
        sourceSnapshotSha256 = sha256(readFileSync(snapshot));
    } finally {
        rmSync(temporary, { recursive: true, force: true });
    }
    writeFileSync(join(destination, 'review-inputs.json'), JSON.stringify(inputs, null, 2) + '\n');
    if (values['runtime-inventory']) copyFileSync(values['runtime-inventory'], join(destination, 'runtime-inventory.json'));
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
    if (supplement) {
        for (const component of supplement.components) {
            for (const file of component.files) {
                const targetFile = join(destination, 'supplement', file);
                mkdirSync(dirname(targetFile), { recursive: true });
                copyFileSync(resolve(values.supplement, file), targetFile);
            }
        }
        writeFileSync(join(destination, 'supplement-review.json'), JSON.stringify(supplement, null, 2) + '\n');
    }
    const inventory = {
        product, target, features, dependencyLockSha256: lockHash, generator: version,
        scope: values['rust-only'] ? 'rust-audit-only' : 'distribution-notices',
        sourceRevision: revision,
        sourceSnapshot: { file: 'corresponding-source.tar.gz', sha256: sourceSnapshotSha256 },
        inputsSha256: inputs.inputsSha256,
        buildVariant: values.variant ?? null, runtimeInventorySha256: runtimeInventoryHash,
        crates, native, supplement: supplement ?? null,
    };
    writeFileSync(join(destination, 'inventory.json'), JSON.stringify(inventory, null, 2) + '\n');
    console.log(`Generated ${crates.length} Rust dependency entries for ${product} (${target}) in ${destination}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try { main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
