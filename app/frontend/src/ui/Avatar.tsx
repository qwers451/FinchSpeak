import { Show } from 'solid-js';
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
  /** Deleted user. */
  ghost?: boolean;
  class?: string;
};

export function Avatar(props: AvatarProps) {
  const size = () => props.size ?? 40;
  const sizeClass = () => (size() === 40 ? '' : ` s${size()}`);
  const color = () => (props.ghost ? ' ghost' : ` c${avatarColor(props.id ?? props.name ?? '')}`);
  return (
    <span
      class={`av${sizeClass()}${color()}${props.speaking ? ' speaking' : ''}${props.class ? ` ${props.class}` : ''}`}
      style={props.src ? { 'background-image': `url(${props.src})`, 'background-size': 'cover', 'background-position': 'center' } : undefined}
      data-testid="avatar"
    >
      <Show when={!props.src}>{props.ghost ? '?' : initialOf(props.name)}</Show>
      <Show when={props.status && size() >= 24}><span class={`st ${props.status === 'off' ? '' : props.status}`} /></Show>
    </span>
  );
}
