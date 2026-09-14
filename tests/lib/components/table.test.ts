// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { createRawSnippet, mount } from 'svelte';
import Table from '$lib/components/Table.svelte';

beforeEach(() => {
	document.body.innerHTML = '';
});

const columns = [
	{ label: 'Name' },
	{ label: 'Owner' },
	{ label: 'Actions', align: 'right' as const }
];

const row = createRawSnippet(() => ({
	render: () => '<tr><th scope="row">Beta</th><td>sam</td><td></td></tr>'
}));

function render(props: Record<string, unknown> = {}) {
	const target = document.createElement('div');
	document.body.append(target);

	mount(Table, {
		target,
		props: { caption: 'Marks on this server', columns, count: 1, children: row, ...props }
	});

	const table = target.querySelector('table');
	if (!table) {
		throw new Error('no table was rendered');
	}

	return table;
}

describe('Table', () => {
	it('is a real table with a real caption', () => {
		const table = render();

		expect(table.tagName).toBe('TABLE');
		expect(table.querySelector('caption')?.textContent).toBe('Marks on this server');
	});

	it('gives every column a column header', () => {
		const headers = [...render().querySelectorAll('thead th')];

		expect(headers.map((header) => header.textContent?.trim())).toEqual([
			'Name',
			'Owner',
			'Actions'
		]);
		// `scope="col"` is what lets a screen reader say which column a cell is in rather than
		// reading a grid of cells at the reader.
		expect(headers.every((header) => header.getAttribute('scope') === 'col')).toBe(true);
	});

	it('renders the rows it is given', () => {
		const table = render();

		expect(table.querySelectorAll('tbody tr')).toHaveLength(1);
		expect(table.querySelector('tbody th')?.getAttribute('scope')).toBe('row');
	});

	it('says so, in one cell spanning the table, when there is nothing to show', () => {
		const table = render({ count: 0, empty: 'No marks yet.' });
		const cells = [...table.querySelectorAll('tbody td')];

		expect(cells).toHaveLength(1);
		expect(cells[0].textContent?.trim()).toBe('No marks yet.');
		expect(cells[0].getAttribute('colspan')).toBe('3');
	});

	it('leaves the caller to say what "nothing" means', () => {
		expect(render({ count: 0 }).textContent).toContain('Nothing here.');
	});
});
