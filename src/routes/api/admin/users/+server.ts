import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { requireAdmin } from '$lib/server/api';
import { counts, listUsers } from '$lib/server/admin';

/**
 * Every account, and how much of everything there is.
 *
 * The counts come with the listing rather than from a route of their own because they are what the
 * same page shows above it: one request, one answer, and no window in which the two disagree.
 */
export const GET: RequestHandler = async ({ locals }) => {
	const auth = requireAdmin(locals);
	if (!auth.success) {
		return auth.response;
	}

	const [users, totals] = await Promise.all([listUsers(), counts()]);

	return json({ users, counts: totals });
};
