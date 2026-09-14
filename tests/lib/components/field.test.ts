// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { mount } from 'svelte';
import Field from '$lib/components/Field.svelte';

beforeEach(() => {
	document.body.innerHTML = '';
});

function render(props: Record<string, unknown> = {}) {
	const target = document.createElement('div');
	document.body.append(target);

	mount(Field, { target, props: { label: 'Filter', ...props } });

	const input = target.querySelector('input');
	if (!input) {
		throw new Error('no input was rendered');
	}

	return { target, input, label: target.querySelector('label') };
}

describe('Field', () => {
	it('is a real input', () => {
		expect(render().input.tagName).toBe('INPUT');
	});

	it('points its label at its input, which is what makes the label a label', () => {
		const { input, label } = render();

		expect(label?.textContent?.trim()).toBe('Filter');
		expect(label?.htmlFor).toBeTruthy();
		expect(label?.htmlFor).toBe(input.id);
	});

	it('describes its input with the description it was given', () => {
		const { target, input } = render({ description: 'Matches anywhere in the name.' });
		const described = target.querySelector(
			`#${CSS.escape(input.getAttribute('aria-describedby') ?? '')}`
		);

		expect(described?.textContent).toBe('Matches anywhere in the name.');
	});

	it('says nothing about a description it does not have', () => {
		// An `aria-describedby` pointing at nothing is read as nothing by some screen readers and as
		// a mistake by others, so it is left off rather than left empty.
		expect(render().input.getAttribute('aria-describedby')).toBe(null);
		expect(render().input.getAttribute('aria-invalid')).toBe(null);
	});

	it('marks itself invalid and announces the error when there is one', () => {
		const { target, input } = render({
			description: 'Matches anywhere in the name.',
			error: 'That filter is not valid.'
		});

		expect(input.getAttribute('aria-invalid')).toBe('true');

		const error = target.querySelector('[role="alert"]');
		expect(error?.textContent).toBe('That filter is not valid.');

		// Both, and in the order they appear: a screen reader reads the description and then the
		// reason, rather than one of them standing in for the other.
		const ids = (input.getAttribute('aria-describedby') ?? '').split(' ');
		expect(ids).toHaveLength(2);
		expect(target.querySelector(`#${CSS.escape(ids[1])}`)?.textContent).toBe(
			'That filter is not valid.'
		);
	});

	it('passes the input type through, so a search field is a search field', () => {
		expect(render({ type: 'search' }).input.getAttribute('type')).toBe('search');
	});

	it('gives two fields their own ids, so neither describes the other', () => {
		const first = render({ description: 'One' });
		const second = render({ description: 'Two' });

		expect(first.input.id).not.toBe(second.input.id);
		expect(first.input.getAttribute('aria-describedby')).not.toBe(
			second.input.getAttribute('aria-describedby')
		);
	});
});
