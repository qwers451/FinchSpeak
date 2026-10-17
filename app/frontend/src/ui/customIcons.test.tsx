import { render } from 'solid-js/web';
import { afterEach, expect, it } from 'vitest';
import { Icon } from './Icon';
import { ICONS } from './iconData';
import sprite from '../../public/custom/assets/ui-icons/sprite.svg?raw';
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.innerHTML = ''; });
it('gives every application icon a kit glyph without replacing its accessible identity', () => {
 const host = document.createElement('div'); document.body.append(host);
 dispose = render(() => <>{Object.keys(ICONS).map(name => <Icon name={name as keyof typeof ICONS} title={name} />)}</>,host);
 const glyphs = host.querySelectorAll('use.custom-glyph');
 expect(glyphs.length).toBe(Object.keys(ICONS).length);
 expect(Array.from(glyphs).every(g => g.getAttribute('href')?.includes('#fi-'))).toBe(true);
 for (const glyph of glyphs) {
   const symbol = glyph.getAttribute('href')!.split('#')[1];
   expect(sprite, symbol).toContain(`id="${symbol}"`);
 }
 expect(host.querySelector('[aria-label="mic"]')).not.toBeNull();
});
