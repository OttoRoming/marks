import { describe, expect, it } from 'vitest';
import { userUpdateSchema } from '$lib/schemas/admin';

describe('userUpdateSchema', () => {
	it('accepts the one thing an admin may change', () => {
		expect(userUpdateSchema.parse({ is_admin: true })).toEqual({ is_admin: true });
		expect(userUpdateSchema.parse({ is_admin: false })).toEqual({ is_admin: false });
	});

	it('refuses a flag that is not a flag', () => {
		expect(() => userUpdateSchema.parse({ is_admin: 'yes' })).toThrow();
		expect(() => userUpdateSchema.parse({ is_admin: 1 })).toThrow();
	});

	it('refuses a change that changes nothing', () => {
		// Nothing else is accepted, so a body without the flag has nothing in it at all.
		expect(() => userUpdateSchema.parse({})).toThrow();
		expect(() => userUpdateSchema.parse({ username: 'someone-else' })).toThrow();
	});
});
