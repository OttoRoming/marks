import { dev } from '$app/environment';
import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { user, session } from '$lib/server/db/schema';
import { db } from '$lib/server/db';
import { unauthorized } from '$lib/server/api';
import { parseJsonBody } from '$lib/server/validation';
import { verifyPassword } from '$lib/server/password';
import { signupSchema } from '$lib/schemas/auth';
import { SESSION_COOKIE_NAME } from '$lib/server/session';

import { eq } from 'drizzle-orm';

export const POST: RequestHandler = async ({ request, cookies }) => {
	const parsed = await parseJsonBody(request, signupSchema);
	if (!parsed.success) {
		return parsed.response;
	}
	const data = parsed.data;

	const [selected_user] = await db
		.select({ id: user.id, password: user.password })
		.from(user)
		.where(eq(user.username, data.username));

	// An unknown username and a wrong password answer identically, so the response cannot be
	// used to find out which usernames exist.
	const failed_login_response = unauthorized('Incorrect username or password');

	if (selected_user === undefined) {
		return failed_login_response;
	}

	if (!(await verifyPassword(selected_user.password, data.password))) {
		return failed_login_response;
	}

	const [new_session] = await db.insert(session).values({ user_id: selected_user.id }).returning();

	cookies.set(SESSION_COOKIE_NAME, new_session.id, {
		path: '/',
		httpOnly: true,
		sameSite: 'lax',
		secure: !dev,
		expires: new_session.expires_at
	});

	return json({ session: new_session }, { status: 200 });
};
