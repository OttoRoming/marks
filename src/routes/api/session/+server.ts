import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { unauthorized } from '$lib/server/api';

/**
 * Who this request is, as far as the server knows.
 *
 * The one route the web pages open with: a browser holds its session in an HttpOnly cookie, so
 * JavaScript cannot read it to find out whether it is signed in, and has no other way to ask. A
 * page therefore asks this, and is told either who it is talking to or that nobody is signed in —
 * which is what lets it show a sign-in form instead of an empty page.
 *
 * `locals.user` is filled in by `src/hooks.server.ts` from that cookie, and already carries only
 * the fields an account is willing to publish (see `userSelection`): the same three the admin pages
 * list, so nothing here is a wider view of an account than the account itself.
 */
export const GET: RequestHandler = async ({ locals }) => {
	const { user } = locals;

	if (!user) {
		return unauthorized();
	}

	return json({ user });
};
