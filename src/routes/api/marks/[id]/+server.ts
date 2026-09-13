import { json } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import type { RequestHandler } from './$types';
import { mark } from '$lib/server/db/schema';
import { db } from '$lib/server/db';
import { parseJsonBody } from '$lib/server/validation';
import { fetchFavicon } from '$lib/server/favicon';
import {
	deleteIconRowIfUnreferenced,
	getOrCreateIconRow,
	getOwnMark,
	markSelection
} from '$lib/server/marks';
import { markUpdateSchema } from '$lib/schemas/mark';

function not_found() {
	return json({ error: 'Mark not found' }, { status: 404 });
}

export const GET: RequestHandler = async ({ params, locals }) => {
	const user = locals.user;
	if (!user) {
		return json({ error: 'Unauthorized' }, { status: 401 });
	}

	const existing = await getOwnMark(params.id, user.id);
	if (!existing) {
		return not_found();
	}

	const { id, name, content, icon_id } = existing;
	return json({ mark: { id, name, content, icon_id } });
};

export const PATCH: RequestHandler = async ({ params, request, locals }) => {
	const user = locals.user;
	if (!user) {
		return json({ error: 'Unauthorized' }, { status: 401 });
	}

	const existing = await getOwnMark(params.id, user.id);
	if (!existing) {
		return not_found();
	}

	const parsed = await parseJsonBody(request, markUpdateSchema);
	if (!parsed.success) {
		return parsed.response;
	}

	const { name, content } = parsed.data;
	const content_changed = content !== undefined && content !== existing.content;

	// Only re-derive the icon when the content it was derived from actually changed,
	// so renaming a mark does not refetch (or drop) its favicon.
	const favicon = content_changed ? await fetchFavicon(content) : null;
	const icon_id = content_changed
		? favicon
			? await getOrCreateIconRow(favicon)
			: null
		: existing.icon_id;

	const [updated] = await db
		.update(mark)
		.set({
			name: name ?? existing.name,
			content: content ?? existing.content,
			icon_id
		})
		.where(eq(mark.id, existing.id))
		.returning(markSelection);

	// The previous icon row is left alone when the new content still resolves to it, or when
	// another mark shares it; it is dropped only once nothing references it any more.
	if (content_changed) {
		await deleteIconRowIfUnreferenced(existing.icon_id);
	}

	return json({ mark: updated });
};

export const DELETE: RequestHandler = async ({ params, locals }) => {
	const user = locals.user;
	if (!user) {
		return json({ error: 'Unauthorized' }, { status: 401 });
	}

	const existing = await getOwnMark(params.id, user.id);
	if (!existing) {
		return not_found();
	}

	await db.delete(mark).where(eq(mark.id, existing.id));

	// Deleting a mark does not touch `icon` (the FK cascade runs the other way), so the row
	// would otherwise be orphaned — unless another mark still shares it.
	await deleteIconRowIfUnreferenced(existing.icon_id);

	return new Response(null, { status: 204 });
};
