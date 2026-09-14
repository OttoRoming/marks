import { api, ApiError } from './api';

export type AdminUser = { id: string; username: string; is_admin: boolean };

export type AdminMark = {
	id: string;
	name: string;
	content: string;
	icon_id: string | null;
	user_id: string;
	owner: string;
};

export type Counts = { users: number; admins: number; marks: number; sessions: number };

/**
 * What the page is allowed to be, which is one of four things on screen.
 *
 * `asking` is not the same as `sign-in`: one is a page that has not finished asking who this is,
 * the other is a page that has been told nobody is signed in. Showing the second while the first is
 * true makes every visit flash a login form at someone who is already signed in.
 */
export type Gate = 'asking' | 'sign-in' | 'not-found' | 'ready';

type SessionAnswer = { user: AdminUser };
type UsersAnswer = { users: AdminUser[]; counts: Counts };
type MarksAnswer = { marks: AdminMark[] };

/**
 * The state the admin pages work from, and the gate they are behind.
 *
 * One object rather than one per page: the pages differ in what they list and agree on everything
 * else — who is signed in, whether they may be here, the loading flag, the line that says what
 * happened, and what a successful action does to a list.
 *
 * Nothing here is a SvelteKit load function: every fact on the page is asked for over `/api/`, which
 * is the interface the desktop client uses too. That keeps the browser and the desktop client on
 * one contract, and leaves the API as the only thing that has to be trusted.
 */
export class Admin {
	users = $state<AdminUser[]>([]);
	marks = $state<AdminMark[]>([]);
	counts = $state<Counts | null>(null);
	status = $state<{ message: string; error: boolean } | null>(null);
	busy = $state(false);

	/** Who the server says is looking at the page, and null while that is unknown or nobody is. */
	account = $state<AdminUser | null>(null);
	/** Whether the server has answered yet: the difference between asking and being told nobody. */
	asked = $state(false);
	/** Set when the admin routes answer 404: signed in, but this account is not an admin. */
	notAnAdmin = $state(false);

	get gate(): Gate {
		if (!this.asked) {
			return 'asking';
		}

		if (!this.account) {
			return 'sign-in';
		}

		return this.notAnAdmin ? 'not-found' : 'ready';
	}

	/** Asks who this is, and whether they may be here. What the layout does on mount. */
	async start(): Promise<void> {
		this.asked = false;
		this.notAnAdmin = false;

		try {
			const { user } = await api.get<SessionAnswer>('/api/session');
			this.account = user;
		} catch (error) {
			// 401 is not a failure to report: it is the ordinary answer to a browser that has no
			// session, and it is what the sign-in form is for.
			this.account = null;
			if (!(error instanceof ApiError && error.status === 401)) {
				this.status = { message: explain(error), error: true };
			}

			this.asked = true;
			return;
		}

		this.asked = true;
		await this.loadUsers();
	}

	/** Signs in, and asks again who that turned out to be. */
	async signIn(username: string, password: string): Promise<void> {
		this.busy = true;
		this.status = null;

		try {
			await api.post('/api/auth/login', { username, password });
		} catch (error) {
			this.status = { message: explain(error), error: true };
			this.busy = false;
			return;
		}

		this.busy = false;
		await this.start();
	}

	/**
	 * Loads the accounts, and the counts that come with them.
	 *
	 * Written out rather than run through `#doing` because a 404 here is not a failure: it is this
	 * API's answer to "may this account administer", which the page turns into a page of its own
	 * rather than a line of complaint.
	 */
	async loadUsers(): Promise<void> {
		this.busy = true;
		this.status = null;
		this.notAnAdmin = false;

		try {
			const answer = await api.get<UsersAnswer>('/api/admin/users');
			this.users = answer.users;
			this.counts = answer.counts;
		} catch (error) {
			if (error instanceof ApiError && error.status === 404) {
				this.notAnAdmin = true;
			} else {
				this.status = { message: explain(error), error: true };
			}
		} finally {
			this.busy = false;
		}
	}

	async loadMarks(): Promise<void> {
		await this.#doing(
			() => api.get<MarksAnswer>('/api/admin/marks'),
			(answer) => {
				this.marks = answer.marks;
				// Nothing said: the table is on screen, and announcing a page that has just loaded
				// says what the reader is about to read anyway.
				return null;
			}
		);
	}

	async setAdmin(user: AdminUser, is_admin: boolean): Promise<void> {
		await this.#doing(
			() => api.patch<{ user: AdminUser }>(`/api/admin/users/${user.id}`, { is_admin }),
			(updated) => {
				this.users = this.users.map((held) => (held.id === updated.user.id ? updated.user : held));
				this.#countAdmins();
				return `${updated.user.username} is ${updated.user.is_admin ? 'an admin now' : 'no longer an admin'}.`;
			}
		);
	}

	async removeUser(user: AdminUser): Promise<void> {
		await this.#doing(
			() => api.delete(`/api/admin/users/${user.id}`),
			() => {
				this.users = this.users.filter((held) => held.id !== user.id);
				// The account's marks went with it, so the page that lists marks is stale by exactly
				// those: dropped here rather than refetched for the same reason.
				this.marks = this.marks.filter((mark) => mark.user_id !== user.id);
				this.#countAdmins();
				return `Deleted ${user.username}, and their marks with them.`;
			}
		);
	}

	async removeMark(mark: AdminMark): Promise<void> {
		await this.#doing(
			() => api.delete(`/api/admin/marks/${mark.id}`),
			() => {
				this.marks = this.marks.filter((held) => held.id !== mark.id);
				return `Deleted "${mark.name}".`;
			}
		);
	}

	/**
	 * Runs an admin action and says what came of it.
	 *
	 * Every action here is the same shape — ask, then either say what was done or say what the
	 * server refused — so that shape is written once and `done` holds the only part that differs:
	 * what to call the result, and what it does to the lists on screen. Answering with `null` says
	 * nothing at all, which is what loading wants.
	 */
	async #doing<T>(ask: () => Promise<T>, done: (result: T) => string | null): Promise<void> {
		this.busy = true;
		this.status = null;

		try {
			const said = done(await ask());
			this.status = said === null ? null : { message: said, error: false };
		} catch (error) {
			this.status = { message: explain(error), error: true };
		} finally {
			this.busy = false;
		}
	}

	/** The counts the users answer came with, kept true as accounts change. */
	#countAdmins(): void {
		if (this.counts) {
			this.counts = {
				...this.counts,
				users: this.users.length,
				admins: this.users.filter((held) => held.is_admin).length
			};
		}
	}
}

/** What to tell the user about a call that failed. */
function explain(error: unknown): string {
	return error instanceof ApiError ? error.message : 'That did not work.';
}
