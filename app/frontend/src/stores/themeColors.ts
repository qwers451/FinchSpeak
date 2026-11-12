// "My colors": the person picks six colors; every other shade the interface needs
// (hover, borders, quieter text, tints) is worked out from them so the theme stays coherent.

export interface ThemeColors {
  /** Chat and main area. */
  background: string;
  /** Sidebar and side panels. */
  sidebar: string;
  /** Cards, inputs, the message box. */
  surface: string;
  text: string;
  /** Buttons, links, the selected channel. */
  accent: string;
  /** Speaking, online, live. */
  signal: string;
}

export const THEME_COLOR_KEYS: (keyof ThemeColors)[] = ['background', 'sidebar', 'surface', 'text', 'accent', 'signal'];

export const DEFAULT_THEME_COLORS: ThemeColors = {
  background: '#262624', sidebar: '#20201e', surface: '#30302e', text: '#f0eee6', accent: '#d9987e', signal: '#a8c6b2',
};

/** The "Снегирь" (Bullfinch) theme: the colours of the FinchSpeak logo, dark. */
export const BULLFINCH_COLORS: ThemeColors = { background: '#171516', sidebar: '#111011', surface: '#232021', text: '#fff3e7', accent: '#ff7a1f', signal: '#ffc46b' };

export const THEME_PRESETS: { id: string; name: string; colors: ThemeColors }[] = [
  { id: 'ember', name: 'Ember', colors: DEFAULT_THEME_COLORS },
  { id: 'ocean', name: 'Ocean', colors: { background: '#0f1b2a', sidebar: '#0b1521', surface: '#16263a', text: '#e6eef8', accent: '#4ea8de', signal: '#5ce1c6' } },
  { id: 'forest', name: 'Forest', colors: { background: '#17201a', sidebar: '#121914', surface: '#1f2b23', text: '#e8f0e6', accent: '#8fcf72', signal: '#e8c468' } },
  { id: 'sakura', name: 'Sakura', colors: { background: '#2a1f26', sidebar: '#22191f', surface: '#362832', text: '#f7e9f0', accent: '#f29bbf', signal: '#9fd8e6' } },
  { id: 'amethyst', name: 'Amethyst', colors: { background: '#1d1a2b', sidebar: '#171522', surface: '#272338', text: '#ece8fb', accent: '#a58cff', signal: '#7ee0b0' } },
  { id: 'paper', name: 'Paper', colors: { background: '#f6f3ee', sidebar: '#ece7df', surface: '#ffffff', text: '#2b2824', accent: '#c4622d', signal: '#2f8f6b' } },
];

const HEX = /^#[0-9a-f]{6}$/i;
export const isHexColor = (value: unknown): value is string => typeof value === 'string' && HEX.test(value);

/** Saved colors with anything missing or broken taken from the defaults. */
export function parseThemeColors(value: unknown): ThemeColors {
  const saved = (value && typeof value === 'object' ? value : {}) as Record<string, unknown>;
  const colors = { ...DEFAULT_THEME_COLORS };
  for (const key of THEME_COLOR_KEYS) {
    const color = saved[key];
    if (isHexColor(color)) colors[key] = color.toLowerCase();
  }
  return colors;
}

type Rgb = [number, number, number];
const toRgb = (hex: string): Rgb => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)) as Rgb;
const toHex = (rgb: Rgb) => `#${rgb.map((c) => Math.round(Math.min(255, Math.max(0, c))).toString(16).padStart(2, '0')).join('')}`;

/** `amount` of the way from `from` to `to` (0 = from, 1 = to). */
export function mix(from: string, to: string, amount: number): string {
  const a = toRgb(from);
  const b = toRgb(to);
  return toHex([0, 1, 2].map((i) => a[i] + (b[i] - a[i]) * amount) as Rgb);
}

/** WCAG relative luminance, 0 (black) to 1 (white). */
export function luminance(hex: string): number {
  const [r, g, b] = toRgb(hex).map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Dark or light text, whichever reads better on `background`. */
export const readableOn = (background: string) => (contrast(background, '#16130f') >= contrast(background, '#ffffff') ? '#16130f' : '#ffffff');

export const isLightTheme = (colors: ThemeColors) => luminance(colors.background) > 0.4;

/** Every CSS variable the interface reads, worked out from the six chosen colors. */
export function themeVariables(colors: ThemeColors): Record<string, string> {
  const { background, sidebar, surface, text, accent, signal } = colors;
  const light = isLightTheme(colors);
  const red = light ? '#c0392b' : '#b94343';
  return {
    '--bg-app': background,
    '--bg-panel': sidebar,
    '--bg-card': surface,
    '--bg-hover': mix(surface, text, 0.08),
    '--bg-deep': light ? mix(surface, background, 0.35) : mix(sidebar, '#000000', 0.3),
    '--line': mix(surface, text, 0.2),
    '--line-soft': mix(background, text, 0.1),
    '--text': text,
    '--text-2': mix(text, background, 0.24),
    '--text-3': mix(text, background, 0.4),
    '--yellow': accent,
    '--yellow-2': mix(accent, light ? '#000000' : '#ffffff', 0.16),
    '--amber': mix(accent, background, 0.12),
    '--orange': accent,
    '--on-accent': readableOn(accent),
    '--on-signal': readableOn(signal),
    '--red': red,
    '--red-text': light ? '#a5281c' : '#f0a09b',
    '--violet': mix(signal, background, 0.45),
    '--violet-2': mix(signal, background, 0.3),
    '--violet-text': mix(signal, text, 0.5),
    '--cyan': signal,
    '--teal': signal,
    '--blue': mix(signal, accent, 0.35),
    '--wave': signal,
    '--tint-accent': mix(background, accent, 0.16),
    '--tint-signal': mix(background, signal, 0.16),
    '--tint-danger': mix(background, red, 0.2),
    '--glow-focus': `0 0 0 3px ${accent}55`,
    '--glow-speak': `0 0 0 2px ${signal}`,
  };
}

/** Name of every variable `themeVariables` sets, to clear them when another theme is chosen. */
export const THEME_VARIABLE_NAMES = Object.keys(themeVariables(DEFAULT_THEME_COLORS));

// --- a theme from a picture -------------------------------------------------------------------

type Hsl = [number, number, number];

function rgbToHsl([r, g, b]: Rgb): Hsl {
  const [rn, gn, bn] = [r / 255, g / 255, b / 255];
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === rn ? (gn - bn) / d + (gn < bn ? 6 : 0) : max === gn ? (bn - rn) / d + 2 : (rn - gn) / d + 4;
  return [h * 60, s, l];
}

export function hslToHex(h: number, s: number, l: number): string {
  const hue = ((h % 360) + 360) % 360;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] = hue < 60 ? [c, x, 0] : hue < 120 ? [x, c, 0] : hue < 180 ? [0, c, x] : hue < 240 ? [0, x, c] : hue < 300 ? [x, 0, c] : [c, 0, x];
  return toHex([(r + m) * 255, (g + m) * 255, (b + m) * 255]);
}

const hueDistance = (a: number, b: number) => {
  const d = Math.abs(a - b) % 360;
  return d > 180 ? 360 - d : d;
};
const clamp01 = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

/**
 * Six theme colors from the pixels of a picture (RGBA, as a canvas gives them).
 * The backgrounds take the picture's main hue, quiet enough to read on; the accent is its most
 * vivid color and the signal a vivid color of another hue. A light picture gives a light theme.
 */
export function paletteFromPixels(pixels: Uint8ClampedArray | number[]): ThemeColors {
  const buckets = new Map<number, { r: number; g: number; b: number; n: number }>();
  let total = 0;
  let lightSum = 0;
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    if (pixels[i + 3] < 128) continue;
    const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
    const key = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
    const bucket = buckets.get(key) ?? { r: 0, g: 0, b: 0, n: 0 };
    bucket.r += r; bucket.g += g; bucket.b += b; bucket.n += 1;
    buckets.set(key, bucket);
    total += 1;
    lightSum += luminance(toHex([r, g, b]));
  }
  if (total === 0) return DEFAULT_THEME_COLORS;

  const colors = [...buckets.values()]
    .map(({ r, g, b, n }) => ({ hsl: rgbToHsl([r / n, g / n, b / n]), share: n / total }))
    .sort((a, b) => b.share - a.share);
  // The main hue is the one covering the most of the picture: hues are pooled in 30° sectors,
  // so a gradient sky counts as one color instead of many slightly different ones.
  const sectors = Array.from({ length: 12 }, () => ({ weight: 0, x: 0, y: 0, sat: 0 }));
  for (const { hsl: [h, s], share } of colors) {
    if (s < 0.08) continue;
    const sector = sectors[Math.floor(h / 30) % 12];
    sector.weight += share;
    sector.x += Math.cos((h * Math.PI) / 180) * share;
    sector.y += Math.sin((h * Math.PI) / 180) * share;
    sector.sat += s * share;
  }
  const main = sectors.reduce((best, sector) => (sector.weight > best.weight ? sector : best));
  const [baseHue, baseSat] = main.weight > 0.1
    ? [((Math.atan2(main.y, main.x) * 180) / Math.PI + 360) % 360, main.sat / main.weight]
    : colors[0].hsl;
  // Vivid and common beats vivid and rare; very dark or very pale colors make poor accents.
  const vividness = ({ hsl: [, s, l], share }: (typeof colors)[number]) => Math.sqrt(share) * s * (1 - Math.abs(l - 0.55) * 1.4);
  const vivid = colors.filter((color) => color.hsl[1] > 0.25 && color.hsl[2] > 0.15 && color.hsl[2] < 0.9).sort((a, b) => vividness(b) - vividness(a));
  const accent = vivid[0]?.hsl ?? [baseHue, 0.6, 0.6];
  // The signal should stand out from both the accent and the backgrounds.
  const apart = (color: (typeof colors)[number], from: number, by: number) => hueDistance(color.hsl[0], from) >= by;
  const signal = (vivid.find((color) => apart(color, accent[0], 50) && apart(color, baseHue, 40))
    ?? vivid.find((color) => apart(color, accent[0], 50)))?.hsl ?? [accent[0] + 150, accent[1], accent[2]];

  const light = lightSum / total > 0.55;
  const sat = Math.min(baseSat, 0.35);
  if (light) {
    return {
      background: hslToHex(baseHue, sat * 0.5, 0.95),
      sidebar: hslToHex(baseHue, sat * 0.5, 0.91),
      surface: hslToHex(baseHue, sat * 0.3, 0.99),
      text: hslToHex(baseHue, 0.15, 0.14),
      accent: hslToHex(accent[0], clamp01(accent[1], 0.45, 0.85), clamp01(accent[2], 0.36, 0.46)),
      signal: hslToHex(signal[0], clamp01(signal[1], 0.4, 0.8), clamp01(signal[2], 0.32, 0.42)),
    };
  }
  return {
    background: hslToHex(baseHue, sat * 0.6, 0.13),
    sidebar: hslToHex(baseHue, sat * 0.6, 0.1),
    surface: hslToHex(baseHue, sat * 0.55, 0.18),
    text: hslToHex(baseHue, 0.15, 0.93),
    accent: hslToHex(accent[0], clamp01(accent[1], 0.45, 0.9), clamp01(accent[2], 0.58, 0.72)),
    signal: hslToHex(signal[0], clamp01(signal[1], 0.4, 0.85), clamp01(signal[2], 0.58, 0.75)),
  };
}

/** Read a picture file and work out a theme from it (scaled down first: 64 px is plenty). */
export async function paletteFromImage(file: Blob): Promise<ThemeColors> {
  const bitmap = await createImageBitmap(file);
  try {
    const scale = Math.min(1, 64 / Math.max(bitmap.width, bitmap.height));
    const width = Math.max(1, Math.round(bitmap.width * scale));
    const height = Math.max(1, Math.round(bitmap.height * scale));
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext('2d');
    if (!context) return DEFAULT_THEME_COLORS;
    context.drawImage(bitmap, 0, 0, width, height);
    return paletteFromPixels(context.getImageData(0, 0, width, height).data);
  } finally {
    bitmap.close();
  }
}
