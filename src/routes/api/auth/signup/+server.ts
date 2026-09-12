import { json } from '@sveltejs/kit';
import { eq } from 'drizzle-orm';
import type { RequestHandler } from './$types';
import { user } from '$lib/server/db/schema';
import { db } from '$lib/server/db';
import { parseJsonBody } from '$lib/server/validation';
import { hashPassword } from '$lib/server/password';
import { signupSchema } from '$lib/schemas/auth';

export const POST: RequestHandler = async ({ request }) => {
	const parsed = await parseJsonBody(request, signupSchema);
	if (!parsed.success) {
		return parsed.response;
	}

	const { username, password } = parsed.data;

	const username_taken = (await db.$count(user, eq(user.username, username))) > 0;
	if (username_taken) {
		return json(
			{
				error: 'Username is already taken',
				fieldErrors: { username: ['Username is already taken'] }
			},
			{ status: 409 }
		);
	}

	// The first user should always be admin
	const user_count = await db.$count(user);
	const is_admin = user_count === 0;

	const password_hash = await hashPassword(password);

	const new_user = await db
		.insert(user)
		.values({ username, password: password_hash, is_admin })
		.returning();

	return json({ user: new_user }, { status: 201 });
};
