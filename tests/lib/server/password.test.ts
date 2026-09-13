import { describe, expect, it } from 'vitest';
import { hashPassword, verifyPassword } from '$lib/server/password';

/** The PHC prefix encoding algorithm, version and the pinned cost parameters. */
const PHC_PREFIX = '$argon2id$v=19$m=19456,t=2,p=1$';

function saltOf(phc: string): string {
	return phc.split('$')[4];
}

describe('hashPassword', () => {
	it('produces an argon2id PHC string with the pinned cost parameters', async () => {
		const hash = await hashPassword('supersecret');

		expect(hash.startsWith(PHC_PREFIX)).toBe(true);
	});

	it('generates a fresh salt per call, so identical passwords hash differently', async () => {
		const [first, second] = await Promise.all([
			hashPassword('supersecret'),
			hashPassword('supersecret')
		]);

		expect(first).not.toBe(second);
		expect(saltOf(first)).not.toBe(saltOf(second));
	});

	it('never embeds the plaintext', async () => {
		const hash = await hashPassword('supersecret');

		expect(hash).not.toContain('supersecret');
	});
});

describe('verifyPassword', () => {
	it('accepts the correct password', async () => {
		const hash = await hashPassword('supersecret');

		expect(await verifyPassword(hash, 'supersecret')).toBe(true);
	});

	it('rejects a wrong password', async () => {
		const hash = await hashPassword('supersecret');

		expect(await verifyPassword(hash, 'supersecreT')).toBe(false);
		expect(await verifyPassword(hash, '')).toBe(false);
	});

	it('round-trips unicode and long passwords', async () => {
		const passwords = ['pässwörd🔒ünicode', 'a'.repeat(200), ' spaces kept '];

		for (const password of passwords) {
			const hash = await hashPassword(password);
			expect(await verifyPassword(hash, password)).toBe(true);
		}
	});

	it('returns false for an empty stored hash instead of throwing', async () => {
		expect(await verifyPassword('', 'supersecret')).toBe(false);
	});

	it('returns false for malformed or truncated hashes instead of throwing', async () => {
		const malformed = [
			'not-a-hash',
			'$argon2id$',
			'$argon2id$v=19$m=19456,t=2,p=1$abc$def',
			'$argon2id$v=19$m=19456,t=2,p=1$',
			'$2b$10$abcdefghijklmnopqrstuv'
		];

		for (const hash of malformed) {
			expect(await verifyPassword(hash, 'supersecret')).toBe(false);
		}
	});

	it('does not accept a hash of a different password under the same salt', async () => {
		const hash = await hashPassword('supersecret');
		const tampered = `${hash.slice(0, hash.lastIndexOf('$') + 1)}AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA`;

		expect(await verifyPassword(tampered, 'supersecret')).toBe(false);
	});
});
