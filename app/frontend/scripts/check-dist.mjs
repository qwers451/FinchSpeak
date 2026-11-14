// Postbuild guard: the dev-only visual-audit entry (src/qa) must not be in the production bundle.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

const root = new URL('../dist', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const needles = ['__qaReady', 'mountQa', 'Konstantinopolskaya'];
const hits = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else if (path.endsWith('.js')) {
      const text = readFileSync(path, 'utf8');
      for (const needle of needles) if (text.includes(needle)) hits.push(`${path}: ${needle}`);
    }
  }
};
walk(decodeURIComponent(root));
if (hits.length) {
  console.error(`QA-only code found in the production bundle:\n${hits.join('\n')}`);
  process.exit(1);
}
console.log('check-dist: no QA-only code in dist');
