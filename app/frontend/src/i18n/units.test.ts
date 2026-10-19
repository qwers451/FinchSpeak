import { afterEach, expect, it } from 'vitest';
import { formatBitrate, formatBytes, formatDuration } from './index';
import { DEFAULT_PREFS, setPrefs } from '../stores/settingsStore';

const setLanguage = (language: 'ru' | 'en') => setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language } });
const MB = 1024 * 1024;

afterEach(() => setLanguage('en'));

it('ru: sizes, bitrates and durations use Russian units and decimal commas', () => {
  setLanguage('ru');
  expect(formatBytes(500)).toBe('500 Б');
  expect(formatBytes(2048)).toBe('2 КБ');
  expect(formatBytes(5.5 * MB)).toBe('5,5 МБ');
  expect(formatBytes(3 * 1024 * MB)).toBe('3 ГБ');
  expect(formatBytes(500, 'KB')).toBe('1 КБ');
  expect(formatBitrate(850)).toBe('850 кбит/с');
  expect(formatBitrate(2500)).toBe('2,5 Мбит/с');
  expect(formatDuration(30)).toBe('0 мин');
  expect(formatDuration(5 * 60)).toBe('5 мин');
  expect(formatDuration(2 * 3600)).toBe('2 ч');
  expect(formatDuration(86400 + 2 * 3600 + 59)).toBe('1 д 2 ч');
});

it('en: the units and the layout stay as they were', () => {
  setLanguage('en');
  expect(formatBytes(500)).toBe('500 B');
  expect(formatBytes(2048)).toBe('2 KB');
  expect(formatBytes(5.5 * MB)).toBe('5.5 MB');
  expect(formatBytes(500, 'KB')).toBe('1 KB');
  expect(formatBitrate(850)).toBe('850 kbps');
  expect(formatBitrate(2500)).toBe('2.5 Mbps');
  expect(formatDuration(5 * 60)).toBe('5 min');
  expect(formatDuration(2 * 3600)).toBe('2 h');
  expect(formatDuration(86400 + 2 * 3600 + 59)).toBe('1 d 2 h');
});
