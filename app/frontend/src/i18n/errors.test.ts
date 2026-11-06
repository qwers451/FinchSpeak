import { beforeEach, expect, it } from 'vitest';
import { DEFAULT_PREFS, setPrefs } from '../stores/settingsStore';
import { describeError } from './errors';

beforeEach(() => {
  setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'ru' } });
});

it.each([
  ['Cannot reach the server: connection refused', 'Сервер недоступен: connection refused'],
  ['Signed in, but the QUIC connection failed: timeout', 'Вход выполнен, но голосовое соединение не установилось. Проверьте, открыт ли UDP-порт 4433.'],
  ['Registered, but the QUIC connection failed: timeout', 'Регистрация прошла, но голосовое соединение не установилось. Проверьте, открыт ли UDP-порт 4433.'],
  ['Registration failed: name taken', 'Не удалось зарегистрироваться: name taken'],
  ['Login failed: bad password', 'Не удалось войти: bad password'],
  ['The server https://x is not trusted yet', 'Подтвердите сертификат сервера, затем войдите.'],
  ['The certificate of x changed since you trusted it', 'Сертификат сервера изменился. Не входите, пока администратор не подтвердит замену.'],
  ['Logged out locally, but could not revoke the session', 'Вы вышли на этом компьютере. Сервер недоступен, поэтому сессия на нём не завершена.'],
  ['Invalid server address: x', 'Неверный адрес сервера'],
  ['Not authenticated', 'Вы не вошли в аккаунт'],
])('translates %s', (raw, expected) => {
  expect(describeError(raw)).toBe(expected);
});

it('strips the Error prefix', () => {
  expect(describeError(new Error('Login failed: nope'))).toBe('Не удалось войти: nope');
  expect(describeError('Error: Login failed: nope')).toBe('Не удалось войти: nope');
});

it('unknown_error_passes_through', () => {
  expect(describeError('Error: something odd')).toBe('something odd');
});
