// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { createRawSnippet, flushSync, mount } from 'svelte';
import type { Snippet } from 'svelte';
import Dialog from '$lib/components/Dialog.svelte';

/**
 * jsdom has `HTMLDialogElement` but not `showModal` or `close`, so the two methods the component
 * uses are supplied here rather than worked around in the component.
 *
 * They are the whole contract: the component asks the browser for a modal dialog and listens for
 * the close that follows. What the browser does with focus, the top layer and making the rest of
 * the page inert is its own work — the part that cannot be tested here, and the part that would be
 * a mistake to reimplement.
 */
beforeEach(() => {
	const proto = HTMLDialogElement.prototype as unknown as {
		showModal?: () => void;
		close?: () => void;
	};

	proto.showModal ??= function (this: HTMLDialogElement) {
		this.setAttribute('open', '');
	};

	proto.close ??= function (this: HTMLDialogElement) {
		this.removeAttribute('open');
		this.dispatchEvent(new Event('close'));
	};

	document.body.innerHTML = '';
});

/** A snippet standing in for the markup a page would write between the tags. */
function markup(html: string): Snippet {
	return createRawSnippet(() => ({ render: () => html }));
}

function render(props: Record<string, unknown> = {}) {
	const target = document.createElement('div');
	document.body.append(target);

	// Content always, because a dialog with nothing in it is not a dialog: the tests below are about
	// the frame, and the frame is around something.
	//
	// Flushed, because what opens the dialog is an effect — `showModal` is a call to the element
	// rather than something the markup can say — and an effect does not run until the mount is
	// flushed.
	flushSync(() => {
		mount(Dialog, {
			target,
			props: {
				heading: 'Delete this mark?',
				children: markup('<p>Beta will be deleted.</p>'),
				...props
			}
		});
	});

	const dialog = target.querySelector('dialog');
	if (!dialog) {
		throw new Error('no dialog was rendered');
	}

	return dialog;
}

describe('Dialog', () => {
	it('renders a real dialog element', () => {
		expect(render().tagName).toBe('DIALOG');
	});

	it('asks the browser for a modal dialog when it is open', () => {
		expect(render({ open: true }).hasAttribute('open')).toBe(true);
	});

	it('is not shown until it is asked for', () => {
		expect(render({ open: false }).hasAttribute('open')).toBe(false);
	});

	it('is named by its heading, which is how a screen reader announces it', () => {
		const dialog = render();
		const heading = dialog.querySelector('h2');

		expect(heading?.textContent?.trim()).toBe('Delete this mark?');
		expect(heading?.id).toBeTruthy();
		expect(dialog.getAttribute('aria-labelledby')).toBe(heading?.id);
	});

	it('renders the content and the actions it is given', () => {
		const dialog = render({
			children: markup('<p>Beta will be deleted.</p>'),
			actions: markup('<button>Delete</button>')
		});

		expect(dialog.textContent).toContain('Beta will be deleted.');
		expect(dialog.querySelector('button')?.textContent).toBe('Delete');
	});

	it('leaves the actions out when there are none', () => {
		expect(render().querySelector('button')).toBe(null);
	});
});
