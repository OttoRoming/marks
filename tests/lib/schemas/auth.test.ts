import { describe, expect, it } from 'vitest';
import type { z } from 'zod';
import { signupSchema } from '$lib/schemas/auth';

/** Collects the zod issue messages for a failing parse, or [] when it succeeds. */
function issues(schema: z.ZodType, value: unknown): string[] {
	const result = schema.safeParse(value);
	return result.success ? [] : result.error.issues.map((issue) => issue.message);
}

describe('signupSchema', () => {
	it('accepts a username and password', () => {
		const result = signupSchema.safeParse({ username: 'otto', password: 'supersecret' });

		expect(result.success).toBe(true);
		expect(result.data).toEqual({ username: 'otto', password: 'supersecret' });
	});

	it('trims the username but leaves the password untouched', () => {
		const result = signupSchema.safeParse({ username: '  otto  ', password: '  spaces  ' });

		expect(result.success).toBe(true);
		expect(result.data).toEqual({ username: 'otto', password: '  spaces  ' });
	});

	it('enforces the username length limits after trimming', () => {
		expect(issues(signupSchema, { username: 'ab', password: 'supersecret' })).toContain(
			'Username must be at least 3 characters'
		);
		expect(issues(signupSchema, { username: 'a'.repeat(33), password: 'supersecret' })).toContain(
			'Username must be at most 32 characters'
		);
		// Padding must not let a too-short username through.
		expect(issues(signupSchema, { username: '  ab  ', password: 'supersecret' })).toContain(
			'Username must be at least 3 characters'
		);
	});

	it('requires a password of at least 8 characters', () => {
		expect(issues(signupSchema, { username: 'otto', password: 'short' })).toContain(
			'Password must be at least 8 characters'
		);
		expect(signupSchema.safeParse({ username: 'otto', password: '12345678' }).success).toBe(true);
	});

	it('rejects missing or wrong-typed fields', () => {
		expect(signupSchema.safeParse({}).success).toBe(false);
		expect(signupSchema.safeParse({ username: 'otto' }).success).toBe(false);
		expect(signupSchema.safeParse({ username: 42, password: 'supersecret' }).success).toBe(false);
		expect(signupSchema.safeParse({ username: 'otto', password: null }).success).toBe(false);
	});

	it('reports a friendly message when a field is omitted entirely', () => {
		// Without the `error` option these would leak zod internals such as
		// "Invalid input: expected string, received undefined" to the API client.
		expect(issues(signupSchema, {})).toEqual(['Username is required', 'Password is required']);
		expect(issues(signupSchema, { username: 'otto' })).toContain('Password is required');
		expect(issues(signupSchema, { password: 'supersecret' })).toContain('Username is required');
	});

	it('strips unknown keys, so a client cannot self-assign admin', () => {
		const result = signupSchema.safeParse({
			username: 'otto',
			password: 'supersecret',
			is_admin: true,
			id: 'forged'
		});

		expect(result.success).toBe(true);
		expect(result.data).toEqual({ username: 'otto', password: 'supersecret' });
	});
});
