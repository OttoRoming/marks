// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
import type { SessionUser } from '$lib/server/session';

declare global {
	namespace App {
		// interface Error {}
		interface Locals {
			/** Populated by `src/hooks.server.ts` from the session cookie. */
			user: SessionUser | null;
			session: { id: string; expires_at: Date } | null;
		}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}
}

export {};
