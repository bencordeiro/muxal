// Run with Node 22.6+: node --experimental-strip-types scripts/test-pi-session-binding.mjs
// Exercises the real bundled extension without launching Pi or an agent.
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import bindingExtension from "../crates/muxal/assets/pi/muxal-session-binding.ts";

const sandbox = mkdtempSync(join(tmpdir(), "muxal-pi-binding-test-"));
const target = join(sandbox, "binding.json");
const originalTarget = process.env.MUXAL_SESSION_BINDING_FILE;
process.env.MUXAL_SESSION_BINDING_FILE = target;
const handlers = new Map();
bindingExtension({
	on(event, handler) {
		const list = handlers.get(event) ?? [];
		list.push(handler);
		handlers.set(event, list);
	},
});
let sessionId = "empty-new-session";
const ctx = { sessionManager: { getSessionId: () => sessionId } };
const emit = async (type) => {
	for (const handler of handlers.get(type) ?? []) await handler({ type }, ctx);
};
const input = () => JSON.parse(readFileSync(`${target}.input`, "utf8"));

try {
	await emit("session_start");
	assert.equal(JSON.parse(readFileSync(target, "utf8")).session_id, sessionId);
	assert.equal(input().waiting_for_input, false);
	assert.equal(input().cwd, process.cwd());

	await emit("ui_prompt_start");
	assert.equal(input().waiting_for_input, true);
	await emit("ui_prompt_start");
	await emit("ui_prompt_end");
	assert.equal(input().waiting_for_input, true, "a nested prompt is still open");
	await emit("ui_prompt_end");
	assert.equal(input().waiting_for_input, false);

	// Muxal consumes the conversation-switch record. Prompt state must survive.
	unlinkSync(target);
	await emit("ui_prompt_start");
	assert.equal(input().waiting_for_input, true);
	sessionId = "another-empty-session";
	await emit("session_start");
	assert.equal(input().session_id, sessionId);
	assert.equal(input().waiting_for_input, false, "session switches clear old prompts");
	await emit("ui_prompt_end");
	assert.equal(input().waiting_for_input, false, "late end events cannot underflow");

	// A failed save must remain retryable on later conversation activity.
	rmSync(sandbox, { recursive: true });
	sessionId = "retry-after-write-error";
	await emit("session_start");
	mkdirSync(sandbox);
	await emit("turn_end");
	assert.equal(JSON.parse(readFileSync(target, "utf8")).session_id, sessionId);
	await emit("ui_prompt_start");
	assert.equal(input().waiting_for_input, true);
	await emit("session_shutdown");
	assert.equal(input().waiting_for_input, false);
	console.log("Pi binding: session switches, nested prompts, shutdown and write retries passed");
} finally {
	if (originalTarget === undefined) delete process.env.MUXAL_SESSION_BINDING_FILE;
	else process.env.MUXAL_SESSION_BINDING_FILE = originalTarget;
	rmSync(sandbox, { recursive: true, force: true });
}
