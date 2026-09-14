<script lang="ts">
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import Dialog from '$lib/components/Dialog.svelte';
	import Field from '$lib/components/Field.svelte';
	import Status from '$lib/components/Status.svelte';
	import Table from '$lib/components/Table.svelte';
	import type { AdminMark } from '$lib/web/admin.svelte';
	import { useAdmin } from '$lib/web/context';

	// The layout's own state: it has already asked who is here and whether they may be.
	const admin = useAdmin();
	onMount(() => void admin.loadMarks());

	/** How many rows are drawn at once. */
	const LIMIT = 200;

	let query = $state('');

	// Filtered here rather than by the API: the server sends the list once, and this is a page of
	// rows already in the browser. What it must not do is pretend the list is complete when it is
	// not, which is what the line below the field is for.
	const found = $derived(
		query.trim() === ''
			? admin.marks
			: admin.marks.filter((mark) =>
					[mark.name, mark.content, mark.owner].some((field) =>
						field.toLowerCase().includes(query.trim().toLowerCase())
					)
				)
	);

	const shown = $derived(found.slice(0, LIMIT));

	/** The mark a confirmation is being asked about, while one is. */
	let pending = $state<AdminMark | null>(null);
	let confirming = $state(false);

	function askToDelete(mark: AdminMark): void {
		pending = mark;
		confirming = true;
	}

	async function deleteConfirmed(): Promise<void> {
		const mark = pending;
		confirming = false;

		if (mark) {
			await admin.removeMark(mark);
		}
	}
</script>

<h2 class="text-base font-semibold">Marks</h2>
<p class="mt-1 text-sm text-neutral-400">Every mark on this server, and whose it is.</p>

<div class="mt-4 max-w-md">
	<Field
		label="Filter"
		type="search"
		bind:value={query}
		placeholder="A name, an address, or an account"
		description="Matches anywhere in the name, the content or the owner."
	/>
</div>

<div class="mt-4">
	<Status message={admin.status?.message ?? null} error={admin.status?.error ?? false} />
</div>

{#if found.length > shown.length}
	<p class="mt-4 text-sm text-neutral-400" role="status">
		Showing {shown.length} of {found.length}. Narrow it with the filter.
	</p>
{/if}

<div class="mt-4">
	<Table
		caption="Marks on this server"
		columns={[
			{ label: 'Name' },
			{ label: 'Owner' },
			{ label: 'Content' },
			{ label: 'Actions', align: 'right' }
		]}
		count={shown.length}
		empty={query.trim() === '' ? 'No marks yet.' : 'Nothing matches that filter.'}
	>
		{#each shown as mark (mark.id)}
			<tr class="hover:bg-neutral-900/60">
				<th scope="row" class="px-4 py-2 text-left font-medium">{mark.name}</th>
				<td class="px-4 py-2 text-neutral-400">{mark.owner}</td>
				<td class="max-w-xs truncate px-4 py-2 font-mono text-xs text-neutral-400">
					{mark.content}
				</td>
				<td class="px-4 py-2">
					<div class="flex justify-end">
						<Button variant="danger" disabled={admin.busy} onclick={() => askToDelete(mark)}>
							Delete
						</Button>
					</div>
				</td>
			</tr>
		{/each}
	</Table>
</div>

<Dialog bind:open={confirming} heading="Delete this mark?">
	<p>
		<strong class="font-medium text-neutral-100">{pending?.name}</strong> will be deleted, whoever saved
		it. This cannot be undone.
	</p>

	{#snippet actions()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button variant="danger" disabled={admin.busy} onclick={deleteConfirmed}>Delete</Button>
	{/snippet}
</Dialog>
