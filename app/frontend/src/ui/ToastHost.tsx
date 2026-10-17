import { createEffect, For, Show } from 'solid-js';
import { appError, setAppError } from '../stores/notificationStore';
import { Icon, type IconName } from './Icon';
import { dismissToast, pushToast, toasts, type ToastKind } from './toastStore';
import { t } from '../i18n';

const ICON: Record<ToastKind, IconName> = { info: 'info', success: 'check', warn: 'warn', error: 'warn' };

/** Bottom-right stack. Also shows whatever the stores report through `setAppError`. */
export function ToastHost() {
  createEffect(() => {
    const message = appError();
    if (message) {
      pushToast({ kind: 'error', title: message });
      setAppError(null);
    }
  });

  return (
    <div class="toast-host" role="region" aria-label={t('Notifications')}>
      <For each={toasts()}>
        {(toast) => (
          <div class={`toast ${toast.kind === 'info' ? '' : toast.kind}`} role={toast.kind === 'error' ? 'alert' : 'status'}>
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
                    onClick={() => {
                      action().run();
                      dismissToast(toast.id);
                    }}
                  >
                    {t(action().label)}
                  </button>
                )}
              </Show>
            </div>
            <button type="button" class="icon-close" aria-label={t('Dismiss')} onClick={() => dismissToast(toast.id)}>
              <Icon name="x" size="sm" />
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
