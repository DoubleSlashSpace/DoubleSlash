import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

function trackedFiles(directory) {
    const result = spawnSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], {
        cwd: directory, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024,
    });
    if (result.status !== 0) throw new Error(`Cannot inventory source files: ${result.stderr}`);
    return [...new Set(result.stdout.split('\0').filter(Boolean))];
}

// Explicitly include ignored generated model arrays. Their presence or a clean
// submodule revision alone says nothing about the bytes compiled into the app.
export function sourceFiles(root = repository) {
    const files = trackedFiles(root).filter(path => /^(rust\/|android\/|assets\/|games\/|web\/|scripts\/|packaging\/(?!licenses\/)|\.github\/workflows\/|build_[^/]+|LICENSE$|\.gitmodules$)/.test(path));
    for (const vendor of ['rust/doubleslash-opus/opus', 'rust/doubleslash-vpx/libvpx']) {
        if (!existsSync(join(root, vendor))) continue;
        files.push(...trackedFiles(join(root, vendor)).map(path => `${vendor}/${path}`));
    }
    const list = join(root, 'rust/doubleslash-opus/opus/tar_list.txt');
    if (existsSync(list)) {
        for (const name of readFileSync(list, 'utf8').split(/\r?\n/).filter(Boolean)) {
            if (name.startsWith('/') || name.includes('\\') || name.split('/').includes('..')) throw new Error('Unsafe model source path');
            const file = `rust/doubleslash-opus/opus/${name}`;
            if (existsSync(join(root, file))) files.push(file);
        }
    }
    return [...new Set(files)].filter(path => existsSync(join(root, path)) && lstatSync(join(root, path)).isFile()).sort();
}

export function reviewInputs(root = repository) {
    const files = sourceFiles(root).filter(path =>
        /(?:^|\/)(?:Cargo\.(?:toml|lock)|build\.rs|[^/]*\.gradle\.kts|gradle\.properties|libs\.versions\.toml)$/.test(path) ||
        /^(?:build_|scripts\/|\.github\/workflows\/|assets\/|games\/|web\/|rust\/(?:assets|patches)\/|rust\/doubleslash-(?:opus|vpx)\/|rust\/doubleslash-client\/qml\/|android\/app\/src\/main\/(?:assets|res)\/|android\/gradle\/)/.test(path) ||
        path.startsWith('packaging/') || /\.(?:html|m?js|css|ttf|otf|woff2?|wav|ogg|mp3|svg|png|ico)$/.test(path));
    const inputs = files.map(path => ({ path, sha256: sha256(readFileSync(join(root, path))) }));
    return { inputsSha256: sha256(JSON.stringify(inputs)), inputs };
}

export function archiveFiles(product, root = repository) {
    // Preserve workspace members as well as the selected crate: Cargo parses
    // every member manifest even when building only one package.
    return sourceFiles(root).filter(path =>
        !path.startsWith('.github/') && !path.startsWith('rust/doubleslash-supernode-manager/') &&
        (!path.startsWith('android/') || product === 'android'))
        .filter(path => !/\/dnn\/models\//.test(path));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const report = reviewInputs();
    if (process.argv[2]) writeFileSync(resolve(process.argv[2]), JSON.stringify(report, null, 2) + '\n');
    else console.log(JSON.stringify(report, null, 2));
}
