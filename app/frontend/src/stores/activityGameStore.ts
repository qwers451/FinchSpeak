import { createSignal } from 'solid-js';

export type UserActivity = { user_id: string; game: string };
const [games, setGames] = createSignal<Record<string, string>>({});

export const gameOf = (userId: string): string | undefined => games()[userId];

export function setGameActivity(activity: { user_id: string; game: string | null }) {
  setGames((previous) => {
    const { [activity.user_id]: _old, ...rest } = previous;
    return activity.game ? { ...rest, [activity.user_id]: activity.game } : rest;
  });
}

export function restoreGameActivities(activities: UserActivity[] = []) {
  setGames(Object.fromEntries(activities.map(({ user_id, game }) => [user_id, game])));
}
