export type Strength = { score: 0 | 1 | 2 | 3 | 4; label: string };

/** A rough guide for the strength bar (length and variety); the server enforces the real minimum. */
export function passwordStrength(password: string): Strength {
  if (!password) return { score: 0, label: '' };
  let points = 0;
  if (password.length >= 8) points++;
  if (password.length >= 12) points++;
  if (/[a-z]/.test(password) && /[A-Z]/.test(password)) points++;
  if (/\d/.test(password) && /[^A-Za-z0-9]/.test(password)) points++;
  else if (/\d|[^A-Za-z0-9]/.test(password)) points += password.length >= 10 ? 1 : 0;
  const score = Math.min(4, Math.max(1, points)) as 1 | 2 | 3 | 4;
  return { score, label: ['', 'Weak', 'Fair', 'Good', 'Strong'][score] };
}
