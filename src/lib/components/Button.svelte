<script lang="ts">
	import type { HTMLButtonAttributes } from 'svelte/elements';

	/**
	 * A button, and nothing more than one.
	 *
	 * The element does the work a button has always done — it can be reached with Tab, activated by
	 * Enter and Space, and reported as disabled rather than merely looking it — so all this adds is
	 * the look, and the three looks an admin page needs.
	 *
	 * `type` defaults to `button` rather than to the element's own `submit`: an admin page has
	 * buttons that are not forms, and a button that submits whatever form it happens to be inside
	 * is a button that does something other than what it says.
	 */
	let {
		variant = 'quiet',
		type = 'button',
		class: klass = '',
		children,
		...rest
	}: HTMLButtonAttributes & { variant?: 'quiet' | 'primary' | 'danger' } = $props();

	const looks = {
		quiet:
			'border-neutral-700 bg-neutral-800 text-neutral-100 hover:bg-neutral-700 disabled:hover:bg-neutral-800',
		primary: 'border-sky-500 bg-sky-600 text-white hover:bg-sky-500 disabled:hover:bg-sky-600',
		danger:
			'border-red-800 bg-red-900/60 text-red-100 hover:bg-red-800 disabled:hover:bg-red-900/60'
	};

	const shared =
		'inline-flex items-center justify-center gap-2 rounded border px-3 py-1.5 text-sm font-medium transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-sky-400 disabled:cursor-not-allowed disabled:opacity-50';
</script>

<button {type} class="{shared} {looks[variant]} {klass}" {...rest}>
	{@render children?.()}
</button>
