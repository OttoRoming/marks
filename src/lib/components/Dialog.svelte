<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A modal dialog built on the native one.
	 *
	 * `<dialog>` with `showModal()` is the reason this component is this small: the browser puts it
	 * in the top layer, moves focus into it and keeps it there, closes it on Escape, makes
	 * everything behind it inert and unclickable, and announces it as a dialog to a screen reader.
	 * A positioned `div` with a backdrop and a key handler has to be taught all five, and is usually
	 * taught three of them.
	 *
	 * The element is driven rather than rendered into, because `showModal()` is what gives all of
	 * that. Escape is therefore not handled here: the browser closes the dialog, the `close` event
	 * follows, and the `open` binding is brought back into step by it.
	 */
	let {
		open = $bindable(false),
		heading,
		children,
		actions = undefined
	}: {
		open?: boolean;
		heading: string;
		children: Snippet;
		actions?: Snippet;
	} = $props();

	const id = $props.id();
	let element = $state<HTMLDialogElement | null>(null);

	$effect(() => {
		const dialog = element;
		if (!dialog) {
			return;
		}

		if (open && !dialog.open) {
			dialog.showModal();
		} else if (!open && dialog.open) {
			dialog.close();
		}
	});
</script>

<dialog
	bind:this={element}
	aria-labelledby={id}
	onclose={() => (open = false)}
	class="w-full max-w-md rounded-lg border border-neutral-700 bg-neutral-900 p-0 text-neutral-100 backdrop:bg-black/60"
>
	<h2 {id} class="border-b border-neutral-800 px-5 py-3 text-base font-semibold">{heading}</h2>

	<div class="px-5 py-4 text-sm text-neutral-300">
		{@render children()}
	</div>

	{#if actions}
		<div class="flex justify-end gap-2 border-t border-neutral-800 px-5 py-3">
			{@render actions()}
		</div>
	{/if}
</dialog>
