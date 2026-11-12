// Regenerates every raster icon from the brand SVGs in design/.
//   node scripts/make-icons.mjs
// design/finchspeak-tile.svg       — the app tile with sound waves (48 px and larger)
// design/finchspeak-tile-small.svg — the same tile without waves (16–32 px, the tray)
// Rasterising is done by `tauri icon` (already a dev dependency); this script only picks sizes
// and packs the Windows .ico, whose small and large entries come from different pictures.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const frontend = path.join(root, 'app', 'frontend');
const icons = path.join(root, 'app', 'icons');
const SMALL = [16, 24, 32];
const LARGE = [48, 64, 128, 256];

function render(svg, sizes) {
  const out = mkdtempSync(path.join(tmpdir(), 'finch-icons-'));
  execFileSync('npx', ['tauri', 'icon', path.join(root, 'design', svg), '-o', out, '-p', sizes.join(',')],
    { cwd: frontend, stdio: 'ignore', shell: true });
  return out;
}

/** A .ico whose entries are PNG images (supported since Windows Vista). */
function ico(entries) {
  const header = Buffer.alloc(6 + 16 * entries.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  let offset = header.length;
  entries.forEach(({ size, png }, i) => {
    const at = 6 + 16 * i;
    header.writeUInt8(size >= 256 ? 0 : size, at);
    header.writeUInt8(size >= 256 ? 0 : size, at + 1);
    header.writeUInt16LE(1, at + 4);
    header.writeUInt16LE(32, at + 6);
    header.writeUInt32LE(png.length, at + 8);
    header.writeUInt32LE(offset, at + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...entries.map((entry) => entry.png)]);
}

const small = render('finchspeak-tile-small.svg', [...SMALL, 64]);
const large = render('finchspeak-tile.svg', [...LARGE, 150, 44, 50, 512]);
try {
  const png = (dir, size) => readFileSync(path.join(dir, `${size}x${size}.png`));
  writeFileSync(path.join(icons, 'icon.ico'), ico([
    ...SMALL.map((size) => ({ size, png: png(small, size) })),
    ...LARGE.map((size) => ({ size, png: png(large, size) })),
  ]));
  copyFileSync(path.join(small, '32x32.png'), path.join(icons, '32x32.png'));
  copyFileSync(path.join(large, '128x128.png'), path.join(icons, '128x128.png'));
  copyFileSync(path.join(large, '256x256.png'), path.join(icons, '128x128@2x.png'));
  copyFileSync(path.join(large, '512x512.png'), path.join(icons, 'icon.png'));
  copyFileSync(path.join(large, '150x150.png'), path.join(icons, 'Square150x150Logo.png'));
  copyFileSync(path.join(large, '44x44.png'), path.join(icons, 'Square44x44Logo.png'));
  copyFileSync(path.join(large, '50x50.png'), path.join(icons, 'StoreLogo.png'));
  // The tray: one picture for every state; the state is a coloured dot drawn by app/src/tray.rs.
  copyFileSync(path.join(small, '64x64.png'), path.join(icons, 'tray.png'));
  console.log('icons written to app/icons');
} finally {
  rmSync(small, { recursive: true, force: true });
  rmSync(large, { recursive: true, force: true });
}
