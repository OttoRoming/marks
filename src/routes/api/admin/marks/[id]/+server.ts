import type { RequestHandler } from './$types';
import { notFound, requireAdmin } from '$lib/server/api';
import { deleteMark } from '$lib/server/admin';

/**
 * Deletes any mark, whoever owns it.
 *
 * The same 204 as the owner's own delete (see `src/routes/api/marks/[id]/+server.ts`), so a client
 * does not have to know which of the two it called to know what success looks like.
 */
export const DELETE: RequestHandler = async ({ params, locals }) => {
	const auth = requireAdmin(locals);
	if (!auth.success) {
		return auth.response;
	}

	if (!(await deleteMark(params.id))) {
		return notFound('Mark not found');
	}

	return new Response(null, { status: 204 });
};
