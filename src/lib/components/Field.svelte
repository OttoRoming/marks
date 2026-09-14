<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';

	/**
	 * A labelled text field: a real `<input>` with a real `<label for>`.
	 *
	 * The two things that make a field usable without looking at it are wired here rather than left
	 * to each caller — `aria-describedby` pointing at the description and the error, and
	 * `aria-invalid` while there is one — because a field whose helper text is only visually beside
	 * it is a field whose helper text does not exist.
	 *
	 * The id comes from `$props.id()`, which Svelte generates per component instance, so two fields
	 * on a page cannot end up describing each other.
	 */
	let {
		label,
		description = undefined,
		error = undefined,
		value = $bindable(''),
		class: klass = '',
		...rest
	}: HTMLInputAttributes & {
		label: string;
		description?: string;
		error?: string;
		value?: string;
	} = $props();

	const id = $props.id();
	const description_id = `${id}-description`;
	const error_id = `${id}-error`;

	// Only the parts that are actually there: `aria-describedby` pointing at nothing reads as
	// nothing in some screen readers and as a mistake in others.
	const described_by = $derived(
		[description ? description_id : null, error ? error_id : null].filter(Boolean).join(' ') ||
			undefined
	);
</script>

<div class="flex flex-col gap-1.5">
	<label for={id} class="text-sm font-medium text-neutral-200">{label}</label>

	{#if description}
		<p id={description_id} class="text-xs text-neutral-400">{description}</p>
	{/if}

	<input
		{id}
		bind:value
		aria-describedby={described_by}
		aria-invalid={error ? true : undefined}
		class="rounded border border-neutral-700 bg-neutral-950 px-3 py-1.5 text-sm text-neutral-100 placeholder:text-neutral-500 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-sky-400 aria-[invalid=true]:border-red-700 {klass}"
		{...rest}
	/>

	{#if error}
		<p id={error_id} role="alert" class="text-xs text-red-400">{error}</p>
	{/if}
</div>
