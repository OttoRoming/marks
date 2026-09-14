<script lang="ts">
	/**
	 * The line that says what just happened.
	 *
	 * Two roles, because they ask different questions of a screen reader: `status` is polite and
	 * waits for a pause in whatever is being read, which suits "Saved"; `alert` interrupts, which
	 * suits "that was refused". A failure that waited its turn is a failure found later.
	 */
	let { message = null, error = false }: { message?: string | null; error?: boolean } = $props();
</script>

{#if message}
	<p
		role={error ? 'alert' : 'status'}
		class="rounded border px-3 py-2 text-sm {error
			? 'border-red-800 bg-red-950/50 text-red-200'
			: 'border-neutral-700 bg-neutral-900 text-neutral-300'}"
	>
		{message}
	</p>
{/if}
