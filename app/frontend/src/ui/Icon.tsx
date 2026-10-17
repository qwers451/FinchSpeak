import { For, type JSX } from 'solid-js';
import { ICONS, type IconName } from './iconData';

export type { IconName };

/** The icon symbols, rendered once near the root of the app. */
export function IconSprite() {
  return (
    <svg width="0" height="0" style={{ position: 'absolute' }} aria-hidden="true">
      <defs>
        <For each={Object.entries(ICONS)}>
          {([id, icon]) => (
            <symbol id={`i-${id}`} viewBox="0 0 24 24">
              <path d={icon.d} {...('sw' in icon ? { 'stroke-width': icon.sw } : {})} />
            </symbol>
          )}
        </For>
      </defs>
    </svg>
  );
}

const CUSTOM_NAMES: Partial<Record<IconName, string>> = {"full": "fullscreen", "pip": "grid", "grip": "more", "vol": "volume", "vol-off": "volume-off", "phones": "headphones", "phones-off": "headphones-off", "share": "screen-share", "logout": "logout", "x": "close", "chev-d": "chevron-down", "chev-r": "chevron-right", "clip": "paperclip", "smile": "reaction", "up": "chevron-up", "down": "chevron-down", "warn": "warning", "bars": "signal", "db": "server", "dl": "download", "moon": "eye-off", "clock": "info", "door": "logout", "play": "broadcast", "stop": "screen-share-off", "at": "user", "min": "minimize", "box": "maximize"};

type IconProps = { name: IconName; size?: 'sm' | 'md' | 'lg' | 'xl'; class?: string; style?: JSX.CSSProperties; title?: string };

/** 24 px grid, 2 px stroke, round caps. Decorative unless it has a `title`. */
export function Icon(props: IconProps) {
  const size = () => (props.size && props.size !== 'md' ? ` ${props.size}` : '');
  return (
    <svg
      class={`ic${size()}${props.class ? ` ${props.class}` : ''}`}
      style={props.style}
      aria-hidden={props.title ? undefined : 'true'}
      role={props.title ? 'img' : undefined}
      aria-label={props.title}
    >
      <use class="classic-glyph" href={`#i-${props.name}`} />
      <use class="custom-glyph" href={`./custom/assets/ui-icons/sprite.svg#fi-${CUSTOM_NAMES[props.name] ?? props.name}`} />
    </svg>
  );
}
