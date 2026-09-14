import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { ApiError } from '$lib/web/api';

vi.mock('$lib/web/api', async (importOriginal) => {
	const actual = await importOriginal<typeof import('$lib/web/api')>();

	return {
		...actual,
		api: { get: vi.fn(), post: vi.fn(), patch: vi.fn(), delete: vi.fn() }
	};
});

import { api } from '$lib/web/api';
import { Admin, type AdminMark, type AdminUser } from '$lib/web/admin.svelte';

/**
 * The four verbs are mocked wholesale, so they are typed as mocks here rather than as the generic
 * methods they stand in for: what a test checks is which call was made with what, and what the
 * store did with the answer.
 */
const mocked = api as unknown as { get: Mock; post: Mock; patch: Mock; delete: Mock };

const otto = { id: 'u1', username: 'otto', is_admin: true };
const sam = { id: 'u2', username: 'sam', is_admin: false };
const beta = {
	id: 'm1',
	name: 'Beta',
	content: 'https://beta.example/',
	icon_id: null,
	user_id: 'u2',
	owner: 'sam'
};

const counts = { users: 2, admins: 1, marks: 1, sessions: 3 };

/** An admin page that has loaded both lists, which is the state each test starts from. */
async function loaded(
	users: AdminUser[] = [otto, sam],
	marks: AdminMark[] = [beta]
): Promise<Admin> {
	mocked.get.mockImplementation(async (path: string) =>
		path === '/api/admin/marks' ? { marks } : { users, counts }
	);

	const admin = new Admin();
	await admin.loadUsers();
	await admin.loadMarks();

	return admin;
}

beforeEach(() => {
	vi.clearAllMocks();
	mocked.get.mockResolvedValue({ users: [], counts });
});

describe('Admin', () => {
	it('fills the lists and the counts from one answer', async () => {
		mocked.get.mockResolvedValue({ users: [otto, sam], counts });

		const admin = new Admin();
		await admin.loadUsers();

		expect(mocked.get).toHaveBeenCalledWith('/api/admin/users');
		expect(admin.users).toEqual([otto, sam]);
		expect(admin.counts).toEqual(counts);
	});

	it('says nothing when a page loads', async () => {
		// The table and the counts are on screen; announcing a page that has just loaded says what
		// the reader is about to read.
		const admin = await loaded();

		expect(admin.status).toBe(null);
	});

	it('reports what it did, and stops being busy', async () => {
		const admin = await loaded();
		mocked.patch.mockResolvedValue({ user: { ...sam, is_admin: true } });

		await admin.setAdmin(sam, true);

		expect(mocked.patch).toHaveBeenCalledWith('/api/admin/users/u2', { is_admin: true });
		expect(admin.status).toEqual({ message: 'sam is an admin now.', error: false });
		expect(admin.busy).toBe(false);
	});

	it('changes the one account it was asked about', async () => {
		const admin = await loaded();
		mocked.patch.mockResolvedValue({ user: { ...sam, is_admin: true } });

		await admin.setAdmin(sam, true);

		expect(admin.users).toEqual([otto, { ...sam, is_admin: true }]);
		// The counts the listing came with are kept true rather than refetched: the answer already
		// said what changed.
		expect(admin.counts?.admins).toBe(2);
		expect(mocked.get).toHaveBeenCalledTimes(2);
	});

	it("takes the account's marks with the account", async () => {
		const admin = await loaded();
		expect(admin.marks).toHaveLength(1);

		mocked.delete.mockResolvedValue(undefined);
		await admin.removeUser(sam);

		expect(admin.users).toEqual([otto]);
		expect(admin.marks).toEqual([]);
		expect(admin.status).toEqual({
			message: 'Deleted sam, and their marks with them.',
			error: false
		});
	});

	it('drops a mark that was deleted', async () => {
		const admin = await loaded();
		mocked.delete.mockResolvedValue(undefined);

		await admin.removeMark(beta);

		expect(mocked.delete).toHaveBeenCalledWith('/api/admin/marks/m1');
		expect(admin.marks).toEqual([]);
	});

	it("reports a refusal in the server's words and changes nothing", async () => {
		const admin = await loaded();
		mocked.delete.mockRejectedValue(new ApiError('The last admin cannot be deleted', 409));

		await admin.removeUser(otto);

		expect(admin.status).toEqual({
			message: 'The last admin cannot be deleted',
			error: true
		});
		expect(admin.users).toEqual([otto, sam]);
	});

	it('says something generally true when the failure was not the API speaking', async () => {
		const admin = await loaded();
		mocked.get.mockRejectedValue(new Error('the network went away'));

		await admin.loadUsers();

		expect(admin.status).toEqual({ message: 'That did not work.', error: true });
	});
});

describe('the gate', () => {
	it('says it is still asking before it has asked', () => {
		// Not "sign in": a page that has not finished asking who this is would flash a login form at
		// someone who is already signed in.
		expect(new Admin().gate).toBe('asking');
	});

	it('asks for a sign-in when nobody is signed in', async () => {
		mocked.get.mockRejectedValue(new ApiError('Unauthorized', 401));

		const admin = new Admin();
		await admin.start();

		expect(admin.gate).toBe('sign-in');
		expect(admin.account).toBe(null);
		// The ordinary answer to a browser with no cookie is a form to fill in, not a failure.
		expect(admin.status).toBe(null);
	});

	it('opens the pages for an admin', async () => {
		mocked.get.mockImplementation(async (path: string) =>
			path === '/api/session' ? { user: otto } : { users: [otto, sam], counts }
		);

		const admin = new Admin();
		await admin.start();

		expect(admin.account).toEqual(otto);
		expect(admin.gate).toBe('ready');
		expect(admin.users).toEqual([otto, sam]);
	});

	it('has nothing at all for an account that is not an admin', async () => {
		// 404 from the admin routes is this API saying "not yours" — the same answer it gives for a
		// page that was never there, which is the whole reason for it.
		mocked.get.mockImplementation(async (path: string) => {
			if (path === '/api/session') {
				return { user: sam };
			}
			throw new ApiError('Not found', 404);
		});

		const admin = new Admin();
		await admin.start();

		expect(admin.account).toEqual(sam);
		expect(admin.gate).toBe('not-found');
		expect(admin.status).toBe(null);
	});

	it('signs in, and then asks who that turned out to be', async () => {
		mocked.get.mockRejectedValue(new ApiError('Unauthorized', 401));
		mocked.post.mockResolvedValue(undefined);

		const admin = new Admin();
		await admin.start();
		expect(admin.gate).toBe('sign-in');

		mocked.get.mockImplementation(async (path: string) =>
			path === '/api/session' ? { user: otto } : { users: [otto], counts }
		);
		await admin.signIn('otto', 'supersecret');

		expect(mocked.post).toHaveBeenCalledWith('/api/auth/login', {
			username: 'otto',
			password: 'supersecret'
		});
		expect(admin.gate).toBe('ready');
	});

	it('says what the server said when the sign-in was refused, and stays at the form', async () => {
		mocked.get.mockRejectedValue(new ApiError('Unauthorized', 401));
		mocked.post.mockRejectedValue(new ApiError('Incorrect username or password', 401));

		const admin = new Admin();
		await admin.start();
		await admin.signIn('otto', 'wrong');

		expect(admin.status).toEqual({ message: 'Incorrect username or password', error: true });
		expect(admin.gate).toBe('sign-in');
	});
});
