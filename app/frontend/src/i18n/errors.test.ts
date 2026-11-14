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
  ['429: Wait 4 s; too many room joins', 'Слишком часто входите в комнаты. Подождите 4 с'],
  ['429: Wait 4 s; too many screen shares', 'Слишком часто запускаете демонстрацию экрана. Подождите 4 с'],
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

it.each([
  ['The password must be at least 8 characters', 'Пароль должен быть не короче 8 символов'],
  ['Registration failed: The password must be at least 8 characters', 'Не удалось зарегистрироваться: Пароль должен быть не короче 8 символов'],
  ['Registration failed: HTTP 400: The password must be at least 8 characters', 'Не удалось зарегистрироваться: HTTP 400: Пароль должен быть не короче 8 символов'],
  ['HTTP 400: The password must be at least 8 characters', 'HTTP 400: Пароль должен быть не короче 8 символов'],
])('the length error is in Russian in every flow: %s', (raw, expected) => {
  expect(describeError(raw)).toBe(expected);
});

it('an empty error still says something went wrong', () => {
  expect(describeError('')).toBe('Что-то пошло не так');
  expect(describeError('Error: ')).toBe('Что-то пошло не так');
});

it.each([
  ['Session revoked', 'Сеанс отозван'],
  ['Invalid token', 'Недействительный токен'],
])('the session end reason %s is translated', (raw, expected) => {
  expect(describeError(raw)).toBe(expected);
});

it('a lost QUIC connection reason is shown in Russian', () => {
  expect(describeError('connection lost')).toBe('Связь потеряна');
});
