import { describe, expect, it } from 'vitest';
import { signupSchema } from '$lib/schemas/auth';
import { parseJsonBody } from '$lib/server/validation';

function post(body: string | undefined, headers: Record<string, string> = {}) {
	return new Request('http://localhost/api/test', {
		method: 'POST',
		headers: { 'content-type': 'application/json', ...headers },
		body
	});
}

describe('parseJsonBody', () => {
	it('returns the parsed and transformed data on success', async () => {
		const result = await parseJsonBody(
			post(JSON.stringify({ username: '  otto  ', password: 'supersecret' })),
			signupSchema
		);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.data).toEqual({ username: 'otto', password: 'supersecret' });
		}
	});

	it('answers 400 Invalid JSON body for malformed JSON', async () => {
		const result = await parseJsonBody(post('{oops'), signupSchema);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.response.status).toBe(400);
			expect(await result.response.json()).toEqual({ error: 'Invalid JSON body' });
		}
	});

	it('answers 400 Invalid JSON body for an absent body', async () => {
		const result = await parseJsonBody(post(undefined), signupSchema);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.response.status).toBe(400);
			expect(await result.response.json()).toEqual({ error: 'Invalid JSON body' });
		}
	});

	it('answers 400 with the validation messages when the shape is wrong', async () => {
		const result = await parseJsonBody(
			post(JSON.stringify({ username: 'ab', password: 'short' })),
			signupSchema
		);

		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.response.status).toBe(400);
			const body = (await result.response.json()) as { error: string };
			expect(body.error).toContain('Username must be at least 3 characters');
			expect(body.error).toContain('Password must be at least 8 characters');
		}
	});

	it('answers 400 for JSON that is not an object', async () => {
		for (const payload of ['"hello"', '42', 'null', '[1,2,3]']) {
			const result = await parseJsonBody(post(payload), signupSchema);
			expect(result.success).toBe(false);
			if (!result.success) {
				expect(result.response.status).toBe(400);
			}
		}
	});

	it('strips unknown keys rather than passing them through', async () => {
		const result = await parseJsonBody(
			post(JSON.stringify({ username: 'otto', password: 'supersecret', is_admin: true })),
			signupSchema
		);

		expect(result.success).toBe(true);
		if (result.success) {
			expect(result.data).not.toHaveProperty('is_admin');
		}
	});

	it('parses the body regardless of the request Content-Type', async () => {
		// request.json() reads the body without consulting the header, so a missing or
		// wrong Content-Type still parses. Documented so the behaviour is intentional.
		const result = await parseJsonBody(
			post(JSON.stringify({ username: 'otto', password: 'supersecret' }), {
				'content-type': 'text/plain'
			}),
			signupSchema
		);

		expect(result.success).toBe(true);
	});
});
