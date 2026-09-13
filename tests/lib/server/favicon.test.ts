import { afterEach, describe, expect, it, vi } from 'vitest';
import { faviconHostname, fetchFavicon, sniffImageType } from '$lib/server/favicon';

const ICO_BYTES = new Uint8Array([0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x10, 0x10]);
const PNG_BYTES = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00]);
const JPEG_BYTES = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]);
const GIF_BYTES = new Uint8Array([...Buffer.from('GIF89a'), 0x01, 0x00]);
const WEBP_BYTES = new Uint8Array([...Buffer.from('RIFF'), 0, 0, 0, 0, ...Buffer.from('WEBP')]);
const WAV_BYTES = new Uint8Array([...Buffer.from('RIFF'), 0, 0, 0, 0, ...Buffer.from('WAVE')]);

/** Stubs global fetch with a single canned response (or a thrown error). */
function stubFetch(result: Response | Error) {
	const mock = vi.fn<typeof fetch>(async () => {
		if (result instanceof Error) {
			throw result;
		}
		return result;
	});

	vi.stubGlobal('fetch', mock);
	return mock;
}

afterEach(() => {
	vi.unstubAllGlobals();
});

describe('faviconHostname', () => {
	it('returns the hostname of an http or https URL', () => {
		expect(faviconHostname('https://github.com')).toBe('github.com');
		expect(faviconHostname('http://github.com/some/path')).toBe('github.com');
	});

	it('strips the port, credentials and path', () => {
		expect(faviconHostname('https://sub.example.com:8443/x?q=1')).toBe('sub.example.com');
		expect(faviconHostname('https://user:pw@github.com/x')).toBe('github.com');
	});

	it('lowercases and punycodes the host', () => {
		expect(faviconHostname('HTTPS://GitHub.COM/Path')).toBe('github.com');
		expect(faviconHostname('https://münchen.de')).toBe('xn--mnchen-3ya.de');
	});

	it('tolerates surrounding whitespace, since URL parsing strips it', () => {
		expect(faviconHostname('  https://github.com  ')).toBe('github.com');
	});

	it('returns null for non-URL text', () => {
		expect(faviconHostname('')).toBeNull();
		expect(faviconHostname('not a url')).toBeNull();
		expect(faviconHostname('just a note about https://github.com')).toBeNull();
	});

	it('rejects non-http(s) schemes', () => {
		expect(faviconHostname('ftp://files.example.com/pub')).toBeNull();
		expect(faviconHostname('javascript:alert(1)')).toBeNull();
		expect(faviconHostname('file:///etc/passwd')).toBeNull();
		expect(faviconHostname('data:text/plain,hello')).toBeNull();
	});

	it('collapses traversal segments, so a path cannot escape the endpoint', () => {
		// The hostname is the only part used in the favicon URL, and a hostname cannot
		// contain "/", so this yields a harmless lookup rather than a path traversal.
		expect(faviconHostname('https://a.com/../../evil.com.ico')).toBe('a.com');
	});
});

describe('sniffImageType', () => {
	it('identifies supported image formats', () => {
		expect(sniffImageType(ICO_BYTES)).toBe('image/x-icon');
		expect(sniffImageType(PNG_BYTES)).toBe('image/png');
		expect(sniffImageType(JPEG_BYTES)).toBe('image/jpeg');
		expect(sniffImageType(GIF_BYTES)).toBe('image/gif');
		expect(sniffImageType(WEBP_BYTES)).toBe('image/webp');
	});

	it('requires the WEBP tag, not just a RIFF container', () => {
		expect(sniffImageType(WAV_BYTES)).toBeNull();
	});

	it('returns null for empty, truncated or non-image data', () => {
		expect(sniffImageType(new Uint8Array())).toBeNull();
		expect(sniffImageType(PNG_BYTES.subarray(0, 7))).toBeNull();
		expect(sniffImageType(new Uint8Array([0x00, 0x00, 0x01]))).toBeNull();
		expect(sniffImageType(Buffer.from('<!doctype html><html>'))).toBeNull();
	});

	it('does not recognise SVG, which is deliberately unsupported', () => {
		expect(sniffImageType(Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>'))).toBeNull();
	});
});

describe('fetchFavicon', () => {
	it('returns the bytes and the source URL for a real icon', async () => {
		stubFetch(new Response(ICO_BYTES, { status: 200 }));

		const favicon = await fetchFavicon('https://github.com');

		expect(favicon).not.toBeNull();
		expect(favicon?.url).toBe('https://icons.duckduckgo.com/ip3/github.com.ico');
		expect(favicon?.content.equals(Buffer.from(ICO_BYTES))).toBe(true);
	});

	it('requests the endpoint with the URL-encoded hostname only', async () => {
		const mock = stubFetch(new Response(PNG_BYTES, { status: 200 }));

		await fetchFavicon('https://münchen.de/some/deep/path?x=1#frag');

		expect(mock).toHaveBeenCalledTimes(1);
		expect(mock.mock.calls[0][0]).toBe('https://icons.duckduckgo.com/ip3/xn--mnchen-3ya.de.ico');
	});

	it('does not call fetch at all when the content is not a URL', async () => {
		const mock = stubFetch(new Response(ICO_BYTES, { status: 200 }));

		expect(await fetchFavicon('just some text')).toBeNull();
		expect(await fetchFavicon('ftp://example.com/x')).toBeNull();
		expect(mock).not.toHaveBeenCalled();
	});

	it('rejects the generic placeholder DuckDuckGo serves with 404', async () => {
		// Observed behaviour: every unregistered host gets this same 1478-byte PNG,
		// so storing it would pin a generic globe to the bookmark as if it were real.
		const placeholder = new Uint8Array(1478);
		placeholder.set(PNG_BYTES);
		stubFetch(new Response(placeholder, { status: 404 }));

		expect(await fetchFavicon('https://bogus-domain-xyz987.com')).toBeNull();
	});

	it('rejects the empty 200 body DuckDuckGo returns for a domain with no icon', async () => {
		// Observed for https://example.com: status 200, Content-Type text/plain, 0 bytes.
		stubFetch(new Response(new Uint8Array(), { status: 200 }));

		expect(await fetchFavicon('https://example.com')).toBeNull();
	});

	it('rejects a non-image body even on a 200', async () => {
		stubFetch(new Response(Buffer.from('<html>not an icon</html>'), { status: 200 }));

		expect(await fetchFavicon('https://github.com')).toBeNull();
	});

	it('rejects a body over the size cap', async () => {
		const oversized = new Uint8Array(512 * 1024 + 1);
		oversized.set(ICO_BYTES);
		stubFetch(new Response(oversized, { status: 200 }));

		expect(await fetchFavicon('https://github.com')).toBeNull();
	});

	it('returns null for server errors', async () => {
		stubFetch(new Response('nope', { status: 500 }));
		expect(await fetchFavicon('https://github.com')).toBeNull();
	});

	it('returns null instead of throwing when the request fails', async () => {
		stubFetch(new TypeError('fetch failed'));
		expect(await fetchFavicon('https://github.com')).toBeNull();

		stubFetch(new DOMException('The operation was aborted', 'AbortError'));
		expect(await fetchFavicon('https://github.com')).toBeNull();
	});

	it('passes an abort signal so a slow endpoint cannot hang the request', async () => {
		const mock = stubFetch(new Response(ICO_BYTES, { status: 200 }));

		await fetchFavicon('https://github.com');

		const [, init] = mock.mock.calls[0];
		expect(init?.signal).toBeInstanceOf(AbortSignal);
	});
});
