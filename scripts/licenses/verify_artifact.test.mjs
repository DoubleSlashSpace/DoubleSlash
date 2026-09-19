import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { sha256, reviewInputs, archiveFiles } from './review_inputs.mjs';
import { verifyArtifact, verifyExtracted } from './verify_artifact.mjs';

test('final archive validation rejects lost notices and audit-only packages', () => {
    const root = mkdtempSync(join(tmpdir(), 'license-artifact-test-'));
    try {
        const stage = join(root, 'package');
        const directory = join(stage, 'licenses');
        mkdirSync(directory, { recursive: true });
        const inventory = {
            product: 'supernode', scope: 'distribution-notices', inputsSha256: sha256('[]'),
            sourceSnapshot: { file: 'source.tar.gz', sha256: sha256('source bytes') },
        };
        const save = () => writeFileSync(join(directory, 'inventory.json'), JSON.stringify(inventory));
        save();
        assert.throws(() => verifyExtracted(root, ['supernode']), /ENOENT/);
        writeFileSync(join(directory, 'rust-licenses.html'), '<html>DoubleSlash licenses</html>');
        writeFileSync(join(directory, 'DoubleSlash-LICENSE.txt'), 'MIT');
        writeFileSync(join(directory, 'review-inputs.json'), '{"inputs":[]}');
        writeFileSync(join(directory, 'source.tar.gz'), 'source bytes');
        const archive = join(root, 'final.tar.gz');
        assert.equal(spawnSync('tar', ['-czf', archive, '-C', root, 'package']).status, 0);
        assert.equal(verifyArtifact(archive, ['supernode']).inventories, 1);
        assert.throws(() => verifyArtifact(stage, ['supernode']), /not a staging directory/);
        inventory.scope = 'rust-audit-only'; save();
        assert.throws(() => verifyExtracted(root, ['supernode']), /Audit-only/);
        assert.equal(verifyExtracted(root, ['supernode'], { allowAuditOnly: true }).inventories, 1);
        inventory.product = 'android'; save();
        assert.equal(verifyExtracted(root, ['android'], { allowAuditOnly: true }).inventories, 1);
        inventory.product = 'supernode';
        inventory.scope = 'distribution-notices'; save();
        writeFileSync(join(directory, 'source.tar.gz'), 'changed');
        assert.throws(() => verifyExtracted(root, ['supernode']), /changed source snapshot/);
    } finally { rmSync(root, { recursive: true, force: true }); }
});

test('review inputs change with build scripts and vendored source despite unchanged lockfile', () => {
    const root = mkdtempSync(join(tmpdir(), 'license-input-test-'));
    try {
        assert.equal(spawnSync('git', ['init', '--quiet', root]).status, 0);
        mkdirSync(join(root, 'rust/doubleslash-opus/opus'), { recursive: true });
        writeFileSync(join(root, 'rust/Cargo.lock'), 'fixed lockfile');
        const build = join(root, 'rust/doubleslash-opus/build.rs');
        writeFileSync(build, 'original build script');
        const before = reviewInputs(root).inputsSha256;
        writeFileSync(build, 'modified build script');
        assert.notEqual(reviewInputs(root).inputsSha256, before);
        const beforeNative = reviewInputs(root).inputsSha256;
        writeFileSync(join(root, 'rust/doubleslash-opus/opus/codec.c'), 'modified native source');
        assert.notEqual(reviewInputs(root).inputsSha256, beforeNative);
        assert.ok(archiveFiles('client', root).includes('rust/doubleslash-opus/opus/codec.c'));
    } finally { rmSync(root, { recursive: true, force: true }); }
});
