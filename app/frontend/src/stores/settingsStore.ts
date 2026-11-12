import { createEffect, createRoot, createSignal } from 'solid-js';
import { DEFAULT_THEME_COLORS, isLightTheme, parseThemeColors, THEME_VARIABLE_NAMES, themeVariables, type ThemeColors } from './themeColors';

// Settings that live on this computer only: notifications, looks.

export interface NotificationPrefs {
  /** Tell me when I am mentioned (also when the window is in the background). */
  mentions: boolean;
  /** Tell me about every message in channels I am not looking at. */
  allMessages: boolean;
  /** Tell me when someone joins the voice room I am in. */
  voiceJoin: boolean;
  sounds: boolean;
  /** Feedback tones for my own voice actions: joining and leaving, mute and deafen, push-to-talk. */
  voiceSounds: boolean;
  /** 0..1 */
  soundVolume: number;
  /** Flash the taskbar button while the window is in the background. */
  flash: boolean;
}

export interface AppearancePrefs {
  /** `user` paints the interface with `customColors`. */
  theme: 'claude' | 'custom' | 'user';
  customColors: ThemeColors;
  language: 'ru' | 'en';
  density: 'comfortable' | 'compact';
  textSize: 'small' | 'medium' | 'large';
  reduceMotion: boolean;
  speakingStyle: 'pulse' | 'ring' | 'waves' | 'bars' | 'glow';
  showMembers: boolean;
}

export interface Prefs {
  privacy: { shareActivity: boolean };
  notifications: NotificationPrefs;
  appearance: AppearancePrefs;
  /** Channel id -> muted until (ms since 1970; Infinity as null = until turned back on). */
  mutedChannels: Record<string, number | null>;
}

export const DEFAULT_PREFS: Prefs = {
  privacy: { shareActivity: true },
  notifications: { mentions: true, allMessages: false, voiceJoin: true, sounds: true, voiceSounds: true, soundVolume: 0.6, flash: true },
  appearance: { theme: 'claude', customColors: DEFAULT_THEME_COLORS, language: 'ru', density: 'comfortable', textSize: 'medium', reduceMotion: false, speakingStyle: 'pulse', showMembers: true },
  mutedChannels: {},
};

const KEY = 'finchspeak.prefs';

/** Scale for each text size, applied as the zoom of the whole interface. */
export const TEXT_ZOOM: Record<AppearancePrefs['textSize'], number> = { small: 0.92, medium: 1, large: 1.12 };

const bool = (value: unknown, fallback: boolean) => (typeof value === 'boolean' ? value : fallback);
const pick = <T extends string>(value: unknown, allowed: readonly T[], fallback: T): T => (allowed.includes(value as T) ? (value as T) : fallback);

/** Read what was saved, repairing anything that is missing or of the wrong type. */
export function parsePrefs(raw: string | null): Prefs {
  let saved: Partial<Record<keyof Prefs, Record<string, unknown>>> = {};
  try {
    const parsed = JSON.parse(raw ?? '{}');
    if (parsed && typeof parsed === 'object') saved = parsed;
  } catch { /* corrupt: start from the defaults */ }
  const n = saved.notifications ?? {};
  const a = saved.appearance ?? {};
  const d = DEFAULT_PREFS;
  const volume = typeof n.soundVolume === 'number' && Number.isFinite(n.soundVolume) ? Math.min(1, Math.max(0, n.soundVolume)) : d.notifications.soundVolume;
  const muted: Record<string, number | null> = {};
  for (const [id, until] of Object.entries(saved.mutedChannels ?? {})) {
    if (until === null || (typeof until === 'number' && Number.isFinite(until))) muted[id] = until as number | null;
  }
  return {
    privacy: { shareActivity: bool(saved.privacy?.shareActivity, d.privacy.shareActivity) },
    notifications: {
      mentions: bool(n.mentions, d.notifications.mentions),
      allMessages: bool(n.allMessages, d.notifications.allMessages),
      voiceJoin: bool(n.voiceJoin, d.notifications.voiceJoin),
      sounds: bool(n.sounds, d.notifications.sounds),
      voiceSounds: bool(n.voiceSounds, d.notifications.voiceSounds),
      soundVolume: volume,
      flash: bool(n.flash, d.notifications.flash),
    },
    appearance: {
      theme: pick(a.theme, ['claude', 'custom', 'user'] as const, d.appearance.theme),
      customColors: parseThemeColors(a.customColors),
      language: pick(a.language, ['ru', 'en'] as const, d.appearance.language),
      density: pick(a.density, ['comfortable', 'compact'] as const, d.appearance.density),
      textSize: pick(a.textSize, ['small', 'medium', 'large'] as const, d.appearance.textSize),
      reduceMotion: bool(a.reduceMotion, d.appearance.reduceMotion),
      speakingStyle: pick(a.speakingStyle, ['pulse', 'ring', 'waves', 'bars', 'glow'], d.appearance.speakingStyle),
      showMembers: bool(a.showMembers, d.appearance.showMembers),
    },
    mutedChannels: muted,
  };
}

function load(): Prefs {
  try {
    return parsePrefs(localStorage.getItem(KEY));
  } catch {
    return DEFAULT_PREFS;
  }
}

export const [prefs, setPrefs] = createSignal<Prefs>(load());

export function updatePrivacy(change: Partial<Prefs['privacy']>) {
  setPrefs((previous) => ({ ...previous, privacy: { ...previous.privacy, ...change } }));
}

export function updateNotifications(change: Partial<NotificationPrefs>) {
  setPrefs((previous) => ({ ...previous, notifications: { ...previous.notifications, ...change } }));
}

export function updateAppearance(change: Partial<AppearancePrefs>) {
  setPrefs((previous) => ({ ...previous, appearance: { ...previous.appearance, ...change } }));
}

/** Mute a channel for `ms` milliseconds, or until turned back on when `ms` is null. */
export function muteChannel(channelId: string, ms: number | null, now = Date.now()) {
  setPrefs((previous) => ({ ...previous, mutedChannels: { ...previous.mutedChannels, [channelId]: ms === null ? null : now + ms } }));
}

export function unmuteChannel(channelId: string) {
  setPrefs((previous) => {
    const { [channelId]: _gone, ...rest } = previous.mutedChannels;
    return { ...previous, mutedChannels: rest };
  });
}

export function isChannelMuted(channelId: string, now = Date.now()): boolean {
  const entry = prefs().mutedChannels;
  if (!(channelId in entry)) return false;
  const until = entry[channelId];
  return until === null || until > now;
}

/** How long colors take to blend into a newly chosen theme. */
export const THEME_BLEND_MS = 420;
let blendTimer: ReturnType<typeof setTimeout> | undefined;
let paintedColors: string | null = null;

/** Let every color blend into the new theme for a moment instead of jumping. */
function blendColors(root: HTMLElement, appearance: AppearancePrefs) {
  const reduced = appearance.reduceMotion || (typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches);
  if (reduced) return;
  root.dataset.themeBlend = 'true';
  clearTimeout(blendTimer);
  blendTimer = setTimeout(() => { delete root.dataset.themeBlend; }, THEME_BLEND_MS);
}

/** Put the looks on the page (called when they change). */
export function applyAppearance(appearance: AppearancePrefs, root: HTMLElement = document.documentElement) {
  const colors = appearance.theme === 'user' ? JSON.stringify(appearance.customColors) : null;
  const firstPaint = !root.dataset.theme;
  const changed = root.dataset.theme !== appearance.theme || colors !== paintedColors;
  if (changed && !firstPaint) blendColors(root, appearance);
  paintedColors = colors;
  if (appearance.theme === 'user') {
    for (const [name, value] of Object.entries(themeVariables(appearance.customColors))) root.style.setProperty(name, value);
    root.style.colorScheme = isLightTheme(appearance.customColors) ? 'light' : 'dark';
  } else {
    for (const name of THEME_VARIABLE_NAMES) root.style.removeProperty(name);
    root.style.removeProperty('color-scheme');
  }
  root.dataset.theme = appearance.theme;
  root.lang = appearance.language;
  root.dataset.density = appearance.density;
  root.dataset.reduceMotion = String(appearance.reduceMotion);
  root.dataset.speakingStyle = appearance.speakingStyle;
  root.style.setProperty('--zoom', String(TEXT_ZOOM[appearance.textSize]));
}

// Remember changes and apply the looks, for the life of the app.
if (typeof document !== 'undefined') {
  createRoot(() => {
    createEffect(() => {
      const current = prefs();
      applyAppearance(current.appearance);
      try { localStorage.setItem(KEY, JSON.stringify(current)); } catch { /* not remembered */ }
    });
  });
}
