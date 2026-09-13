const FAVICON_ENDPOINT = 'https://icons.duckduckgo.com/ip3';
const FETCH_TIMEOUT_MS = 5000;
const MAX_ICON_BYTES = 512 * 1024;

export type Favicon = {
	/** The DuckDuckGo URL the bytes came from, recorded on the `icon` row. */
	url: string;
	content: Buffer;
};

/**
 * Returns the hostname when `content` is an absolute http(s) URL, otherwise null.
 *
 * Only http/https are accepted; parsing with `new URL` is what keeps a hostile value
 * from escaping the favicon endpoint's path (a hostname cannot contain `/`).
 */
export function faviconHostname(content: string): string | null {
	let url: URL;
	try {
		url = new URL(content);
	} catch {
		return null;
	}

	if (url.protocol !== 'http:' && url.protocol !== 'https:') {
		return null;
	}

	return url.hostname || null;
}

/**
 * Identifies an image from its leading bytes.
 *
 * Needed in two places, because DuckDuckGo's `Content-Type` cannot be trusted: it serves
 * a PNG body labelled `image/vnd.microsoft.icon` for reddit.com, and there is no separate
 * MIME column on the `icon` table to store a header value in.
 */
export function sniffImageType(bytes: Uint8Array): string | null {
	if (startsWith(bytes, [0x00, 0x00, 0x01, 0x00])) {
		return 'image/x-icon';
	}
	if (startsWith(bytes, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) {
		return 'image/png';
	}
	if (startsWith(bytes, [0xff, 0xd8, 0xff])) {
		return 'image/jpeg';
	}
	if (startsWith(bytes, [0x47, 0x49, 0x46, 0x38])) {
		return 'image/gif';
	}
	if (
		startsWith(bytes, [0x52, 0x49, 0x46, 0x46]) &&
		startsWith(bytes.subarray(8), [0x57, 0x45, 0x42, 0x50])
	) {
		return 'image/webp';
	}
	return null;
}

function startsWith(bytes: Uint8Array, prefix: number[]): boolean {
	if (bytes.length < prefix.length) {
		return false;
	}
	return prefix.every((byte, index) => bytes[index] === byte);
}

/**
 * Fetches a favicon for `content` via DuckDuckGo's `icons.duckduckgo.com/ip3` endpoint,
 * or returns null when there is nothing usable to store.
 *
 * Two quirks of that endpoint drive the checks below:
 *  - a domain with no icon answers `200` with an **empty body** (example.com), and
 *  - an unregistered domain answers `404` with a generic placeholder PNG.
 * So neither `response.ok` alone nor a non-empty body alone is sufficient: the status
 * rules out the placeholder, and the byte checks rule out the empty 200.
 *
 * Never throws: a bookmark is still worth saving when its favicon cannot be fetched.
 */
export async function fetchFavicon(content: string): Promise<Favicon | null> {
	const hostname = faviconHostname(content);
	if (!hostname) {
		return null;
	}

	const url = `${FAVICON_ENDPOINT}/${encodeURIComponent(hostname)}.ico`;

	let response: Response;
	try {
		response = await fetch(url, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
	} catch {
		return null;
	}

	if (!response.ok) {
		return null;
	}

	const content_bytes = Buffer.from(await response.arrayBuffer());

	if (content_bytes.byteLength === 0 || content_bytes.byteLength > MAX_ICON_BYTES) {
		return null;
	}

	// Refuse to store anything that is not recognisably an image.
	if (sniffImageType(content_bytes) === null) {
		return null;
	}

	return { url, content: content_bytes };
}
