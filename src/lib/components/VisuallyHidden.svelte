<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * Text for whoever is listening rather than looking.
	 *
	 * For the things a page says twice — once in a heading, once as the caption of the table under
	 * it — and for the labels a control needs but a design refuses to show.
	 */
	let { children }: { children: Snippet } = $props();
</script>

<span class="sr-only">{@render children()}</span>
