import { createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { api } from '../../services/api';
import type { HealthSample, ServerHealth } from '../../types';
import { formatBytes, formatDuration, t } from '../../i18n';

/** How often the panel asks the server for a fresh sample, while it is open. */
export const HEALTH_REFRESH_MS = 5000;

/** Points of a sparkline in a 0..width by 0..height box; the largest value touches the top. */
export function sparkPoints(values: number[], width = 120, height = 32): string {
  if (values.length < 2) return '';
  const top = Math.max(...values, 0) || 1;
  const step = width / (values.length - 1);
  return values
    .map((value, index) => `${(index * step).toFixed(1)},${(height - (Math.max(0, value) / top) * height).toFixed(1)}`)
    .join(' ');
}

/** The words for a warning code from the server; unknown codes are shown as they are. */
export function warningText(code: string): string {
  if (code === 'memory-high') return t('Memory is close to the limit');
  if (code === 'disk-low') return t('Little disk space is left under the data folder');
  return code;
}

const percent = (value: number) => `${value.toFixed(1)} %`;
const perSecond = (bytes: number) => `${formatBytes(bytes, 'KB')}/s`;
const limitText = (limit: number | null) => (limit == null ? t('None') : formatBytes(limit));

function Tile(props: { label: string; value: string; series?: number[] }) {
  return (
    <div class="card stat health-tile">
      <span class="t3 fs12">{props.label}</span>
      <strong>{props.value}</strong>
      <Show when={props.series && props.series.length > 1}>
        <svg class="spark" viewBox="0 0 120 32" preserveAspectRatio="none" aria-hidden="true">
          <polyline points={sparkPoints(props.series ?? [])} />
        </svg>
      </Show>
    </div>
  );
}

/** The server page's health view: current numbers, sparklines for the last hour, warnings. */
export function HealthTab() {
  const [health, setHealth] = createSignal<ServerHealth | null>(null);
  const [failed, setFailed] = createSignal(false);

  const load = async () => {
    try {
      setHealth(await api.serverHealth());
      setFailed(false);
    } catch {
      setFailed(true);
    }
  };

  // Polls only while this tab is mounted, i.e. while the administration window shows it.
  onMount(() => {
    void load();
    const timer = setInterval(() => void load(), HEALTH_REFRESH_MS);
    onCleanup(() => clearInterval(timer));
  });

  return (
    <Show
      when={health()}
      fallback={failed() ? <span class="t3 fs12">{t('Could not read the server health. Trying again.')}</span> : <span class="skel pulse health-skel" />}
    >
      {(info) => {
        const series = (pick: (sample: HealthSample) => number) => info().history.map(pick);
        return (
          <div class="col g16">
            <span class="t3 fs12">{t('Updated every 5 seconds')} · {t('Running for')} {formatDuration(info().uptime_secs)}</span>
            <div class="col g8">
              <span class="field-label">{t('Warnings')}</span>
              <Show when={info().warnings.length > 0} fallback={<span class="t3 fs12">{t('No warnings')}</span>}>
                <For each={info().warnings}>{(code) => <div class="health-warning" role="status">{warningText(code)}</div>}</For>
              </Show>
            </div>
            <div class="health-grid">
              <Tile label={t('Process CPU')} value={percent(info().now.cpu_percent)} series={series((s) => s.cpu_percent)} />
              <Tile label={t('Process memory')} value={formatBytes(info().now.rss_bytes)} series={series((s) => s.rss_bytes)} />
              <Tile
                label={t('System memory')}
                value={t('Used {used} of {total}', { used: formatBytes(info().now.system_used_bytes), total: formatBytes(info().now.system_total_bytes) })}
                series={series((s) => s.system_used_bytes)}
              />
              <Tile label={t('Container limit')} value={limitText(info().now.container_limit_bytes)} />
              <Tile label={t('Free disk')} value={formatBytes(info().now.disk_free_bytes)} />
              <Tile label={t('Database')} value={formatBytes(info().db_bytes)} />
              <Tile label={t('Attachments')} value={formatBytes(info().uploads_bytes)} />
              <Tile label={t('Music')} value={formatBytes(info().music_bytes)} />
              <Tile label={t('Sessions')} value={String(info().now.sessions)} series={series((s) => s.sessions)} />
              <Tile label={t('People in voice')} value={String(info().now.voice_users)} series={series((s) => s.voice_users)} />
              <Tile label={t('Incoming traffic')} value={perSecond(info().now.bytes_in_per_sec)} series={series((s) => s.bytes_in_per_sec)} />
              <Tile label={t('Outgoing traffic')} value={perSecond(info().now.bytes_out_per_sec)} series={series((s) => s.bytes_out_per_sec)} />
              <Tile label={t('Lost datagrams')} value={String(info().now.datagrams_dropped)} />
            </div>
          </div>
        );
      }}
    </Show>
  );
}
