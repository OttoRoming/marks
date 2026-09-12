import { hash, verify, type Options } from '@node-rs/argon2';

/**
 * argon2id with OWASP's recommended baseline parameters.
 *
 * `algorithm` is intentionally omitted: `Algorithm` is a type-erased `const enum`
 * (it is `{}` at runtime) and argon2id is the library default, so the emitted PHC
 * string always starts with `$argon2id$`. The cost parameters are pinned anyway so
 * a change in library defaults cannot silently weaken hashes.
 *
 * Each call generates a fresh 16-byte random salt, which `hash()` encodes into the
 * returned PHC string — there is no separate salt to store.
 */
const HASH_OPTIONS = {
	memoryCost: 19456, // 19 MiB
	timeCost: 2, // passes
	outputLen: 32, // bytes
	parallelism: 1
} satisfies Options;

/** Hashes a password, returning a self-describing PHC string: algorithm, params, salt and digest. */
export function hashPassword(password: string): Promise<string> {
	return hash(password, HASH_OPTIONS);
}

/**
 * Checks a password against a stored PHC hash.
 *
 * Returns `false` rather than throwing for empty or malformed hashes, so a corrupt
 * row cannot turn a login attempt into a 500.
 */
export async function verifyPassword(stored_hash: string, password: string): Promise<boolean> {
	if (!stored_hash) {
		return false;
	}

	try {
		return await verify(stored_hash, password);
	} catch {
		return false;
	}
}
