import { z } from 'zod';

export const signupSchema = z.object({
	username: z
		.string()
		.trim()
		.min(3, 'Username must be at least 3 characters')
		.max(32, 'Username must be at most 32 characters'),
	password: z.string().min(8, 'Password must be at least 8 characters')
});

export type SignupInput = z.infer<typeof signupSchema>;
