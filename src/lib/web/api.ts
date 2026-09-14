/**
 * The API of the instance this page was served from.
 *
 * There is no address to configure and none to ask for: a browser reaches a marks server by being
 * served by it, so every call is same-origin and the session cookie the sign-in sets travels with
 * it without anything here having to carry a token. That is the difference from the desktop client,
 * which has to be told which server to talk to and has to keep the token itself.
 */

/** A refusal from the server, carrying the server's own wording and the status it came with. */
export class ApiError extends Error {
	constructor(
		message: string,
		readonly status: number
	) {
		super(message);
		this.name = 'ApiError';
	}
}

type Method = 'GET' | 'POST' | 'PATCH' | 'DELETE';

/**
 * One request, and the only place `fetch` is called.
 *
 * Every route in this API answers failures with an `{ error }` body (see `errorResponse`), so
 * turning a status into a sentence happens once, here, rather than at each call site deciding what
 * to show.
 */
async function request<T>(method: Method, path: string, body?: unknown): Promise<T> {
	const response = await fetch(path, {
		method,
		headers: body === undefined ? undefined : { 'content-type': 'application/json' },
		body: body === undefined ? undefined : JSON.stringify(body)
	});

	if (!response.ok) {
		throw new ApiError(await errorMessage(response), response.status);
	}

	// A 204 carries no body, and reading one as JSON throws.
	if (response.status === 204) {
		return undefined as T;
	}

	return (await response.json()) as T;
}

/**
 * The server's wording for a failure, falling back to the status line.
 *
 * The fallback is not decoration: a proxy in front of the server, or a server that fell over, can
 * answer with HTML that is not JSON at all, and "502 Bad Gateway" is a better thing to show than
 * "Unexpected token <".
 */
async function errorMessage(response: Response): Promise<string> {
	try {
		const body = (await response.json()) as { error?: unknown };

		if (typeof body.error === 'string' && body.error !== '') {
			return body.error;
		}
	} catch {
		// Not JSON: the status is all there is to go on.
	}

	return `${response.status} ${response.statusText}`.trim();
}

/** The four verbs this API uses, over the one request function above. */
export const api = {
	get: <T>(path: string) => request<T>('GET', path),
	post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
	patch: <T>(path: string, body?: unknown) => request<T>('PATCH', path, body),
	delete: <T = void>(path: string) => request<T>('DELETE', path)
};
