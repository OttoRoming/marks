import { z } from 'zod';

/** `mark.name` doubles as the bookmark title; `mark.content` is what a favicon is derived from. */
export const markCreateSchema = z.object({
	name: z
		// The `error` option covers a missing key or wrong type, so an omitted field does
		// not surface zod's internal "expected string, received undefined" wording.
		.string({ error: 'Name is required' })
		.trim()
		.min(1, 'Name is required')
		.max(200, 'Name must be at most 200 characters'),
	content: z
		.string({ error: 'Content is required' })
		.trim()
		.min(1, 'Content is required')
		.max(2000, 'Content must be at most 2000 characters')
});

export const markUpdateSchema = markCreateSchema
	.partial()
	.refine((value) => value.name !== undefined || value.content !== undefined, {
		message: 'Provide at least one of name or content'
	});

export type MarkCreateInput = z.infer<typeof markCreateSchema>;
export type MarkUpdateInput = z.infer<typeof markUpdateSchema>;
