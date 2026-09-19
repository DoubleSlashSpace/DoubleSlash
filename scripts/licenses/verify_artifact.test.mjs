import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { verifyArtifact, verifyExtracted } from './verify_artifact.mjs';

test('final archive validation rejects lost notices', () => {
    const root = mkdtempSync(join(tmpdir(), 'license-artifact-test-'));
    try {
        const stage = join(root, 'package');
        const directory = join(stage, 'licenses');
        mkdirSync(join(directory, 'supplement'), { recursive: true });
        const inventory = { product: 'supernode', supplement: ['qt-LGPL-3.0.txt'] };
        const save = () => writeFileSync(join(directory, 'inventory.json'), JSON.stringify(inventory));
        save();
        assert.throws(() => verifyExtracted(root, ['supernode']), /ENOENT/);
        writeFileSync(join(directory, 'rust-licenses.html'), '<html>DoubleSlash licenses</html>');
        writeFileSync(join(directory, 'DoubleSlash-LICENSE.txt'), 'MIT');
        // A supplement notice the inventory promises but the package lost is
        // the failure that leaves a recipient without a licence text.
        assert.throws(() => verifyExtracted(root, ['supernode']), /ENOENT/);
        writeFileSync(join(directory, 'supplement', 'qt-LGPL-3.0.txt'), 'LGPL-3.0 text');
        const archive = join(root, 'final.tar.gz');
        assert.equal(spawnSync('tar', ['-czf', archive, '-C', root, 'package']).status, 0);
        assert.equal(verifyArtifact(archive, ['supernode']).inventories, 1);
        assert.throws(() => verifyArtifact(stage, ['supernode']), /not a staging directory/);
        assert.throws(() => verifyExtracted(root, ['client']), /missing client notices/);
        writeFileSync(join(directory, 'supplement', 'qt-LGPL-3.0.txt'), '');
        assert.throws(() => verifyExtracted(root, ['supernode']), /Empty notice/);
    } finally { rmSync(root, { recursive: true, force: true }); }
});
