/**
 * muxal session binding — pi extension.
 *
 * Writes the active session id into the per-pane binding file named by
 * $MUXAL_SESSION_BINDING_FILE, so muxal can reopen the conversation you left
 * (after /new, an in-TUI resume, or a fork) instead of the one the pane
 * launched with. The pi counterpart of Claude's SessionStart hook.
 */

import { renameSync, writeFileSync } from "node:fs";

type SessionCtx = { sessionManager: { getSessionId(): string } };

export default function (pi: {
	on(event: string, handler: (event: unknown, ctx: SessionCtx) => unknown): unknown;
}) {
	const target = process.env.MUXAL_SESSION_BINDING_FILE;
	if (!target) return;

	let read: () => string | undefined = () => undefined;
	let last = "";

	const write = () => {
		try {
			const id = read();
			if (!id || id === last) return;
			last = id;
			const tmp = `${target}.${process.pid}.tmp`;
			writeFileSync(tmp, JSON.stringify({ session_id: id, cwd: process.cwd() }));
			renameSync(tmp, target);
		} catch {
			// Bookkeeping must never disturb the session.
		}
	};

	pi.on("session_start", (_event, ctx) => {
		read = () => {
			try {
				return ctx.sessionManager.getSessionId();
			} catch {
				return undefined;
			}
		};
		write();
	});

	// Switches and forks complete after these fire; the matching session_start
	// re-arms the reader on the fresh context, and these re-checks close the
	// gap in between.
	for (const event of ["session_before_switch", "session_before_fork", "session_shutdown"]) {
		pi.on(event, () => {
			setTimeout(write, 50);
			setTimeout(write, 500);
			setTimeout(write, 2000);
		});
	}

	// Belt and braces: any conversation activity re-checks the binding.
	pi.on("message_end", write);
	pi.on("turn_end", write);
}
