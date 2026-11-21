// Export the desktop SVG icons as a folder an icon pack can start from.
//
//   node scripts/export_icon_pack.mjs [outDir]
//
// The default folder is dist/icon-pack. It contains pack.json and one SVG
// per name in rust/doubleslash-client/icons.qrc. Keep those file names.
// Settings › General › Icon pack loads the folder. A name you delete keeps
// the built-in icon.
//
// Button icons are tinted from the shape's alpha, so draw a solid shape on
// a transparent background. Images that are not buttons show the file's own
// colours. A PNG named like the icon (phone.png) replaces that SVG. If both
// are in the folder, the SVG is used.

import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const qrcPath = resolve(root, 'rust/doubleslash-client/icons.qrc');
const outDir = resolve(root, process.argv[2] || 'dist/icon-pack');

const qrc = readFileSync(qrcPath, 'utf8');
const names = [];
for (const line of qrc.split(/\r?\n/)) {
  const match = line.match(/<file alias="icons\/([^"]+\.svg)">([^<]+)<\/file>/);
  if (!match)
    continue;
  names.push({ name: match[1], source: resolve(root, 'rust/doubleslash-client', match[2]) });
}
if (names.length === 0) {
  console.error('icons.qrc listed no icons');
  process.exit(1);
}
names.sort((a, b) => a.name.localeCompare(b.name));

mkdirSync(outDir, { recursive: true });
for (const icon of names) {
  copyFileSync(icon.source, resolve(outDir, icon.name));
}

const pack = {
  v: 1,
  name: 'DoubleSlash',
  icons: names.map(icon => icon.name),
  note: 'Replace any SVG and keep its file name, or add a PNG with the same name (phone.png replaces phone.svg). If both are in this folder, the SVG is used. Settings › General › Icon pack loads this folder. A missing file keeps the built-in icon. Button icons are tinted from the shape\'s alpha, so use a solid shape on a transparent background. Other images show the file\'s own colours.',
};
writeFileSync(resolve(outDir, 'pack.json'), `${JSON.stringify(pack, null, 2)}\n`);
console.log(`Exported ${names.length} icons to ${outDir}`);
