// The Claude Code sessions one pill follows, most recently heard from first. One pill serves every
// session, and some clients start sessions of their own that end seconds later: an ending session must
// only drop itself, so "Open terminal" still has the one the user is working in.

export interface LiveSession {
  id: string;
  cwd: string;
}

/** More than a handful at once is a session that went away without a SessionEnd. */
const MAX_LIVE = 8;

/** `id` was just heard from: it goes first, keeping the folder it had when this event has none. */
export function follow(list: LiveSession[], id: string, cwd: string): LiveSession[] {
  if (!id) return list;
  const known = list.find((s) => s.id === id);
  return [{ id, cwd: cwd || known?.cwd || "" }, ...list.filter((s) => s.id !== id)].slice(0, MAX_LIVE);
}

/** `id` is over. */
export function end(list: LiveSession[], id: string): LiveSession[] {
  return list.filter((s) => s.id !== id);
}