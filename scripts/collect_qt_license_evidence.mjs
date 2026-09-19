import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join, relative, resolve, isAbsolute, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

// Qt's Windows SBOMs can describe the PE image before Authenticode signing.
// Reconstruct that image only when the certificate occupies the file tail.
// This is an evidence comparison; deployed bytes are never changed.
export function unsignedPeImage(bytes) {
    if (bytes.length < 64 || bytes.toString('ascii', 0, 2) !== 'MZ') return null;
    const pe = bytes.readUInt32LE(60);
    if (pe + 24 > bytes.length || bytes.readUInt32LE(pe) !== 0x4550) return null;
    const optional = pe + 24;
    const optionalSize = bytes.readUInt16LE(pe + 20);
    if (optional + optionalSize > bytes.length || optionalSize < 152) return null;
    const magic = bytes.readUInt16LE(optional);
    if (![0x10b, 0x20b].includes(magic)) return null;
    const security = optional + (magic === 0x20b ? 112 : 96) + 32;
    if (security + 8 > optional + optionalSize) return null;
    const offset = bytes.readUInt32LE(security);
    const size = bytes.readUInt32LE(security + 4);
    if (!size || offset < optional + optionalSize || offset + size !== bytes.length) return null;
    const image = Buffer.from(bytes.subarray(0, offset));
    image.fill(0, optional + 64, optional + 68); // PE checksum, changed by signing
    image.fill(0, security, security + 8); // certificate table directory
    return image;
}

export function matchBinary(bytes, documents, installedHashes = new Map()) {
    const sha1 = createHash('sha1').update(bytes).digest('hex');
    const sha256 = createHash('sha256').update(bytes).digest('hex');
    const unsigned = unsignedPeImage(bytes);
    const unsignedHashes = unsigned ? {
        SHA1: createHash('sha1').update(unsigned).digest('hex'),
        SHA256: createHash('sha256').update(unsigned).digest('hex'),
    } : {};
    const matches = [];
    for (const { name, document } of documents) {
        const packages = new Map((document.packages ?? []).map(pkg => [pkg.SPDXID, pkg]));
        for (const file of document.files ?? []) {
            const upstreamChecksumMatches = Boolean(file.checksums?.some(checksum =>
                (checksum.algorithm === 'SHA1' && checksum.checksumValue.toLowerCase() === sha1) ||
                (checksum.algorithm === 'SHA256' && checksum.checksumValue.toLowerCase() === sha256)));
            const installedFileMatches = installedHashes.get(file.fileName) === sha256;
            const preSigningChecksumMatches = Boolean(file.checksums?.some(checksum =>
                unsignedHashes[checksum.algorithm] === checksum.checksumValue.toLowerCase()));
            if (!upstreamChecksumMatches && !preSigningChecksumMatches && !installedFileMatches) continue;
            const owners = (document.relationships ?? []).filter(relation =>
                relation.relationshipType === 'CONTAINS' && relation.relatedSpdxElement === file.SPDXID)
                .map(relation => packages.get(relation.spdxElementId)).filter(Boolean);
            matches.push({
                sbom: name, file: file.fileName, license: file.licenseConcluded,
                matchBasis: upstreamChecksumMatches ? 'sbom-checksum' : preSigningChecksumMatches ? 'sbom-pre-signing-pe-checksum' : 'installed-file-sha256',
                upstreamChecksumMatches,
                preSigningChecksumMatches,
                copyright: file.copyrightText,
                packages: owners.map(pkg => ({ name: pkg.name, version: pkg.versionInfo,
                    source: pkg.downloadLocation, license: pkg.licenseDeclared })),
            });
        }
    }
    return { sha256, matches };
}

function walk(directory) {
    return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
        const file = join(directory, entry.name);
        if (entry.isSymbolicLink()) throw new Error(`Refusing to inventory a symbolic link: ${file}`);
        return entry.isDirectory() ? walk(file) : [file];
    });
}

export function collectEvidence(qtRoot, bundle, output) {
    const sbomDirectory = join(qtRoot, 'sbom');
    const documents = readdirSync(sbomDirectory).filter(name => name.endsWith('.spdx.json')).sort()
        .map(name => ({ name, document: JSON.parse(readFileSync(join(sbomDirectory, name), 'utf8')) }));
    if (!documents.length) throw new Error('No Qt JSON SBOMs found');
    const bundleFiles = walk(bundle).filter(file => /\.(dll|exe)$/i.test(file)).sort();
    const bundledNames = new Set(bundleFiles.map(file => basename(file).toLowerCase()));
    const installedHashes = new Map();
    for (const { document } of documents) {
        for (const file of document.files ?? []) {
            if (!bundledNames.has(basename(file.fileName).toLowerCase()) || installedHashes.has(file.fileName)) continue;
            const installed = resolve(qtRoot, file.fileName);
            const local = relative(resolve(qtRoot), installed);
            if (local === '..' || local.startsWith(`..${sep}`) || isAbsolute(local)) {
                throw new Error(`SBOM file escapes Qt root: ${file.fileName}`);
            }
            if (existsSync(installed)) installedHashes.set(file.fileName, createHash('sha256').update(readFileSync(installed)).digest('hex'));
        }
    }
    const binaries = bundleFiles.map(file => ({
        path: relative(bundle, file).replaceAll('\\', '/'), ...matchBinary(readFileSync(file), documents, installedHashes),
    }));
    if (!binaries.length) throw new Error('No Windows binaries found in the bundle');
    const matchedDocuments = new Set(binaries.flatMap(binary => binary.matches.map(match => match.sbom)));
    const unresolvedPackages = documents.filter(({ name }) => matchedDocuments.has(name))
        .flatMap(({ name, document }) => (document.packages ?? []).filter(pkg =>
            !pkg.licenseDeclared || pkg.licenseDeclared === 'NOASSERTION' ||
            !pkg.downloadLocation || pkg.downloadLocation === 'NOASSERTION').map(pkg => ({
            sbom: name, name: pkg.name, license: pkg.licenseDeclared, source: pkg.downloadLocation,
        })));
    const report = {
        status: 'evidence-only-not-approved',
        limitations: [
            'This inventories DLL/EXE files only; data, scripts, fonts, and embedded components require separate review.',
            'Checksum matches establish file identity, not completeness of notices or satisfaction of license obligations.',
            'Pre-signing PE matches remove the trailing Authenticode certificate and zero its directory and PE checksum; they reproduce the upstream image hash, not a signature-trust verdict.',
            'Installed-file matches without a pre-signing match still need signing/postprocessing provenance review.',
            'Unresolved SBOM packages include build-only entries; investigate their reachability before treating them as shipped.',
            'Unmatched binaries may include first-party executables, signing changes, or third-party runtimes missing from the SBOM.',
        ],
        binaries, unresolvedPackages,
    };
    mkdirSync(output, { recursive: true });
    mkdirSync(join(output, 'sbom'), { recursive: true });
    for (const { name } of documents.filter(({ name }) => matchedDocuments.has(name))) {
        copyFileSync(join(sbomDirectory, name), join(output, 'sbom', name));
    }
    writeFileSync(join(output, 'evidence.json'), JSON.stringify(report, null, 2) + '\n');
    const unmatched = binaries.filter(binary => binary.matches.length === 0);
    const summary = [
        '# Windows Qt License Evidence', '',
        'Evidence only, not a distribution approval. Generated from the current bundle and Qt SBOMs.', '',
        `Native files inspected: ${binaries.length}. Hash-matched files: ${binaries.length - unmatched.length}.`, '',
        '## Files Without SBOM Matches', '',
        ...unmatched.map(binary => `- ${binary.path} (SHA-256: ${binary.sha256})`), '',
        '## Unresolved Package Metadata', '',
        ...unresolvedPackages.map(pkg => `- ${pkg.sbom}: ${pkg.name}; license: ${pkg.license ?? 'missing'}; source: ${pkg.source ?? 'missing'}`), '',
        'See evidence.json for file-level licenses, copyright notices, and package sources. Copied SBOMs retain their upstream metadata.', '',
        'No review.json is generated. Do not rename this evidence to bypass the distribution gate.', '',
    ].join('\n');
    writeFileSync(join(output, 'README.md'), summary);
    return report;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    try {
        const { values } = parseArgs({ options: {
            qt: { type: 'string' }, bundle: { type: 'string' }, output: { type: 'string' },
        } });
        if (!values.qt || !values.bundle || !values.output) {
            throw new Error(`Usage: node ${basename(fileURLToPath(import.meta.url))} --qt QT_ROOT --bundle BUNDLE --output AUDIT_DIRECTORY`);
        }
        const report = collectEvidence(resolve(values.qt), resolve(values.bundle), resolve(values.output));
        console.log(`Collected ${report.binaries.length} native files; ${report.binaries.filter(binary => !binary.matches.length).length} lack SBOM matches. Evidence is not approval.`);
    } catch (error) {
        console.error(error.message);
        process.exitCode = 1;
    }
}
