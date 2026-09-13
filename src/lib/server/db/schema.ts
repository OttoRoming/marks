import { blob, integer, sqliteTable, text } from 'drizzle-orm/sqlite-core';
import dayjs from 'dayjs';

export const user = sqliteTable('user', {
	id: text('id')
		.primaryKey()
		.$defaultFn(() => crypto.randomUUID()),
	username: text('username').notNull().unique(),
	password: text('password').notNull(),
	is_admin: integer({ mode: 'boolean' }).notNull().default(false)
});

export const mark = sqliteTable('mark', {
	id: text('id')
		.primaryKey()
		.$defaultFn(() => crypto.randomUUID()),
	name: text('name').notNull(),
	content: text('content').notNull(),
	icon: blob()
});

export const session = sqliteTable('session', {
	/** The opaque session token itself, not a UUID: see `createSession`. */
	id: text('id')
		.primaryKey()
		.$defaultFn(() =>
			Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString('base64url')
		),
	user_id: text('user_id')
		.notNull()
		.references(() => user.id, { onDelete: 'cascade' }),
	expires_at: integer('expires_at', { mode: 'timestamp_ms' })
		.notNull()
		.$defaultFn(() => dayjs().add(30, 'days').toDate())
});
