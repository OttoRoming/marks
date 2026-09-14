import path from 'node:path';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

/**
 * Kept separate from `vite.config.ts` so unit tests do not load the SvelteKit,
 * Tailwind or adapter plugins.
 *
 * Tests live in `tests/` rather than beside their subjects, so the `$lib` alias is
 * declared here to mirror SvelteKit's own, letting tests import modules the same way
 * application code does. Only DB-free modules are covered, so no `$app/*` shim is needed.
 *
 * The Svelte plugin is here for the tests that mount a component, and for the runes in
 * `$lib/web/*.svelte.ts`. It only transforms `.svelte` and `.svelte.ts` files, so the tests that
 * are about the server keep running in plain node — a test that wants a DOM asks for one itself,
 * with a `@vitest-environment jsdom` comment at the top of the file, rather than every test paying
 * for one.
 */
export default defineConfig({
	plugins: [
		svelte({
			compilerOptions: {
				// The same mode the application is compiled in (see `vite.config.ts`): a component
				// compiled the other way round here would not be the component that ships.
				runes: true
			}
		})
	],
	resolve: {
		// A mounted component needs the browser build of `svelte`; the server one throws on `mount`.
		conditions: ['browser'],
		alias: {
			$lib: path.resolve(import.meta.dirname, 'src/lib')
		}
	},
	test: {
		environment: 'node',
		include: ['tests/**/*.test.ts']
	}
});
