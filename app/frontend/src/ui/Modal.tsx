import { createEffect, createUniqueId, onCleanup, Show, type JSX } from 'solid-js';
import { Button } from './Button';

type ModalProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  children: JSX.Element;
  /** CSS width of the card. */
  width?: string;
  /** Esc and a click outside close it (default). Turn off for decisions that must be answered. */
  dismissable?: boolean;
  /** Red border for warnings. */
  tone?: 'danger';
};

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** A dialog over a dimmed window: traps focus, closes on Esc, gives focus back afterwards. */
export function Modal(props: ModalProps) {
  const titleId = createUniqueId();
  let card: HTMLDivElement | undefined;

  createEffect(() => {
    if (!props.open) return;
    const previous = document.activeElement as HTMLElement | null;
    queueMicrotask(() => {
      const target = card?.querySelector<HTMLElement>('[data-autofocus] button, [data-autofocus]') ?? card?.querySelector<HTMLElement>(FOCUSABLE);
      target?.focus();
    });
    onCleanup(() => previous?.focus?.());
  });

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && props.dismissable !== false) {
      event.stopPropagation();
      props.onClose();
    } else if (event.key === 'Tab' && card) {
      const items = Array.from(card.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  };

  return (
    <Show when={props.open}>
      <div
        class="overlay"
        onMouseDown={(event) => event.target === event.currentTarget && props.dismissable !== false && props.onClose()}
        onKeyDown={onKeyDown}
      >
        <div
          ref={card}
          class="modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          style={{
            width: props.width ?? '480px',
            ...(props.tone === 'danger' ? { 'border-color': 'var(--red)', 'border-width': '2px' } : {}),
          }}
        >
          <h2 id={titleId} class="modal-title">
            {props.title}
          </h2>
          {props.children}
        </div>
      </div>
    </Show>
  );
}

type ConfirmProps = {
  open: boolean;
  title: string;
  message: JSX.Element;
  confirmLabel: string;
  cancelLabel?: string;
  /** Destructive confirmations are red and the cancel button gets the initial focus. */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog(props: ConfirmProps) {
  return (
    <Modal open={props.open} onClose={props.onCancel} title={props.title} width="420px">
      <div class="col g16">
        <div class="t2">{props.message}</div>
        <div class="row g8" style={{ 'justify-content': 'flex-end' }}>
          <span data-autofocus={props.danger ? '' : undefined} style={{ display: 'contents' }}>
            <Button variant="secondary" onClick={props.onCancel}>
              {props.cancelLabel ?? 'Cancel'}
            </Button>
          </span>
          <span data-autofocus={props.danger ? undefined : ''} style={{ display: 'contents' }}>
            <Button variant={props.danger ? 'danger' : 'primary'} onClick={props.onConfirm}>
              {props.confirmLabel}
            </Button>
          </span>
        </div>
      </div>
    </Modal>
  );
}
