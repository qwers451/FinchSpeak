import { createEffect, createResource, onCleanup, onMount, Show } from 'solid-js';
import { avatarSrc } from '../stores/avatarCache';
import { bindLevel } from '../stores/levelStore';
import { avatarColor, initialOf } from './avatarColor';

export type AvatarSize = 16 | 24 | 32 | 40 | 56 | 72 | 96 | 128;
export type AvatarStatus = 'on' | 'idle' | 'dnd' | 'off';

type AvatarProps = {
  /** Display name: gives the initial. */
  name?: string | null;
  /** Stable id: gives the colour. */
  id?: string;
  size?: AvatarSize;
  /** Picture, when the person has uploaded one. */
  src?: string | null;
  status?: AvatarStatus;
  speaking?: boolean;
  levelPeerId?: number;
  ref?: (el: HTMLSpanElement) => void;
  /** Deleted user. */
  ghost?: boolean;
  class?: string;
};

export function Avatar(props: AvatarProps) {
  let root!: HTMLSpanElement;
  onMount(() => {
    createEffect(() => {
      const peerId = props.levelPeerId;
      if (peerId !== undefined) onCleanup(bindLevel(root, peerId));
    });
  });
  const [loaded] = createResource(() => props.src ?? null, (address) => avatarSrc(address).catch(() => null));
  const picture = () => (props.src ? loaded() ?? null : null);
  const size = () => props.size ?? 40;
  const sizeClass = () => (size() === 40 ? '' : ` s${size()}`);
  const color = () => (props.ghost ? ' ghost' : ` c${avatarColor(props.id ?? props.name ?? '')}`);
  return (
    <span
      ref={(el) => { root = el; props.ref?.(el); }}
      class={`av${sizeClass()}${color()}${props.speaking ? ' speaking' : ''}${props.class ? ` ${props.class}` : ''}`}
      data-testid="avatar"
    >
      <span class="av-art" style={picture() ? { 'background-image': `url(${picture()})`, 'background-size': 'cover', 'background-position': 'center' } : undefined}>
        <Show when={!picture()}>{props.ghost ? '?' : initialOf(props.name)}</Show>
      </span>
      <span class="av-speaking-ring" aria-hidden="true" />
      <Show when={props.speaking}>
        <span class="av-speaking-glow" aria-hidden="true" />
        <Show when={size() >= 40}>
          <span class="av-bars" aria-hidden="true"><i /><i /><i /><i /></span>
        </Show>
      </Show>
      <Show when={props.status && size() >= 24}><span class={`st ${props.status === 'off' ? '' : props.status}`} /></Show>
    </span>
  );
}
