import { render } from 'solid-js/web';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined), Channel: class { onmessage: unknown = null; } }));

import { api } from '../../services/api';
import { setAdminOpen } from '../../stores/adminStore';
import { setCurrentUser } from '../../stores/authStore';
import { DEFAULT_PREFS, setPrefs } from '../../stores/settingsStore';
import type { HealthSample, ServerHealth } from '../../types';
import { AdminModal } from './AdminModal';
import { HealthTab, sparkPoints, warningText } from './HealthTab';

const MB = 1024 * 1024;
const GB = 1024 * MB;

const sample = (over: Partial<HealthSample> = {}): HealthSample => ({
  at: 1, cpu_percent: 12.5, rss_bytes: 200 * MB, system_used_bytes: 4 * GB, system_total_bytes: 16 * GB, container_limit_bytes: null,
  disk_free_bytes: 50 * GB, sessions: 3, voice_users: 2, bytes_in_per_sec: 2048, bytes_out_per_sec: 4096, datagrams_dropped: 7, ...over,
});

const report = (over: Partial<ServerHealth> = {}): ServerHealth => ({
  now: sample(), history: [sample({ cpu_percent: 1 }), sample({ cpu_percent: 5 }), sample({ cpu_percent: 2 })],
  db_bytes: MB, uploads_bytes: 2 * MB, music_bytes: 0, uptime_secs: 7200, warnings: [], ...over,
});

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
let dispose: (() => void) | undefined;

const mount = (view: () => unknown) => {
  const host = document.createElement('div');
  document.body.appendChild(host);
  dispose = render(view as () => HTMLElement, host);
};
const tile = (label: string) => Array.from(document.querySelectorAll<HTMLElement>('.health-tile')).find((el) => el.querySelector('.t3')?.textContent === label);

beforeEach(() => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } });
  vi.restoreAllMocks();
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.innerHTML = '';
  vi.useRealTimers();
});

it('draws the tiles, a sparkline per series and the warnings in words', async () => {
  vi.spyOn(api, 'serverHealth').mockResolvedValue(report({
    now: sample({ container_limit_bytes: 256 * MB, rss_bytes: 230 * MB, disk_free_bytes: GB / 2 }),
    warnings: ['memory-high', 'disk-low'],
  }));
  mount(() => <HealthTab />);
  await flush();

  expect(tile('Process CPU')?.textContent).toContain('12.5 %');
  expect(tile('Container limit')?.textContent).toContain('256 MB');
  expect(tile('Database')?.textContent).toContain('1 MB');
  expect(tile('Lost datagrams')?.textContent).toContain('7');
  expect(tile('Process CPU')?.querySelectorAll('.spark polyline')).toHaveLength(1);
  expect(tile('Process CPU')?.querySelector('polyline')?.getAttribute('points')).toContain(',');
  expect(tile('Lost datagrams')?.querySelector('svg')).toBeNull();
  expect(document.body.textContent).toContain('Memory is close to the limit');
  expect(document.body.textContent).toContain('Little disk space is left under the data folder');
});

it('shows None when the server is not in a container', async () => {
  vi.spyOn(api, 'serverHealth').mockResolvedValue(report());
  mount(() => <HealthTab />);
  await flush();
  expect(tile('Container limit')?.textContent).toContain('None');
  expect(document.body.textContent).toContain('No warnings');
});

it('asks for fresh numbers every 5 seconds only while the tab is mounted', async () => {
  vi.useFakeTimers();
  const serverHealth = vi.spyOn(api, 'serverHealth').mockResolvedValue(report());
  mount(() => <HealthTab />);
  await vi.advanceTimersByTimeAsync(0);
  expect(serverHealth).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(5000);
  expect(serverHealth).toHaveBeenCalledTimes(2);
  dispose?.();
  dispose = undefined;
  await vi.advanceTimersByTimeAsync(20000);
  expect(serverHealth).toHaveBeenCalledTimes(2);
});

it('the administration window opens the health tab for administrators', async () => {
  vi.spyOn(api, 'adminUsers').mockResolvedValue([]);
  vi.spyOn(api, 'listInvites').mockResolvedValue([]);
  vi.spyOn(api, 'auditLog').mockResolvedValue([]);
  const serverHealth = vi.spyOn(api, 'serverHealth').mockResolvedValue(report({ warnings: ['disk-low'] }));
  setCurrentUser({ id: 'boss', username: 'boss', display_name: 'Boss', role: 'admin' });
  setAdminOpen(true);
  mount(() => <AdminModal />);
  await flush();
  Array.from(document.querySelectorAll<HTMLElement>('.setnav .si')).find((b) => b.textContent === 'Health')!.click();
  await flush();
  await flush();
  expect(serverHealth).toHaveBeenCalled();
  expect(document.body.textContent).toContain('Process memory');
  setAdminOpen(false);
});

it('turns warning codes into words and draws sparkline points', () => {
  expect(warningText('memory-high')).toBe('Memory is close to the limit');
  expect(warningText('something-new')).toBe('something-new');
  expect(sparkPoints([])).toBe('');
  expect(sparkPoints([5])).toBe('');
  expect(sparkPoints([0, 10], 100, 20)).toBe('0.0,20.0 100.0,0.0');
});
