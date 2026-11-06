import { createSignal } from 'solid-js';
import { api } from '../services/api';
import { pushToast } from '../ui';
import { t } from '../i18n';

export const [availableUpdate, setAvailableUpdate] = createSignal<{ version: string; notes: string } | null>(null);

const FIRST_CHECK_MS = 15_000;
const CHECK_EVERY_MS = 24 * 60 * 60 * 1000;
const announced = new Set<string>();

async function check() {
  try {
    const found = await api.checkUpdate();
    setAvailableUpdate(found);
    if (found && !announced.has(found.version)) {
      announced.add(found.version);
      pushToast({
        kind: 'info',
        title: t('FinchSpeak {version} is available', { version: found.version }),
        text: t('Install it from Settings → Keys and about.'),
      });
    }
  } catch (err) {
    console.warn('update check failed', err);
  }
}

/** Looks for a new version 15 s after the start and then once a day; returns a function that stops it. */
export function startUpdateChecks(): () => void {
  let interval: ReturnType<typeof setInterval> | undefined;
  const first = setTimeout(() => {
    void check();
    interval = setInterval(() => void check(), CHECK_EVERY_MS);
  }, FIRST_CHECK_MS);
  return () => {
    clearTimeout(first);
    if (interval) clearInterval(interval);
  };
}
