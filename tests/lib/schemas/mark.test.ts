import { describe, expect, it } from 'vitest';
import type { z } from 'zod';
import { markCreateSchema, markUpdateSchema } from '$lib/schemas/mark';

/** Collects the zod issue messages for a failing parse, or [] when it succeeds. */
function issues(schema: z.ZodType, value: unknown): string[] {
	const result = schema.safeParse(value);
	return result.success ? [] : result.error.issues.map((issue) => issue.message);
}

describe('markCreateSchema', () => {
	it('accepts a name and content', () => {
		const result = markCreateSchema.safeParse({ name: 'GitHub', content: 'https://github.com' });

		expect(result.success).toBe(true);
		expect(result.data).toEqual({ name: 'GitHub', content: 'https://github.com' });
	});

	it('trims both fields', () => {
		const result = markCreateSchema.safeParse({
			name: '  GitHub  ',
			content: '  https://github.com  '
		});

		expect(result.success).toBe(true);
		expect(result.data).toEqual({ name: 'GitHub', content: 'https://github.com' });
	});

	it('requires a non-blank name', () => {
		expect(issues(markCreateSchema, { name: '', content: 'x' })).toContain('Name is required');
		expect(issues(markCreateSchema, { name: '   ', content: 'x' })).toContain('Name is required');
		expect(issues(markCreateSchema, { content: 'x' })).toContain('Name is required');
	});

	it('requires non-blank content, which may be any text rather than a URL', () => {
		expect(issues(markCreateSchema, { name: 'x', content: '' })).toContain('Content is required');
		expect(issues(markCreateSchema, { name: 'x' })).toContain('Content is required');
		// A plain note with no URL is valid input; the favicon step is what needs a URL.
		expect(markCreateSchema.safeParse({ name: 'Note', content: 'just some text' }).success).toBe(
			true
		);
	});

	it('enforces the length limits after trimming', () => {
		expect(issues(markCreateSchema, { name: 'a'.repeat(201), content: 'x' })).toContain(
			'Name must be at most 200 characters'
		);
		expect(issues(markCreateSchema, { name: 'x', content: 'a'.repeat(2001) })).toContain(
			'Content must be at most 2000 characters'
		);
		// Whitespace must not push a valid value over the limit.
		expect(
			markCreateSchema.safeParse({ name: `  ${'a'.repeat(200)}  `, content: 'x' }).success
		).toBe(true);
	});

	it('rejects non-string types', () => {
		expect(markCreateSchema.safeParse({ name: 42, content: 'x' }).success).toBe(false);
		expect(markCreateSchema.safeParse({ name: 'x', content: null }).success).toBe(false);
		expect(markCreateSchema.safeParse({ name: 'x', content: { $ne: null } }).success).toBe(false);
	});

	it('strips unknown keys', () => {
		const result = markCreateSchema.safeParse({
			name: 'x',
			content: 'y',
			user_id: 'someone-elses-id',
			icon_id: 'forged'
		});

		expect(result.success).toBe(true);
		expect(result.data).toEqual({ name: 'x', content: 'y' });
	});
});

describe('markUpdateSchema', () => {
	it('accepts a name-only update', () => {
		expect(markUpdateSchema.safeParse({ name: 'renamed' }).success).toBe(true);
	});

	it('accepts a content-only update', () => {
		expect(markUpdateSchema.safeParse({ content: 'https://example.com' }).success).toBe(true);
	});

	it('accepts both fields', () => {
		expect(markUpdateSchema.safeParse({ name: 'a', content: 'b' }).success).toBe(true);
	});

	it('rejects an empty update', () => {
		expect(issues(markUpdateSchema, {})).toContain('Provide at least one of name or content');
	});

	it('rejects an update of only unknown keys', () => {
		// Unknown keys are stripped before the refine runs, so this is an empty update.
		expect(issues(markUpdateSchema, { colour: 'red' })).toContain(
			'Provide at least one of name or content'
		);
		expect(issues(markUpdateSchema, { user_id: 'someone-elses-id' })).toContain(
			'Provide at least one of name or content'
		);
	});

	it('still validates the fields that are present', () => {
		expect(issues(markUpdateSchema, { name: '   ' })).toContain('Name is required');
		expect(issues(markUpdateSchema, { content: '' })).toContain('Content is required');
		expect(issues(markUpdateSchema, { name: 'a'.repeat(201) })).toContain(
			'Name must be at most 200 characters'
		);
	});

	it('leaves omitted fields undefined so the route can fall back to the stored value', () => {
		const result = markUpdateSchema.safeParse({ name: 'renamed' });

		expect(result.success).toBe(true);
		expect(result.data?.content).toBeUndefined();
	});
});
