import { json } from '@sveltejs/kit';
import { asc, eq } from 'drizzle-orm';
import type { RequestHandler } from './$types';
import { mark } from '$lib/server/db/schema';
import { db } from '$lib/server/db';
import { requireUser } from '$lib/server/api';
import { parseJsonBody } from '$lib/server/validation';
import { fetchFavicon } from '$lib/server/favicon';
import { getOrCreateIconRow, markSelection } from '$lib/server/marks';
import { markCreateSchema } from '$lib/schemas/mark';

/** Lists the signed-in user's marks. No icon bytes: those come from /api/marks/[id]/icon. */
export const GET: RequestHandler = async ({ locals }) => {
	const auth = requireUser(locals);
	if (!auth.success) {
		return auth.response;
	}

	// There is no created_at column, so the order is alphabetical rather than chronological.
	const marks = await db
		.select(markSelection)
		.from(mark)
		.where(eq(mark.user_id, auth.user.id))
		.orderBy(asc(mark.name));

	return json({ marks });
};

export const POST: RequestHandler = async ({ request, locals }) => {
	const auth = requireUser(locals);
	if (!auth.success) {
		return auth.response;
	}

	const parsed = await parseJsonBody(request, markCreateSchema);
	if (!parsed.success) {
		return parsed.response;
	}

	const { name, content } = parsed.data;

	// A favicon is a bonus: a null result still creates the mark, just without an icon.
	// Marks on a hostname already stored share that hostname's icon row.
	const favicon = await fetchFavicon(content);
	const icon_id = favicon ? await getOrCreateIconRow(favicon) : null;

	const [new_mark] = await db
		.insert(mark)
		.values({ user_id: auth.user.id, icon_id, name, content })
		.returning(markSelection);

	return json({ mark: new_mark }, { status: 201 });
};
