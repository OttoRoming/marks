<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * A table of things, as a real `<table>`.
	 *
	 * Everything a screen reader needs in order to say "Owner: otto. Name: Beta. Row three of
	 * seven" is what a table has always carried: a `<caption>`, a `<thead>`, `<th scope="col">`, and
	 * one `<tr>` per row. None of it is a `div` wearing a role, and none of it has to be explained
	 * to each assistive technology separately — which is the whole argument for this library.
	 *
	 * The rows are the caller's: this holds the frame, which every page shares, and the cells, which
	 * every page differs in.
	 */
	let {
		caption,
		columns,
		count,
		empty = 'Nothing here.',
		children
	}: {
		caption: string;
		columns: { label: string; align?: 'left' | 'right' }[];
		count: number;
		empty?: string;
		children: Snippet;
	} = $props();
</script>

<div class="overflow-x-auto rounded-lg border border-neutral-800">
	<table class="w-full border-collapse text-sm">
		<caption class="sr-only">{caption}</caption>

		<thead class="bg-neutral-900 text-left text-xs tracking-wide text-neutral-400 uppercase">
			<tr>
				{#each columns as column (column.label)}
					<th
						scope="col"
						class="px-4 py-2 font-medium {column.align === 'right' ? 'text-right' : ''}"
					>
						{column.label}
					</th>
				{/each}
			</tr>
		</thead>

		<tbody class="divide-y divide-neutral-800">
			{#if count === 0}
				<tr>
					<td colspan={columns.length} class="px-4 py-6 text-center text-neutral-500">
						{empty}
					</td>
				</tr>
			{:else}
				{@render children()}
			{/if}
		</tbody>
	</table>
</div>
