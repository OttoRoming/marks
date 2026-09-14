import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, api } from '$lib/web/api';

/** A `fetch` that answers once, and records what it was asked. */
function answering(response: Response): { path: string; init?: RequestInit }[] {
	const calls: { path: string; init?: RequestInit }[] = [];

	vi.stubGlobal(
		'fetch',
		vi.fn(async (path: string, init?: RequestInit) => {
			calls.push({ path, init });
			return response;
		})
	);

	return calls;
}

function json(body: unknown, status = 200): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { 'content-type': 'application/json' }
	});
}

/**
 * The refusal a call came back with.
 *
 * A helper because every test below wants the same three things out of a failure — that it is an
 * `ApiError`, what it says, and the status it came with — and catching it by hand in each of them
 * gets the typing and the rethrow wrong in each of them.
 */
async function refusal(doing: Promise<unknown>): Promise<ApiError> {
	try {
		await doing;
	} catch (error) {
		if (error instanceof ApiError) {
			return error;
		}
		throw error;
	}

	throw new Error('the call was expected to be refused');
}

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('api', () => {
	it('reads the body of an answer that has one', async () => {
		answering(json({ counts: { users: 2 } }));

		await expect(api.get('/api/admin/users')).resolves.toEqual({ counts: { users: 2 } });
	});

	it('answers with nothing when there is nothing to read', async () => {
		// 204 has no body, and reading one as JSON throws — which is what the owner's own delete
		// and this one both answer with.
		answering(new Response(null, { status: 204 }));

		await expect(api.delete('/api/admin/marks/m1')).resolves.toBeUndefined();
	});

	it('asks the origin it was served from, with the method and the body it was given', async () => {
		const calls = answering(json({ user: {} }));

		await api.patch('/api/admin/users/u1', { is_admin: true });

		// Relative, which is the point: the browser resolves it against the server that served the
		// page, so there is nowhere to configure and nothing to get wrong.
		expect(calls[0].path).toBe('/api/admin/users/u1');
		expect(calls[0].init?.method).toBe('PATCH');
		expect(calls[0].init?.body).toBe('{"is_admin":true}');
		expect((calls[0].init?.headers as Record<string, string>)['content-type']).toBe(
			'application/json'
		);
	});

	it('sends no body and no content type on a request that has neither', async () => {
		const calls = answering(new Response(null, { status: 204 }));

		await api.delete('/api/admin/marks/m1');

		expect(calls[0].init?.body).toBeUndefined();
		expect(calls[0].init?.headers).toBeUndefined();
	});

	it("refuses in the server's own words, with the status it came with", async () => {
		answering(json({ error: 'The last admin cannot be deleted' }, 409));

		const error = await refusal(api.delete('/api/admin/users/u1'));

		expect(error.message).toBe('The last admin cannot be deleted');
		expect(error.status).toBe(409);
	});

	it('falls back to the status line when the failure is not this API speaking', async () => {
		// A proxy in front of the server answers with HTML, and "Unexpected token <" is not
		// something to show anyone.
		answering(new Response('<html>bad gateway</html>', { status: 502, statusText: 'Bad Gateway' }));

		const error = await refusal(api.get('/api/admin/users'));

		expect(error.message).toBe('502 Bad Gateway');
	});
});
