import { dev } from '$app/environment';
import { json } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import type { RequestHandler } from './$types';
import { user, session } from '$lib/server/db/schema';
import { db } from '$lib/server/db';
import { conflict } from '$lib/server/api';
import { parseJsonBody } from '$lib/server/validation';
import { hashPassword } from '$lib/server/password';
import { signupSchema } from '$lib/schemas/auth';
import { SESSION_COOKIE_NAME } from '$lib/server/session';

export const POST: RequestHandler = async ({ request, cookies }) => {
	const parsed = await parseJsonBody(request, signupSchema);
	if (!parsed.success) {
		return parsed.response;
	}
	const data = parsed.data;

	const username_taken = (await db.$count(user, eq(user.username, data.username))) > 0;
	if (username_taken) {
		// One message serves both halves: the client shows `error` at large, and uses
		// `fieldErrors` to mark the username input itself.
		const message = 'Username is already taken';

		return conflict(message, { username: [message] });
	}

	// The first user should always be admin
	const user_count = await db.$count(user);
	const is_admin = user_count === 0;

	const password_hash = await hashPassword(data.password);

	const [{ user_id }] = await db
		.insert(user)
		.values({ username: data.username, password: password_hash, is_admin })
		.returning({ user_id: user.id });

	const [new_session] = await db.insert(session).values({ user_id }).returning();

	cookies.set(SESSION_COOKIE_NAME, new_session.id, {
		path: '/',
		httpOnly: true,
		sameSite: 'lax',
		secure: !dev,
		expires: new_session.expires_at
	});

	return json({ session: new_session }, { status: 201 });
};
