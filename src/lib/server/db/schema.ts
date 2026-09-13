import { blob, integer, sqliteTable, text } from 'drizzle-orm/sqlite-core';
import { generateSessionToken } from '../session';
import dayjs from 'dayjs';

export const task = sqliteTable('task', {
	id: text('id')
		.primaryKey()
		.$defaultFn(() => crypto.randomUUID()),
	title: text('title').notNull(),
	priority: integer('priority').notNull().default(1)
});

export const user = sqliteTable('user', {
	id: text('id')
		.primaryKey()
		.$defaultFn(() => crypto.randomUUID()),
	username: text('username').unique(),
	password: text('password'),
	is_admin: integer({ mode: 'boolean' })
});

export const mark = sqliteTable('mark', {
	id: text('id')
		.primaryKey()
		.$defaultFn(() => crypto.randomUUID()),
	name: text('name'),
	content: text('content'),
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
