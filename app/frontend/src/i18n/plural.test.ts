import { afterEach, expect, it } from 'vitest';
import { plural } from './index';
import { DEFAULT_PREFS, setPrefs } from '../stores/settingsStore';

const setLanguage = (language: 'ru' | 'en') => setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language } });
const NEW_MESSAGES = { one: '{count} new message', other: '{count} new messages' };

afterEach(() => setLanguage('en'));

it('ru: picks one / few / many by the Russian rules', () => {
  setLanguage('ru');
  const said = (count: number) => plural(count, NEW_MESSAGES);
  expect(said(1)).toBe('1 новое сообщение');
  expect(said(2)).toBe('2 новых сообщения');
  expect(said(5)).toBe('5 новых сообщений');
  expect(said(21)).toBe('21 новое сообщение');
  expect(said(22)).toBe('22 новых сообщения');
  expect(said(25)).toBe('25 новых сообщений');
  expect(said(111)).toBe('111 новых сообщений');
  expect(said(112)).toBe('112 новых сообщений');
});

it('en: message / messages', () => {
  setLanguage('en');
  expect(plural(1, NEW_MESSAGES)).toBe('1 new message');
  expect(plural(2, NEW_MESSAGES)).toBe('2 new messages');
  expect(plural(21, NEW_MESSAGES)).toBe('21 new messages');
  expect(plural(0, NEW_MESSAGES)).toBe('0 new messages');
});

it('formats large counts with the locale and falls back to the English other-form when ru has no entry', () => {
  setLanguage('ru');
  expect(plural(1200, NEW_MESSAGES)).toBe(`${(1200).toLocaleString('ru-RU')} новых сообщений`);
  expect(plural(3, { one: '{count} spare thing', other: '{count} spare things' })).toBe('3 spare things');
});
