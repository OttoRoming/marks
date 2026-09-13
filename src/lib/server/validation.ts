import { z } from 'zod';
import { fromError } from 'zod-validation-error';
import { badRequest } from './api';

type ParseJsonBodyResult<T> = { success: true; data: T } | { success: false; response: Response };

/**
 * Reads a JSON request body and validates it against `schema`.
 *
 * On failure the returned `response` is already a 400: `Invalid JSON body` when the
 * body is absent or not valid JSON (which zod cannot see, since it only receives
 * parsed values), otherwise `Validation failed` with per-field messages.
 */
export async function parseJsonBody<TSchema extends z.ZodType>(
	request: Request,
	schema: TSchema
): Promise<ParseJsonBodyResult<z.output<TSchema>>> {
	let body: unknown;
	try {
		body = await request.json();
	} catch {
		return { success: false, response: badRequest('Invalid JSON body') };
	}

	const parsed = schema.safeParse(body);
	if (!parsed.success) {
		const errorMessage = fromError(parsed.error).toString();

		return {
			success: false,
			response: badRequest(errorMessage)
		};
	}

	return { success: true, data: parsed.data };
}
