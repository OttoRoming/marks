import { z } from 'zod';

export const signupSchema = z.object({
	username: z
		// `error` covers a missing key or wrong type, so an omitted field does not surface
		// zod's internal "expected string, received undefined" wording.
		.string({ error: 'Username is required' })
		.trim()
		.min(3, 'Username must be at least 3 characters')
		.max(32, 'Username must be at most 32 characters'),
	password: z
		.string({ error: 'Password is required' })
		.min(8, 'Password must be at least 8 characters')
});

export type SignupInput = z.infer<typeof signupSchema>;
