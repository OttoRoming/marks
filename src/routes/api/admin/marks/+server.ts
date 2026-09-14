import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { requireAdmin } from '$lib/server/api';
import { listMarks } from '$lib/server/admin';

/** Every mark on the server, with the account that owns it. */
export const GET: RequestHandler = async ({ locals }) => {
	const auth = requireAdmin(locals);
	if (!auth.success) {
		return auth.response;
	}

	return json({ marks: await listMarks() });
};
