import { beforeEach, expect, it, vi } from 'vitest';
const a = '9c12a901-103d-43e1-95b3-8fe4e9962ec1';
const b = '9c12a901-103d-43e1-95b3-8fe4e9962ec2';
const c = '9c12a901-103d-43e1-95b3-8fe4e9962ec3';
beforeEach(() => { vi.resetModules(); localStorage.clear(); });
it('repairs invalid entries and duplicate IDs in saved tile order', async () => {
  localStorage.setItem('finchspeak.tileOrder', JSON.stringify([b, 'bad', 1, b, a]));
  const { tileOrder } = await import('./tileOrderStore');
  expect(tileOrder()).toEqual([b, a]);
});
it('inserts before or after a tile, persists order and keeps unknown participants in join order', async () => {
  const { moveTile, tileOrder } = await import('./tileOrderStore');
  moveTile(c, a, false, [a, b, c]);
  expect(tileOrder()).toEqual([c, a, b]);
  moveTile(c, b, true, [a, b, c]);
  expect(tileOrder()).toEqual([a, b, c]);
  expect(JSON.parse(localStorage.getItem('finchspeak.tileOrder') ?? 'null')).toEqual([a, b, c]);
  vi.resetModules();
  expect((await import('./tileOrderStore')).tileOrder()).toEqual([a, b, c]);
});
