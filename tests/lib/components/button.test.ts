// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createRawSnippet, mount } from 'svelte';
import Button from '$lib/components/Button.svelte';

beforeEach(() => {
	document.body.innerHTML = '';
});

function render(props: Record<string, unknown> = {}) {
	const target = document.createElement('div');
	document.body.append(target);

	mount(Button, {
		target,
		props: { children: createRawSnippet(() => ({ render: () => 'Make admin' })), ...props }
	});

	const button = target.querySelector('button');
	if (!button) {
		throw new Error('no button was rendered');
	}

	return button;
}

describe('Button', () => {
	it('is a real button, so it is reachable and activatable the way buttons are', () => {
		expect(render().tagName).toBe('BUTTON');
	});

	it('does not submit the form it may be in unless it says so', () => {
		// The element's own default is `submit`, which is wrong for a page of actions that are not
		// forms: a button that submits is a button that does something other than what it says.
		expect(render().getAttribute('type')).toBe('button');
		expect(render({ type: 'submit' }).getAttribute('type')).toBe('submit');
	});

	it('reports itself as disabled rather than only looking it', () => {
		expect(render().disabled).toBe(false);
		expect(render({ disabled: true }).disabled).toBe(true);
	});

	it('calls what it is given when it is pressed', () => {
		const onclick = vi.fn();
		const button = render({ onclick });

		button.click();

		expect(onclick).toHaveBeenCalledTimes(1);
	});

	it('does not call anything when it is disabled', () => {
		const onclick = vi.fn();
		const button = render({ onclick, disabled: true });

		button.click();

		expect(onclick).not.toHaveBeenCalled();
	});

	it('takes a look from the variant it is given', () => {
		expect(render().className).toContain('bg-neutral-800');
		expect(render({ variant: 'primary' }).className).toContain('bg-sky-600');
		expect(render({ variant: 'danger' }).className).toContain('bg-red-900/60');
	});

	it("keeps a class of the caller's own", () => {
		expect(render({ class: 'w-full' }).className).toContain('w-full');
	});
});
