import path from 'node:path';
import { defineConfig } from 'vitest/config';

/**
 * Kept separate from `vite.config.ts` so unit tests do not load the SvelteKit,
 * Tailwind or adapter plugins.
 *
 * Tests live in `tests/` rather than beside their subjects, so the `$lib` alias is
 * declared here to mirror SvelteKit's own, letting tests import modules the same way
 * application code does. Only DB-free modules are covered, so no `$app/*` shim is needed.
 */
export default defineConfig({
	resolve: {
		alias: {
			$lib: path.resolve(import.meta.dirname, 'src/lib')
		}
	},
	test: {
		environment: 'node',
		include: ['tests/**/*.test.ts']
	}
});
