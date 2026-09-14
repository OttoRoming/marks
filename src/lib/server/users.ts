import { user } from './db/schema';

/**
 * The columns of an account that are safe to send to a client.
 *
 * The password hash is not one of them, and that is the whole reason this selection exists rather
 * than each query naming the columns it wants: a `select()` of the table itself would put a hash
 * into a JSON body the first time somebody wrote one.
 *
 * It lives in its own module rather than beside the admin queries that used to hold it, because the
 * session route needs it too — an account describing itself and an admin listing other accounts
 * must agree about what an account is.
 */
export const userSelection = {
	id: user.id,
	username: user.username,
	is_admin: user.is_admin
};
