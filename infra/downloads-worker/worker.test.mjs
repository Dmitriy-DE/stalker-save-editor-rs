// node --test infra/downloads-worker/worker.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import worker from "./worker.mjs";

function environment({ allow = true } = {}) {
  const stored = new Map();
  return {
    stored,
    BUCKET: {
      async put(key, body) { stored.set(key, body); },
      async get(key) { return stored.has(key) ? { body: stored.get(key), httpEtag: "e", writeHttpMetadata() {} } : null; },
    },
    DIAGNOSTICS_RATE_LIMITER: { async limit() { return { success: allow }; } },
  };
}

function post(body, type = "application/json") {
  return new Request("https://example.invalid/metrics", { method: "POST", headers: { "content-type": type }, body });
}

const summary = JSON.stringify({ schema: 1, sessions: 3, first_frame_avg_ms: 48, save_operations: [] });

test("a schema 1 summary is stored under metrics/", async () => {
  const env = environment();
  const response = await worker.fetch(post(summary), env);
  assert.equal(response.status, 201);
  assert.equal(env.stored.size, 1);
  const [key] = env.stored.keys();
  assert.match(key, /^metrics\/.+\.json$/);
});

test("anything that is not a schema 1 object is refused and not stored", async () => {
  for (const body of ["not json", "[]", "null", '{"schema":2,"sessions":1}', '{"schema":1}', '{"schema":1,"sessions":-1}']) {
    const env = environment();
    const response = await worker.fetch(post(body), env);
    assert.equal(response.status, 400, body);
    assert.equal(env.stored.size, 0, body);
  }
});

test("wrong method, wrong type, oversize and rate limit are refused", async () => {
  const env = environment();
  assert.equal((await worker.fetch(new Request("https://example.invalid/metrics"), env)).status, 405);
  assert.equal((await worker.fetch(post(summary, "text/plain"), env)).status, 415);
  assert.equal((await worker.fetch(post(" ".repeat(16 * 1024 + 1)), env)).status, 413);
  assert.equal((await worker.fetch(post(summary), environment({ allow: false }))).status, 429);
  assert.equal(env.stored.size, 0);
});

test("stored summaries and diagnostics are not downloadable", async () => {
  const env = environment();
  await worker.fetch(post(summary), env);
  const [key] = env.stored.keys();
  assert.equal((await worker.fetch(new Request(`https://example.invalid/${key}`), env)).status, 404);
  assert.equal((await worker.fetch(new Request("https://example.invalid/diagnostics/x.log.gz"), env)).status, 404);
});
