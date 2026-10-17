import { beforeEach } from 'vitest';
import { DEFAULT_PREFS, setPrefs } from './stores/settingsStore';

// Existing interaction tests use English selectors. Localization tests set their locale explicitly.
beforeEach(() => { setPrefs({ ...DEFAULT_PREFS, appearance: { ...DEFAULT_PREFS.appearance, language: 'en' } }); });
