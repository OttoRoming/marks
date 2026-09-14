import { z } from 'zod';

/**
 * What an admin may change about an account.
 *
 * Only the one flag, and deliberately: a username is the account's identity and a password is
 * the account's own business, so an admin interface changes who may administer, not who anyone is.
 * An account's marks are not deletable here either — deleting the account is, and the marks go
 * with it through the schema's cascades (`onDelete: 'cascade'` on `mark.user_id`).
 */
export const userUpdateSchema = z.object({
	is_admin: z.boolean()
});

export type UserUpdateInput = z.infer<typeof userUpdateSchema>;
