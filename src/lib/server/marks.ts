import { and, eq } from 'drizzle-orm';
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

/** Inserts an `icon` row for a fetched favicon and returns its id. */
export async function createIconRow(favicon: Favicon): Promise<string> {
	const [row] = await db
		.insert(icon)
		.values({ type: 'favicon', url: favicon.url, content: favicon.content })
		.returning({ id: icon.id });

	return row.id;
}

/**
 * Deletes an icon row that is no longer referenced by its mark.
 *
 * Icon rows are never shared between marks, so once a mark is deleted or points at a
 * freshly fetched icon, the previous row is garbage.
 */
export async function deleteIconRow(icon_id: string | null): Promise<void> {
	if (!icon_id) {
		return;
	}

	await db.delete(icon).where(eq(icon.id, icon_id));
}
