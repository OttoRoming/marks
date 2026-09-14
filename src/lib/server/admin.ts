import { asc, count, eq } from 'drizzle-orm';
import { db } from './db';
import { mark, session, user } from './db/schema';
import { deleteIconRowIfUnreferenced, markSelection } from './marks';
import { userSelection } from './users';

/** Every account, alphabetically. */
export async function listUsers() {
	return db.select(userSelection).from(user).orderBy(asc(user.username));
}

/**
 * Every mark, with the account that owns it, by owner and then by name.
 *
 * No search argument: the admin page filters what it has with the same matcher the launcher uses,
 * so a query would be a second implementation of that, in SQL, able to disagree with the first.
 * An install with more marks than a page can reasonably hold would want pagination instead, which
 * is a different feature from a filter.
 */
export async function listMarks() {
	return db
		.select({
			...markSelection,
			user_id: mark.user_id,
			owner: user.username
		})
		.from(mark)
		.innerJoin(user, eq(user.id, mark.user_id))
		.orderBy(asc(user.username), asc(mark.name));
}

/** How much of everything there is, for the page that says so. */
export async function counts() {
	const [accounts] = await db.select({ total: count() }).from(user);
	const [admins] = await db.select({ total: count() }).from(user).where(eq(user.is_admin, true));
	const [marks] = await db.select({ total: count() }).from(mark);
	const [sessions] = await db.select({ total: count() }).from(session);

	return {
		users: accounts.total,
		admins: admins.total,
		marks: marks.total,
		sessions: sessions.total
	};
}

/**
 * Makes an account an admin, or takes that away, and returns it as it now is.
 *
 * Null when there is no such account, which the route answers 404 to — the same answer as an
 * account that is not there at all.
 */
export async function setUserAdmin(user_id: string, is_admin: boolean) {
	const [updated] = await db
		.update(user)
		.set({ is_admin })
		.where(eq(user.id, user_id))
		.returning(userSelection);

	return updated ?? null;
}

/** Whether an account is the only admin left, which is the one thing that must not happen. */
export async function isLastAdmin(user_id: string): Promise<boolean> {
	const [account] = await db
		.select({ is_admin: user.is_admin })
		.from(user)
		.where(eq(user.id, user_id));

	if (!account?.is_admin) {
		return false;
	}

	const [others] = await db.select({ total: count() }).from(user).where(eq(user.is_admin, true));

	return others.total <= 1;
}

/**
 * Deletes an account, and everything that belonged to it, through the schema's cascades.
 *
 * The marks and sessions go with the account on their own (`onDelete: 'cascade'`), but `icon`
 * hangs off a mark as `onDelete: 'set null'` and is shared by url, so the rows the account's
 * marks pointed at are cleaned up here — each one only if nothing else points at it, which is
 * what `deleteIconRowIfUnreferenced` decides. Collected before the delete, because afterwards
 * there is no mark left to ask.
 *
 * False when there is no such account.
 */
export async function deleteUser(user_id: string): Promise<boolean> {
	const owned = await db
		.select({ icon_id: mark.icon_id })
		.from(mark)
		.where(eq(mark.user_id, user_id));

	const [deleted] = await db.delete(user).where(eq(user.id, user_id)).returning({ id: user.id });
	if (!deleted) {
		return false;
	}

	for (const { icon_id } of owned) {
		await deleteIconRowIfUnreferenced(icon_id);
	}

	return true;
}

/** Deletes any mark, whoever owns it. False when there is no such mark. */
export async function deleteMark(mark_id: string): Promise<boolean> {
	const [existing] = await db
		.select({ id: mark.id, icon_id: mark.icon_id })
		.from(mark)
		.where(eq(mark.id, mark_id));

	if (!existing) {
		return false;
	}

	await db.delete(mark).where(eq(mark.id, existing.id));
	await deleteIconRowIfUnreferenced(existing.icon_id);

	return true;
}
