import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { badRequest, conflict, notFound, requireAdmin } from '$lib/server/api';
import { deleteUser, isLastAdmin, setUserAdmin } from '$lib/server/admin';
import { parseJsonBody } from '$lib/server/validation';
import { userUpdateSchema } from '$lib/schemas/admin';

/** Makes an account an admin, or takes that away. */
export const PATCH: RequestHandler = async ({ params, request, locals }) => {
	const auth = requireAdmin(locals);
	if (!auth.success) {
		return auth.response;
	}

	const parsed = await parseJsonBody(request, userUpdateSchema);
	if (!parsed.success) {
		return parsed.response;
	}

	const { is_admin } = parsed.data;

	// The two ways an admin could shut everyone out of the admin pages, refused here rather than
	// left to whoever reads the logs afterwards: taking their own flag away, and taking it from the
	// last account that has it.
	if (params.id === auth.user.id && !is_admin) {
		return badRequest('You cannot remove your own admin rights');
	}

	if (!is_admin && (await isLastAdmin(params.id))) {
		return conflict('The last admin cannot stop being an admin');
	}

	const updated = await setUserAdmin(params.id, is_admin);
	if (!updated) {
		return notFound('Account not found');
	}

	return json({ user: updated });
};

/** Deletes an account, and its marks and sessions with it. */
export const DELETE: RequestHandler = async ({ params, locals }) => {
	const auth = requireAdmin(locals);
	if (!auth.success) {
		return auth.response;
	}

	// Deleting yourself would end the session making the request, and the last admin going is the
	// same lock-out by another route.
	if (params.id === auth.user.id) {
		return badRequest('You cannot delete your own account');
	}

	if (await isLastAdmin(params.id)) {
		return conflict('The last admin cannot be deleted');
	}

	if (!(await deleteUser(params.id))) {
		return notFound('Account not found');
	}

	return new Response(null, { status: 204 });
};
