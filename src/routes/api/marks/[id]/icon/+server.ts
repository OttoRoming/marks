import { eq } from 'drizzle-orm';
import type { RequestHandler } from './$types';
import { icon } from '$lib/server/db/schema';
import { db } from '$lib/server/db';
import { notFound, requireUser } from '$lib/server/api';
import { sniffImageType } from '$lib/server/favicon';
import { getOwnMark } from '$lib/server/marks';

/** Serves the mark's stored favicon bytes. */
export const GET: RequestHandler = async ({ params, locals }) => {
	const auth = requireUser(locals);
	if (!auth.success) {
		return auth.response;
	}

	const existing = await getOwnMark(params.id, auth.user.id);
	if (!existing?.icon_id) {
		return notFound('Icon not found');
	}

	const [row] = await db
		.select({ content: icon.content })
		.from(icon)
		.where(eq(icon.id, existing.icon_id));

	const bytes = row?.content;
	if (!bytes || bytes.byteLength === 0) {
		return notFound('Icon not found');
	}

	// The type is sniffed from the bytes rather than stored, because DuckDuckGo's
	// Content-Type header is unreliable (see sniffImageType).
	return new Response(new Uint8Array(bytes), {
		headers: {
			'content-type': sniffImageType(bytes) ?? 'application/octet-stream',
			'cache-control': 'private, max-age=604800'
		}
	});
};
