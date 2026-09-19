import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { sha256 } from './hash.mjs';

function run(command, args, cwd) {
    const result = spawnSync(command, args, { cwd, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
    if (result.error || result.status !== 0) throw new Error(`${command} failed: ${result.error?.message ?? result.stderr}`);
    return result.stdout;
}

function files(directory, root = directory) {
    return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
        const path = join(directory, entry.name);
        // macOS framework and DMG Applications symlinks are not notice files.
        if (entry.isSymbolicLink()) return [];
        return entry.isDirectory() ? files(path, root) : [relative(root, path).replaceAll('\\', '/')];
    });
}

function localFile(directory, name) {
    const path = resolve(directory, name);
    const local = relative(realpathSync(directory), realpathSync(path));
    if (isAbsolute(name) || local === '..' || local.startsWith(`..${sep}`) || isAbsolute(local) || !lstatSync(path).isFile()) {
        throw new Error(`Notice path escapes its directory: ${name}`);
    }
    if (!readFileSync(path).length) throw new Error(`Empty notice: ${name}`);
    return path;
}

// This validates a freshly extracted final artifact, never a pre-package staging
// directory. It checks that the notices a recipient is entitled to actually
// shipped and are readable - not that anyone reviewed them.
export function verifyExtracted(directory, products) {
    const entries = files(directory);
    const inventories = entries.filter(path => /(?:^|\/)licenses\/(?:.*\/)?inventory\.json$/.test(path));
    const found = new Set();
    for (const path of inventories) {
        const base = dirname(join(directory, path));
        const inventory = JSON.parse(readFileSync(join(directory, path), 'utf8'));
        found.add(inventory.product);
        const html = readFileSync(localFile(base, 'rust-licenses.html'), 'utf8');
        if (!html.includes('<html') || !html.includes('DoubleSlash')) throw new Error(`Unreadable license HTML: ${path}`);
        localFile(base, 'DoubleSlash-LICENSE.txt');
        // localFile throws on a missing or empty notice, which is the failure
        // that would leave a recipient without a licence text they are owed.
        for (const file of inventory.supplement ?? []) localFile(join(base, 'supplement'), file);
        for (const native of inventory.native ?? []) localFile(base, native.notice);
        for (const match of html.matchAll(/href="([^"]+)"/g)) {
            if (!/^https:\/\//.test(match[1])) localFile(base, match[1]);
        }
    }
    for (const product of products) if (!found.has(product)) throw new Error(`Final artifact is missing ${product} notices`);
    // Every packaged ABI must have its own target inventory, even if Gradle
    // accidentally retained an old .so in jniLibs.
    if (products.includes('android')) {
        const targets = { 'arm64-v8a': 'aarch64-linux-android', 'armeabi-v7a': 'armv7-linux-androideabi', x86_64: 'x86_64-linux-android', x86: 'i686-linux-android' };
        for (const path of entries) {
            const abi = path.match(/(?:^|\/)lib\/([^/]+)\/[^/]+\.so$/)?.[1];
            if (abi && (!targets[abi] || !inventories.some(path => path.includes(`/licenses/${targets[abi]}/`)))) {
                throw new Error(`Packaged ABI has no license inventory: ${abi}`);
            }
        }
    }
    return { inventories: inventories.length, products: [...found] };
}

export function verifyArtifact(artifact, products) {
    artifact = resolve(artifact);
    if (!lstatSync(artifact).isFile()) throw new Error('Supply a final archive, not a staging directory');
    const temporary = mkdtempSync(join(tmpdir(), 'doubleslash-artifact-'));
    let mounted = false;
    let directory = temporary;
    try {
        if (/\.dmg$/i.test(artifact)) {
            directory = join(temporary, 'mounted');
            run('hdiutil', ['attach', '-readonly', '-nobrowse', '-mountpoint', directory, artifact]);
            mounted = true;
        } else if (/\.AppImage$/i.test(artifact)) {
            run(artifact, ['--appimage-extract'], temporary);
            directory = join(temporary, 'squashfs-root');
        } else if (/\.tar\.gz$/i.test(artifact)) {
            run('tar', ['-xzf', artifact, '-C', temporary]);
        } else if (/\.(?:7z|zip|apk|aab)$/i.test(artifact)) {
            const sevenZip = process.env.SEVENZIP ?? (process.platform === 'win32' && existsSync('C:/Program Files/7-Zip/7z.exe') ? 'C:/Program Files/7-Zip/7z.exe' : '7z');
            run(sevenZip, ['x', '-y', `-o${temporary}`, artifact]);
        } else throw new Error('Unsupported release archive');
        return { artifact, sha256: sha256(readFileSync(artifact)), ...verifyExtracted(directory, products) };
    } finally {
        if (mounted) run('hdiutil', ['detach', directory]);
        rmSync(temporary, { recursive: true, force: true });
    }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try {
        const args = process.argv.slice(2);
        if (args.length < 2) throw new Error('Usage: node scripts/licenses/verify_artifact.mjs ARCHIVE PRODUCT [PRODUCT...]');
        console.log(JSON.stringify(verifyArtifact(args[0], args.slice(1)), null, 2));
    } catch (error) { console.error(error.message); process.exitCode = 1; }
}
