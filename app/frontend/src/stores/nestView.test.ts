import { expect, it } from 'vitest';
import { branchPlan, setNestView, nestLevel } from './nestView';

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

it('spreads nests and birdhouses over as many branches as either needs', () => {
  expect(branchPlan([], [], 4)).toEqual([{ nests: [], houses: [] }]);
  expect(branchPlan<number | string>([1, 2, 3, 4, 5], ['a'], 4)).toEqual([{ nests: [1, 2, 3, 4], houses: ['a'] }, { nests: [5], houses: [] }]);
  expect(branchPlan<number | string>([1], ['a', 'b', 'c'], 2)).toEqual([{ nests: [1], houses: ['a', 'b'] }, { nests: [], houses: ['c'] }]);
  expect(branchPlan([1, 2], [], 0)).toEqual([{ nests: [1], houses: [] }, { nests: [2], houses: [] }]);
});
