import { expect, it } from 'vitest';
import { setNestView, nestLevel } from './nestView';

it.each([
  [0, false, 0], [1, false, 1], [2, true, 2], [4, false, 2], [5, false, 3], [5, true, 3],
] as const)('nestLevel(%i, %s) is %i', (count, speaking, level) => {
  expect(nestLevel(count, speaking)).toBe(level);
});

it('remembers the choice', () => {
  setNestView(true);
  expect(localStorage.getItem('finchspeak.nestView')).toBe('1');
  setNestView(false);
  expect(localStorage.getItem('finchspeak.nestView')).toBe('0');
});
