import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { main, renderLicenses, validateSupplement } from '../generate_licenses.mjs';
import { matchBinary, collectEvidence, unsignedPeImage } from '../collect_qt_license_evidence.mjs';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync } from 'node:fs';

test('pre-signing PE reconstruction explains signature-only SBOM differences without changing input', () => {
    const unsigned = Buffer.alloc(512);
    unsigned.write('MZ'); unsigned.writeUInt32LE(64, 60);
    unsigned.writeUInt32LE(0x4550, 64); unsigned.writeUInt16LE(240, 84);
    unsigned.writeUInt16LE(0x20b, 88);
    const signed = Buffer.concat([unsigned, Buffer.alloc(16, 42)]);
    signed.writeUInt32LE(12345, 88 + 64);
    signed.writeUInt32LE(512, 88 + 112 + 32);
    signed.writeUInt32LE(16, 88 + 112 + 36);
    assert.deepEqual(unsignedPeImage(signed), unsigned);
    assert.equal(signed.readUInt32LE(88 + 64), 12345);
    const checksum = createHash('sha1').update(unsigned).digest('hex');
    const match = matchBinary(signed, [{ name: 'test', document: { files: [{ fileName: 'test.dll', checksums: [{ algorithm: 'SHA1', checksumValue: checksum }] }] } }]);
    assert.equal(match.matches[0].matchBasis, 'sbom-pre-signing-pe-checksum');
    assert.equal(match.matches[0].upstreamChecksumMatches, false);
    signed[400] = 1;
    assert.equal(matchBinary(signed, [{ name: 'test', document: { files: [{ checksums: [{ algorithm: 'SHA1', checksumValue: checksum }] }] } }]).matches.length, 0);
    assert.equal(unsignedPeImage(Buffer.from('not PE')), null);
});

test('Qt evidence matches actual checksums, not filenames', () => {
    const bytes = Buffer.from('test binary');
    const documents = [{ name: 'qt-test.spdx.json', document: {
        packages: [{ SPDXID: 'pkg', name: 'QtTest', downloadLocation: 'test-source', licenseDeclared: 'LGPL-3.0-only' }],
        files: [{ SPDXID: 'file', fileName: './bin/renamed.dll', licenseConcluded: 'LGPL-3.0-only',
            checksums: [{ algorithm: 'SHA1', checksumValue: createHash('sha1').update(bytes).digest('hex') }] }],
        relationships: [{ spdxElementId: 'pkg', relatedSpdxElement: 'file', relationshipType: 'CONTAINS' }],
    } }];
    const match = matchBinary(bytes, documents);
    assert.equal(match.matches[0].packages[0].name, 'QtTest');
    assert.equal(matchBinary(Buffer.from('changed binary'), documents).matches.length, 0);
    const installedBytes = Buffer.from('signed installed binary');
    const installedHashes = new Map([['./bin/renamed.dll', createHash('sha256').update(installedBytes).digest('hex')]]);
    const installedMatch = matchBinary(installedBytes, documents, installedHashes).matches[0];
    assert.equal(installedMatch.matchBasis, 'installed-file-sha256');
    assert.equal(installedMatch.upstreamChecksumMatches, false);
});

test('Qt evidence does not produce a distribution approval', () => {
    const directory = mkdtempSync(join(tmpdir(), 'doubleslash-qt-evidence-'));
    try {
        mkdirSync(join(directory, 'qt', 'sbom'), { recursive: true });
        mkdirSync(join(directory, 'bundle'));
        writeFileSync(join(directory, 'qt', 'sbom', 'qt.spdx.json'), JSON.stringify({ packages: [], files: [] }));
        writeFileSync(join(directory, 'bundle', 'unknown.dll'), 'test');
        const report = collectEvidence(join(directory, 'qt'), join(directory, 'bundle'), join(directory, 'output'));
        assert.equal(report.status, 'evidence-only-not-approved');
        assert.equal(report.binaries[0].matches.length, 0);
        assert.equal(existsSync(join(directory, 'output', 'review.json')), false);
    } finally {
        rmSync(directory, { recursive: true, force: true });
    }
});

test('renders crate versions and escapes license text', () => {
    const html = renderLicenses({ licenses: [{ name: 'MIT', text: '<copyright & notice>', used_by: [{ crate: { name: 'example', version: '1.2.3' } }] }] });
    assert.match(html, /example 1\.2\.3/);
    assert.match(html, /&lt;copyright &amp; notice&gt;/);
});

test('rejects empty or incomplete license output', () => {
    assert.throws(() => renderLicenses({ licenses: [] }), /No resolved/);
    assert.throws(() => renderLicenses({ licenses: [{ id: 'MIT', text: '' }] }), /Incomplete/);
});

test('desktop distribution requires supplemental evidence', () => {
    assert.throws(() => main(['--product', 'client', '--target', 'x86_64-pc-windows-msvc', '--output', 'unused']), /requires a reviewed/);
});

test('supplement is target-bound and rejects missing files or directory traversal', () => {
    const directory = mkdtempSync(join(tmpdir(), 'doubleslash-license-test-'));
    try {
        assert.throws(() => validateSupplement(directory, 'android', 'aarch64-linux-android', ''), /No review.json under/);
        const review = {
            product: 'client', target: 'test-target', features: 'qt-ui',
            components: [{ name: 'Qt', version: 'test', source: 'test-source', obligations: 'test-review', files: ['notice.txt'] }],
        };
        const save = () => writeFileSync(join(directory, 'review.json'), JSON.stringify(review));
        save();
        assert.throws(() => validateSupplement(directory, 'client', 'other-target', 'qt-ui'), /Supplement must/);
        // A build whose feature set changes what ships must not reuse a
        // supplement written for the other one.
        assert.throws(() => validateSupplement(directory, 'client', 'test-target', 'qt-ui,webengine'), /Supplement must/);
        // The notice text a component names has to exist.
        assert.throws(() => validateSupplement(directory, 'client', 'test-target', 'qt-ui'), /ENOENT/);
        writeFileSync(join(directory, 'notice.txt'), 'license text');
        assert.equal(validateSupplement(directory, 'client', 'test-target', 'qt-ui').components.length, 1);
        // An empty notice ships nothing readable, so it is not a notice.
        writeFileSync(join(directory, 'notice.txt'), '   ');
        assert.throws(() => validateSupplement(directory, 'client', 'test-target', 'qt-ui'), /Empty supplement/);
        writeFileSync(join(directory, 'notice.txt'), 'license text');
        // A component missing its obligations text is not reviewable.
        const obligations = review.components[0].obligations;
        review.components[0].obligations = '';
        save();
        assert.throws(() => validateSupplement(directory, 'client', 'test-target', 'qt-ui'), /requires version, source, obligations/);
        review.components[0].obligations = obligations;
        review.components[0].files = ['../outside.txt'];
        save();
        assert.throws(() => validateSupplement(directory, 'client', 'test-target', 'qt-ui'), /escapes/);
    } finally {
        rmSync(directory, { recursive: true, force: true });
    }
});

test('notices include project copyright and exact crate source downloads', () => {
    const html = renderLicenses({ licenses: [{ name: 'MPL-2.0', text: 'license text', used_by: [{ crate: {
        name: 'example', version: '1.2.3', source: 'registry+https://github.com/rust-lang/crates.io-index',
    } }] }] }, 'Copyright project authors');
    assert.match(html, /Copyright project authors/);
    assert.match(html, /https:\/\/crates.io\/api\/v1\/crates\/example\/1.2.3\/download/);
});
