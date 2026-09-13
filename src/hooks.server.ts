import type { Handle } from '@sveltejs/kit';
import { SESSION_COOKIE_NAME, validateSessionToken } from '$lib/server/session';

/**
 * Resolves the session cookie once per request and exposes the result on `locals`, so
 * route handlers only need to check `locals.user` rather than re-validating the token.
 */
export const handle: Handle = async ({ event, resolve }) => {
	const token = event.cookies.get(SESSION_COOKIE_NAME);

	if (token) {
		const validated = await validateSessionToken(token);

		if (validated) {
			event.locals.user = validated.user;
			event.locals.session = validated.session;
		} else {
			// Expired or unrecognised: drop it so the browser stops sending it.
			event.cookies.delete(SESSION_COOKIE_NAME, { path: '/' });
		}
	}

	return resolve(event);
};
