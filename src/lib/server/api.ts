import { json } from '@sveltejs/kit';
import type { SessionUser } from './session';

/**
 * Per-field messages for a form failure, keyed by field name.
 *
 * Only signup uses them today; they exist so a client can mark the offending input instead
 * of parsing the sentence in `error`.
 */
export type FieldErrors = Record<string, string[]>;

/**
 * Builds the `{ error }` body that every failure in this API shares.
 *
 * The status and the message are one contract, which is why they are written together here
 * rather than at each call site: the Rust client prints `error` verbatim and falls back to a
 * bare status line only when the body cannot be read (see `error_message` in
 * `client/src/api.rs`), so a route that returns a status without a message degrades the UI.
 */
function errorResponse(status: number, error: string, fieldErrors?: FieldErrors): Response {
	// `fieldErrors` is left out rather than sent empty, so a client can tell a failure that
	// carries no field detail from one that carries detail about no field.
	return json(fieldErrors ? { error, fieldErrors } : { error }, { status });
}

/** 400 for a body that is absent, unparseable, or does not match its schema. */
export function badRequest(error: string): Response {
	return errorResponse(400, error);
}

/**
 * 401 for a request with no usable session, which is the default wording.
 *
 * Login passes its own message instead: a rejected credential is an ordinary outcome rather
 * than an expired session. Both are still 401s, and the client tells them apart by which call
 * it made (see `Api::authenticate` in `client/src/api.rs`) rather than by the body.
 */
export function unauthorized(error = 'Unauthorized'): Response {
	return errorResponse(401, error);
}

/** 404 for a resource that does not exist, or does not belong to the caller. */
export function notFound(error: string): Response {
	return errorResponse(404, error);
}

/** 409 for a request that is well-formed but clashes with what is already stored. */
export function conflict(error: string, fieldErrors?: FieldErrors): Response {
	return errorResponse(409, error, fieldErrors);
}

export type RequireUserResult =
	{ success: true; user: SessionUser } | { success: false; response: Response };

/**
 * The signed-in user, or the 401 to answer with — the guard every authenticated route opens
 * with, shaped like `parseJsonBody` so the two checks a handler makes read the same way:
 *
 * ```ts
 * const auth = requireUser(locals);
 * if (!auth.success) {
 * 	return auth.response;
 * }
 * ```
 *
 * `locals.user` is filled in by `src/hooks.server.ts`, which resolves the session cookie once
 * per request; a null user therefore means the request arrived with no cookie, or with one
 * that was unknown or expired.
 *
 * The parameter is typed structurally rather than as `App.Locals` so this module stays free
 * of generated SvelteKit types and can be unit tested on its own.
 */
export function requireUser(locals: { user: SessionUser | null }): RequireUserResult {
	const { user } = locals;

	if (!user) {
		return { success: false, response: unauthorized() };
	}

	return { success: true, user };
}

/**
 * Whether this account may use the admin pages and the admin API.
 *
 * One place decides that, so the pages (see `src/routes/admin/+layout.server.ts`) and the routes
 * cannot come to different conclusions about the same account.
 */
export function isAdmin(user: SessionUser | null): boolean {
	return user?.is_admin === true;
}

export type RequireAdminResult = RequireUserResult;

/**
 * The signed-in user, when they are an admin — or the response to answer with, shaped like
 * [`requireUser`] so an admin route opens the way every other route does:
 *
 * ```ts
 * const auth = requireAdmin(locals);
 * if (!auth.success) {
 * 	return auth.response;
 * }
 * ```
 *
 * An account that is not an admin is answered with **404**, not 403: the admin pages are not
 * something such a user is being kept out of, they are something that is not there for them — the
 * same reason `getOwnMark` reports someone else's mark as missing rather than as forbidden. A page
 * that was renamed, or never existed, then answers identically.
 */
export function requireAdmin(locals: { user: SessionUser | null }): RequireAdminResult {
	const auth = requireUser(locals);
	if (!auth.success) {
		return auth;
	}

	if (!isAdmin(auth.user)) {
		return { success: false, response: notFound('Not found') };
	}

	return auth;
}
