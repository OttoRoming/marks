<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import type { Snippet } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import Field from '$lib/components/Field.svelte';
	import Status from '$lib/components/Status.svelte';
	import { Admin } from '$lib/web/admin.svelte';
	import { provideAdmin } from '$lib/web/context';

	/**
	 * The admin shell, and the gate in front of it.
	 *
	 * Nothing here is a load function: the page asks `/api/session` who it is talking to, and asks
	 * the admin routes whether that account may be here. The API is the whole interface — the same
	 * one the desktop client uses — so what the browser is allowed to know is decided in one place
	 * rather than in two.
	 *
	 * The four states are four different pages, because they are: a page still asking, a page asking
	 * for a sign-in, a page that exists for someone else, and the pages themselves.
	 */
	let { children }: { children: Snippet } = $props();

	const admin = new Admin();
	provideAdmin(admin);

	onMount(() => void admin.start());

	let username = $state('');
	let password = $state('');

	async function signIn(event: SubmitEvent): Promise<void> {
		event.preventDefault();
		await admin.signIn(username, password);
		password = '';
	}

	// The ids as literals, not as strings: `resolve` is typed by the routes this application has, so
	// a link naming a page that does not exist is a build error rather than a 404.
	const pages = [
		{ href: '/admin', label: 'Accounts' },
		{ href: '/admin/marks', label: 'Marks' }
	] as const;

	const here = $derived(page.url.pathname.replace(/\/$/, ''));
</script>

<svelte:head><title>Admin · Marks</title></svelte:head>

<div class="min-h-screen bg-neutral-950 text-neutral-100">
	{#if admin.gate === 'asking'}
		<p class="px-6 py-12 text-sm text-neutral-400" role="status">Checking who you are…</p>
	{:else if admin.gate === 'sign-in'}
		<main class="mx-auto max-w-sm px-6 py-12">
			<h1 class="text-lg font-semibold">Admin</h1>
			<p class="mt-1 text-sm text-neutral-400">
				Sign in to an account that administers this server.
			</p>

			<!-- A real form, so that Enter submits it, a password manager offers to fill it, and the
			     fields are announced as a username and a password rather than as two text boxes. -->
			<form class="mt-6 flex flex-col gap-4" onsubmit={signIn}>
				<Field label="Username" bind:value={username} autocomplete="username" />
				<Field
					label="Password"
					type="password"
					bind:value={password}
					autocomplete="current-password"
				/>
				<Button type="submit" variant="primary" disabled={admin.busy}>Sign in</Button>
			</form>

			<div class="mt-4">
				<Status message={admin.status?.message ?? null} error={admin.status?.error ?? false} />
			</div>
		</main>
	{:else if admin.gate === 'not-found'}
		<main class="mx-auto max-w-md px-6 py-12">
			<h1 class="text-lg font-semibold">Not found</h1>
			<p class="mt-1 text-sm text-neutral-400">
				There is nothing here for {admin.account?.username}. These pages are for admins.
			</p>
		</main>
	{:else}
		<header class="border-b border-neutral-800 bg-neutral-900">
			<div class="mx-auto flex max-w-5xl flex-wrap items-baseline justify-between gap-2 px-6 pt-4">
				<h1 class="text-lg font-semibold">Admin</h1>
				<p class="text-sm text-neutral-400">signed in as {admin.account?.username}</p>
			</div>

			<nav class="mx-auto max-w-5xl px-4">
				<ul class="flex gap-1">
					{#each pages as item (item.href)}
						{@const current = here === item.href}
						<li>
							<a
								href={resolve(item.href)}
								aria-current={current ? 'page' : undefined}
								class="-mb-px inline-block border-b-2 px-3 py-3 text-sm {current
									? 'border-sky-400 font-medium text-neutral-100'
									: 'border-transparent text-neutral-400 hover:text-neutral-200'}"
							>
								{item.label}
							</a>
						</li>
					{/each}
				</ul>
			</nav>
		</header>

		<main class="mx-auto max-w-5xl px-6 py-6">
			{@render children()}
		</main>
	{/if}
</div>
