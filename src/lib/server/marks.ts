import { and, eq, notExists } from 'drizzle-orm';
import { db } from './db';
import { icon, mark } from './db/schema';
import type { Favicon } from './favicon';

/**
 * The columns safe to serialise to a client. `icon` holds a blob and is served
 * separately from `/api/marks/[id]/icon`, so it is never selected here.
 */
export const markSelection = {
	id: mark.id,
	name: mark.name,
	content: mark.content,
	icon_id: mark.icon_id
};

/**
 * Looks up a mark by id, scoped to its owner.
 *
 * Returns null both when the mark does not exist and when it belongs to someone else,
 * so callers cannot distinguish the two and leak another user's mark ids.
 */
export async function getOwnMark(mark_id: string, user_id: string) {
	const [row] = await db
		.select()
		.from(mark)
		.where(and(eq(mark.id, mark_id), eq(mark.user_id, user_id)));

	return row ?? null;
}

/** Looks up the single `icon` row stored for `url`, or null when there is none. */
async function findIconId(url: string): Promise<string | null> {
	const [row] = await db.select({ id: icon.id }).from(icon).where(eq(icon.url, url));

	return row?.id ?? null;
}

/**
 * Returns the id of the `icon` row holding `favicon`, storing it the first time its url is seen.
 *
 * `icon.url` is unique and is derived from the mark's hostname alone, so every mark on one
 * hostname resolves to the same row: the second mark reuses the bytes already stored rather
 * than duplicating them or tripping the uniqueness constraint.
 */
export async function getOrCreateIconRow(favicon: Favicon): Promise<string> {
	const existing = await findIconId(favicon.url);
	if (existing) {
		return existing;
	}

	// `onConflictDoNothing` covers what the lookup above cannot: a concurrent request that
	// stored the same url in between leaves this insert empty-handed instead of failing.
	const [inserted] = await db
		.insert(icon)
		.values({ type: 'favicon', url: favicon.url, content: favicon.content })
		.onConflictDoNothing({ target: icon.url })
		.returning({ id: icon.id });

	if (inserted) {
		return inserted.id;
	}

	const winner = await findIconId(favicon.url);
	if (!winner) {
		throw new Error(`Icon row for ${favicon.url} disappeared while storing it`);
	}

	return winner;
}

/**
 * Deletes an icon row once no mark references it.
 *
 * Rows are shared by url, so a mark letting go of one does not make it garbage on its own:
 * another mark — or the very mark that just moved to a different hostname — may still point
 * at it, and deleting it there would blank out that mark's icon through the
 * `onDelete: 'set null'` foreign key.
 */
export async function deleteIconRowIfUnreferenced(icon_id: string | null): Promise<void> {
	if (!icon_id) {
		return;
	}

	// Checked in the same statement, so the row cannot gain a referencing mark in between.
	await db
		.delete(icon)
		.where(
			and(eq(icon.id, icon_id), notExists(db.select().from(mark).where(eq(mark.icon_id, icon_id))))
		);
}
