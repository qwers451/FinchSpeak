import { createEffect, createSignal, For, onCleanup, Show, untrack } from 'solid-js';
import { appError, setAppError } from '../stores/notificationStore';
import { Icon, type IconName } from './Icon';
import { dismissToast, pauseToast, resumeToast, pushToast, toasts, type Toast, type ToastKind } from './toastStore';
import { t } from '../i18n';

const ICON: Record<ToastKind, IconName> = { info: 'info', success: 'check', warn: 'warn', error: 'warn' };

/** Bottom-right stack. Also shows whatever the stores report through `setAppError`. */
export function ToastHost() {
  const [visible, setVisible] = createSignal<Toast[]>(toasts());
  const exits = new Map<number, ReturnType<typeof setTimeout>>();
  const active = (id: number) => toasts().some(toast => toast.id === id);
  createEffect(() => {
    const current = toasts();
    const previous = untrack(visible);
    const instant = import.meta.env.MODE === 'test' || document.documentElement.dataset.reduceMotion === 'true' || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    const leaving = instant ? [] : previous.filter(toast => !current.some(item => item.id === toast.id)).slice(-3);
    const keep = new Set([...current, ...leaving].map(toast => toast.id));
    for (const [id, timer] of exits) {
      if (!keep.has(id) || current.some(toast => toast.id === id)) { clearTimeout(timer); exits.delete(id); }
    }
    for (const toast of leaving) {
      if (!exits.has(toast.id)) exits.set(toast.id, setTimeout(() => {
        exits.delete(toast.id);
        setVisible(list => list.filter(item => item.id !== toast.id));
      }, 140));
    }
    setVisible([...previous.filter(toast => keep.has(toast.id)), ...current.filter(toast => !previous.some(item => item.id === toast.id))]);
  });
  onCleanup(() => {
    for (const timer of exits.values()) clearTimeout(timer);
    for (const toast of toasts()) {
      resumeToast(toast.id, 'pointer');
      resumeToast(toast.id, 'focus');
    }
  });
  createEffect(() => {
    const message = appError();
    if (message) {
      pushToast({ kind: 'error', title: message });
      setAppError(null);
    }
  });

  return (
    <div class="toast-host" role="region" aria-label={t('Notifications')}>
      <For each={visible()}>
        {(toast) => (
          <div
            data-exiting={!active(toast.id) ? 'true' : undefined}
            inert={!active(toast.id) ? true : undefined}
            aria-hidden={!active(toast.id) ? 'true' : undefined}
            onPointerEnter={() => pauseToast(toast.id, 'pointer')}
            onPointerLeave={() => resumeToast(toast.id, 'pointer')}
            onFocusIn={() => pauseToast(toast.id, 'focus')}
            onFocusOut={(event) => {
              if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) resumeToast(toast.id, 'focus');
            }}
            class={`toast ${toast.kind === 'info' ? '' : toast.kind}`} role={!active(toast.id) ? undefined : toast.kind === 'error' ? 'alert' : 'status'}>
            <Icon name={ICON[toast.kind]} />
            <div class="col grow">
              <strong>{toast.title}</strong>
              <Show when={toast.text}>
                <span class="t2 fs13">{toast.text}</span>
              </Show>
              <Show when={toast.action}>
                {(action) => (
                  <button
                    type="button"
                    class="link-btn"
                    disabled={!active(toast.id)}
                    onClick={() => {
                      if (!active(toast.id)) return;
                      dismissToast(toast.id);
                      action().run();
                    }}
                  >
                    {t(action().label)}
                  </button>
                )}
              </Show>
            </div>
            <button type="button" class="icon-close" disabled={!active(toast.id)} aria-label={t('Dismiss')} onClick={() => dismissToast(toast.id)}>
              <Icon name="x" size="sm" />
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
