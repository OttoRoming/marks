<script lang="ts">
	import Button from '$lib/components/Button.svelte';
	import Dialog from '$lib/components/Dialog.svelte';
	import Status from '$lib/components/Status.svelte';
	import Table from '$lib/components/Table.svelte';
	import VisuallyHidden from '$lib/components/VisuallyHidden.svelte';
	import type { AdminUser } from '$lib/web/admin.svelte';
	import { useAdmin } from '$lib/web/context';

	// The layout's own state, not a second copy of it: it asked who is here, and it is the one that
	// knows whether the pages may be shown at all.
	const admin = useAdmin();

	/**
	 * Who is doing the administering.
	 *
	 * Their own row offers nothing to press rather than offering it disabled: an admin cannot remove
	 * their own rights or delete their own account — the server refuses both — so the honest thing
	 * on screen is that there is nothing to do there, not a button that will not work.
	 */
	const me = $derived(admin.account?.username ?? '');

	/** The account a confirmation is being asked about, while one is. */
	let pending = $state<AdminUser | null>(null);
	let confirming = $state(false);

	function askToDelete(user: AdminUser): void {
		pending = user;
		confirming = true;
	}

	async function deleteConfirmed(): Promise<void> {
		const user = pending;
		confirming = false;

		if (user) {
			await admin.removeUser(user);
		}
	}
</script>

<h2 class="text-base font-semibold">Accounts</h2>
<p class="mt-1 text-sm text-neutral-400">
	Every account on this server. Marks belong to the account that saved them, and go when it does.
</p>

{#if admin.counts}
	<dl class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
		{#each Object.entries(admin.counts) as [label, value] (label)}
			<div class="rounded-lg border border-neutral-800 bg-neutral-900 px-4 py-3">
				<dt class="text-xs tracking-wide text-neutral-400 uppercase">{label}</dt>
				<dd class="text-xl font-semibold">{value}</dd>
			</div>
		{/each}
	</dl>
{/if}

<div class="mt-4">
	<Status message={admin.status?.message ?? null} error={admin.status?.error ?? false} />
</div>

<div class="mt-4">
	<Table
		caption="Accounts on this server"
		columns={[{ label: 'Account' }, { label: 'Rights' }, { label: 'Actions', align: 'right' }]}
		count={admin.users.length}
		empty="No accounts yet."
	>
		{#each admin.users as user (user.id)}
			{@const self = user.username === me}
			<tr class="hover:bg-neutral-900/60">
				<th scope="row" class="px-4 py-2 text-left font-medium">
					{user.username}
					{#if self}
						<VisuallyHidden>(you)</VisuallyHidden>
					{/if}
				</th>

				<td class="px-4 py-2 text-neutral-400">{user.is_admin ? 'Admin' : 'Account'}</td>

				<td class="px-4 py-2">
					<div class="flex justify-end gap-2">
						{#if self}
							<span class="text-sm text-neutral-500">Your own account</span>
						{:else}
							<Button disabled={admin.busy} onclick={() => admin.setAdmin(user, !user.is_admin)}>
								{user.is_admin ? 'Remove admin' : 'Make admin'}
							</Button>
							<Button variant="danger" disabled={admin.busy} onclick={() => askToDelete(user)}>
								Delete
							</Button>
						{/if}
					</div>
				</td>
			</tr>
		{/each}
	</Table>
</div>

<Dialog bind:open={confirming} heading="Delete this account?">
	<p>
		<strong class="font-medium text-neutral-100">{pending?.username}</strong> and every mark they saved
		will be deleted. This cannot be undone.
	</p>

	{#snippet actions()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button variant="danger" disabled={admin.busy} onclick={deleteConfirmed}>Delete</Button>
	{/snippet}
</Dialog>
