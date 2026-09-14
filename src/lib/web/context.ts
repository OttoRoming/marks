import { getContext, setContext } from 'svelte';
import type { Admin } from './admin.svelte';

const KEY = Symbol('admin');

/**
 * The page's admin state, put where its pages can find it.
 *
 * One instance for the whole of `/admin`, made by the layout that owns the gate and read by the
 * pages that own a list each: a page that made its own would ask the server who it is all over
 * again, and would have its own opinion about whether anyone is signed in.
 */
export function provideAdmin(admin: Admin): void {
	setContext(KEY, admin);
}

/** The admin state the layout provided. Throws rather than making a second one. */
export function useAdmin(): Admin {
	const admin = getContext<Admin | undefined>(KEY);

	if (!admin) {
		throw new Error('no admin state here: this component is not inside the admin layout');
	}

	return admin;
}
