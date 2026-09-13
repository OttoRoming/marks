import { eq } from 'drizzle-orm';
import { db } from './db';
import { session, user } from './db/schema';

export const SESSION_COOKIE_NAME = 'token';

export type SessionUser = {
	id: string;
	username: string;
	is_admin: boolean;
};

export type ValidatedSession = {
	session: { id: string; expires_at: Date };
	user: SessionUser;
};

/**
 * Looks up a session by token and returns it with its owning user, or `null` when the
 * token is unknown or expired. Expired rows are deleted on sight.
 *
 * The user projection deliberately omits `password` so the hash never travels with
 * `locals.user` into route handlers or page data.
 */
export async function validateSessionToken(token: string): Promise<ValidatedSession | null> {
	const [row] = await db
		.select({
			session_id: session.id,
			expires_at: session.expires_at,
			user_id: user.id,
			username: user.username,
			is_admin: user.is_admin
		})
		.from(session)
		.innerJoin(user, eq(session.user_id, user.id))
		.where(eq(session.id, token));

	if (!row) {
		return null;
	}

	if (row.expires_at.getTime() <= Date.now()) {
		await db.delete(session).where(eq(session.id, token));
		return null;
	}

	return {
		session: { id: row.session_id, expires_at: row.expires_at },
		user: { id: row.user_id, username: row.username, is_admin: row.is_admin }
	};
}

/** Deletes a session, e.g. on logout. Safe to call with an unknown token. */
export async function invalidateSession(token: string): Promise<void> {
	await db.delete(session).where(eq(session.id, token));
}
