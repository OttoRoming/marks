import { describe, expect, it } from 'vitest';
import {
	badRequest,
	conflict,
	isAdmin,
	notFound,
	requireAdmin,
	requireUser,
	unauthorized
} from '$lib/server/api';
import type { SessionUser } from '$lib/server/session';

const user: SessionUser = { id: 'user-1', username: 'otto', is_admin: false };

describe('requireUser', () => {
	it('hands back the user stored on locals', () => {
		const result = requireUser({ user });

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.user).toBe(user);
		}
	});

	it('answers 401 Unauthorized when no user is signed in', async () => {
		const result = requireUser({ user: null });

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.response.status).toBe(401);
			expect(await result.response.json()).toEqual({ error: 'Unauthorized' });
		}
	});

	it('answers 401 for an expired session, which hooks.server.ts has already cleared', async () => {
		// `handle` deletes the cookie and leaves `locals.user` null when the token is unknown
		// or expired, so the two cases reach handlers identically.
		const result = requireUser({ user: null });

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.response.status).toBe(401);
		}
	});
});

describe('error responses', () => {
	it('answers 400 with the message it was given', async () => {
		const response = badRequest('Invalid JSON body');

		expect(response.status).toBe(400);
		expect(await response.json()).toEqual({ error: 'Invalid JSON body' });
	});

	it('defaults the 401 message to Unauthorized', async () => {
		const response = unauthorized();

		expect(response.status).toBe(401);
		expect(await response.json()).toEqual({ error: 'Unauthorized' });
	});

	it('lets a 401 carry its own message, as a rejected login does', async () => {
		const response = unauthorized('Incorrect username or password');

		expect(response.status).toBe(401);
		expect(await response.json()).toEqual({ error: 'Incorrect username or password' });
	});

	it('answers 404 with the message it was given', async () => {
		const response = notFound('Mark not found');

		expect(response.status).toBe(404);
		expect(await response.json()).toEqual({ error: 'Mark not found' });
	});

	it('answers 409 without a fieldErrors key when no field detail applies', async () => {
		const response = conflict('Something clashes');

		expect(response.status).toBe(409);
		expect(await response.json()).toEqual({ error: 'Something clashes' });
	});

	it('answers 409 with per-field messages, as a taken username does', async () => {
		const message = 'Username is already taken';

		const response = conflict(message, { username: [message] });

		expect(response.status).toBe(409);
		expect(await response.json()).toEqual({
			error: message,
			fieldErrors: { username: [message] }
		});
	});

	it('always answers JSON, so the client never has to guess at the body', async () => {
		for (const response of [badRequest('x'), unauthorized(), notFound('x'), conflict('x')]) {
			expect(response.headers.get('content-type')).toContain('application/json');
		}
	});
});

describe('requireAdmin', () => {
	const admin: SessionUser = { id: 'user-2', username: 'root', is_admin: true };

	it('hands back the user when they are an admin', () => {
		const result = requireAdmin({ user: admin });

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.user).toBe(admin);
		}
	});

	it('answers 401 for a request with no session, like every other authenticated route', async () => {
		const result = requireAdmin({ user: null });

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.response.status).toBe(401);
			expect(await result.response.json()).toEqual({ error: 'Unauthorized' });
		}
	});

	it('answers 404, not 403, to an account that is not an admin', async () => {
		// The admin pages are not something such a user is being kept out of; they are something
		// that is not there for them — which is also what a page that never existed answers, and
		// what someone poking at urls should be told either way.
		const result = requireAdmin({ user });

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.response.status).toBe(404);
			expect(await result.response.json()).toEqual({ error: 'Not found' });
		}
	});

	it('agrees with isAdmin about who may administer', () => {
		expect(isAdmin(admin)).toBe(true);
		expect(isAdmin(user)).toBe(false);
		expect(isAdmin(null)).toBe(false);
	});
});
