// Writes the update manifest the app reads: node make-latest-json.mjs <installer> <tag> <owner/repo> <out>
// The installer's signature must sit next to it as <installer>.sig (made by `tauri signer sign`).
import { readFileSync, writeFileSync } from 'node:fs';
import { basename } from 'node:path';

const [installer, tag, repository, out] = process.argv.slice(2);
if (!installer || !tag || !repository || !out) {
  console.error('usage: node make-latest-json.mjs <installer> <tag> <owner/repo> <out>');
  process.exit(2);
}

const signature = readFileSync(`${installer}.sig`, 'utf8').trim();
const manifest = {
  version: tag.replace(/^v/, ''),
  notes: `FinchSpeak ${tag}`,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': {
      signature,
      url: `https://github.com/${repository}/releases/download/${tag}/${basename(installer)}`,
    },
  },
};
writeFileSync(out, JSON.stringify(manifest, null, 2));
console.log(`wrote ${out}`);
