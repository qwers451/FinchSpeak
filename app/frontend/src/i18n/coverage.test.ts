import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { expect, it } from 'vitest';
import { ru, ruPlurals } from './index';

/**
 * Guards the Russian translation. It reads the sources as text, because most gaps hide in places a render test never
 * reaches: a phrase passed to t() through a variable, a template string in an aria-label, a label chosen by a condition.
 * How to add a string and what to do when this test fails: docs/I18N.md.
 */

const SRC = join(process.cwd(), 'src');

function walk(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return walk(path);
    return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [path] : [];
  });
}

const FILES = walk(SRC)
  .filter((path) => !path.endsWith(join('i18n', 'index.ts')) && !path.endsWith('testSetup.ts') && !path.endsWith('vite-env.d.ts'))
  .map((path) => ({ name: relative(SRC, path).replace(/\\/g, '/'), text: readFileSync(path, 'utf8') }));

/** Comments removed, line structure kept, so positions still map to lines. */
const withoutComments = (text: string) => text.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' ')).replace(/(^|\s)\/\/.*$/gm, '$1');
const lineOf = (text: string, index: number) => text.slice(0, index).split('\n').length;

// ---- 1. every t('…') key has a Russian text

it('every literal t() key has a Russian translation', () => {
  const missing: string[] = [];
  for (const { name, text } of FILES) {
    for (const match of withoutComments(text).matchAll(/\bt\(\s*(['"])((?:(?!\1)[^\\\n]|\\.)*)\1/g)) {
      const key = match[2].replace(/\\(['"])/g, '$1');
      if (!(key in ru)) missing.push(`${name}:${lineOf(text, match.index!)} ${JSON.stringify(key)}`);
    }
  }
  expect(missing).toEqual([]);
});

// ---- 2. the dictionary itself

it('Russian texts are not empty and keep the {placeholders} of their English key', () => {
  const placeholders = (text: string) => [...text.matchAll(/\{([^{}]+)\}/g)].map((m) => m[1]).sort().join(',');
  const broken = Object.entries(ru).filter(([key, value]) => !value.trim() || placeholders(key) !== placeholders(value)).map(([key]) => key);
  expect(broken).toEqual([]);
});

it('a Russian text equals its English key only for names that are not translated', () => {
  const same = Object.entries(ru).filter(([key, value]) => key === value).map(([key]) => key);
  // Theme names are proper names; '{action}: {name}?' reads the same in both languages.
  expect(same.sort()).toEqual(['Claude Dark', 'Finch Custom', '{action}: {name}?']);
});

// ---- 3. no string that reads like interface text is left outside the dictionary

/** Strings that look like text but are not shown to people. Each needs a reason; a new entry is a deliberate decision. */
const NOT_INTERFACE_TEXT: [RegExp, string][] = [
  [/^(Enter|Escape|Tab|Backspace|Delete|Home|End|Arrow(Up|Down|Left|Right)|Control|Alt|Super|Shift|(Key|Digit)([A-Z]|\d)?)$/, 'KeyboardEvent key and code names'],
  [/^(Ctrl|Esc|Shift \+ Enter)$/, 'key caps: the Latin labels printed on keyboards, also in Russian'],
  [/^(POST|Content-Type|INPUT|TEXTAREA|SELECT|Files|FinchSpeak)$/, 'HTTP, DOM and product names'],
  [/^(Left|Disconnected|RoomClosed)$/, 'reason codes sent by the server (stores and viewerState map them to translated texts)'],
  [/^(Fingerprint|Diagnostics)$/, 'internal tags that pick a translated toast in SettingsModal'],
  [/^(Server|Certificate|Agent): |^FinchSpeak \$\{/, 'diagnostics text that people copy for support: English on purpose'],
  [/^(Carol Danvers|K7M2QX9TPW4B)$/, 'example values inside input placeholders'],
  [/^2-digit$/, 'option value of Intl.DateTimeFormat'],
  [/^(H\.264 is not supported|WebCodecs VideoDecoder error:|Failed to (decode|initialize) .*|FinchSpeak viewer.*)/, 'console and diagnostic messages for developers'],
  [/^Failed to /, 'console messages for developers'],
  [/^Root element not found/, 'developer error in index.tsx'],
];
/** Files whose messages only ever appear outside the desktop app (the browser stand) or in logs. */
const NOT_INTERFACE_FILES = ['services/api.ts', 'services/sessionEvents.ts', 'components/chat/markdown.ts' /* a parser: regular expressions, no interface text */];

/** The English forms handed to plural(): { one, other } pairs found in the sources. */
function pluralCalls() {
  return FILES.flatMap(({ name, text }) => [...withoutComments(text).matchAll(/\bplural\([^{}]*\{\s*one:\s*(['"])(.*?)\1,\s*other:\s*(['"])(.*?)\3/g)]
    .map((match) => ({ where: `${name}:${lineOf(text, match.index!)}`, one: match[2], other: match[4] })));
}

function candidates(name: string, text: string) {
  const out: { where: string; value: string }[] = [];
  const pluralForms = new Set(pluralCalls().flatMap(({ one, other }) => [one, other]));
  const code = withoutComments(text);
  for (const match of code.matchAll(/(['"])((?:(?!\1)[^\\\n]|\\.)*)\1|`((?:[^`\\]|\\.)*)`/g)) {
    const isTemplate = match[3] !== undefined;
    const value = (isTemplate ? match[3] : match[2]).replace(/\\(['"])/g, '$1');
    const plain = isTemplate ? value.replace(/\$\{[^}]*\}/g, '\u0000') : value;
    if (!/[A-Za-z]{3}/.test(plain.replace(/\u0000/g, ''))) continue;
    const reads = isTemplate ? /[A-Z][a-z]{2,}|\b[A-Z]{2,3}\b|\b(min|kbps|fps)\b/.test(plain) : /^[A-Z{0-9]/.test(plain);
    if (!reads || value in ru || pluralForms.has(value)) continue;
    out.push({ where: `${name}:${lineOf(text, match.index!)}`, value });
  }
  return out;
}

it('no text that looks like interface copy is missing from the Russian dictionary', () => {
  const unknown = FILES.filter(({ name }) => !NOT_INTERFACE_FILES.includes(name))
    .flatMap(({ name, text }) => candidates(name, text))
    .filter(({ value }) => !NOT_INTERFACE_TEXT.some(([pattern]) => pattern.test(value)))
    .map(({ where, value }) => `${where} ${JSON.stringify(value)}`);
  expect(unknown).toEqual([]);
});

it('every entry of the allowlist still matches something in the sources (no stale reasons)', () => {
  const all = FILES.flatMap(({ name, text }) => (NOT_INTERFACE_FILES.includes(name) ? [] : candidates(name, text)));
  const unused = NOT_INTERFACE_TEXT.filter(([pattern]) => !all.some(({ value }) => pattern.test(value))).map(([pattern]) => String(pattern));
  expect(unused).toEqual([]);
});

it('every plural() call has Russian one / few / many forms', () => {
  const calls = pluralCalls();
  expect(calls.length).toBeGreaterThan(0);
  const incomplete = calls.filter(({ other }) => { const table = ruPlurals[other]; return !table || !table.one || !table.few || !table.many; }).map(({ where, other }) => `${where} ${JSON.stringify(other)}`);
  expect(incomplete).toEqual([]);
});

// ---- 4. accessible names and visible attributes go through t()

it('aria-label, label, title, placeholder and alt are never plain English or a template string', () => {
  const EXAMPLES = /^(https?:\/\/|[A-Z0-9]{8,}$|alice$|carol$|Carol Danvers$)/; // sample input values in AuthModal
  const found: string[] = [];
  for (const { name, text } of FILES) {
    for (const match of withoutComments(text).matchAll(/\b(aria-label|label|title|placeholder|alt)=\{?\s*(['"`])((?:(?!\2)[^\\]|\\.)*)\2/g)) {
      const value = match[3];
      // A template that opens with ${…} only assembles parts that are translated on their own (ParticipantTile).
      if (match[2] === '`' ? !/^[A-Za-z]/.test(value) : !/[A-Za-z]{3}/.test(value)) continue;
      if (match[1] === 'placeholder' && EXAMPLES.test(value)) continue;
      found.push(`${name}:${lineOf(text, match.index!)} ${match[1]}=${JSON.stringify(value)}`);
    }
  }
  expect(found).toEqual([]);
});

it('no visible text sits in JSX outside t()', () => {
  const found: string[] = [];
  for (const { name, text } of FILES.filter((file) => file.name.endsWith('.tsx'))) {
    for (const match of withoutComments(text).matchAll(/>\s*([A-Z][a-z]+(?: [A-Za-z]+)*)\s*</g)) found.push(`${name}:${lineOf(text, match.index!)} ${JSON.stringify(match[1])}`);
  }
  expect(found).toEqual([]);
});
