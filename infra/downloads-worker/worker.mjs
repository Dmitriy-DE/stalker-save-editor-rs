// Public file server for save-editor downloads and opt-in diagnostics intake.
// Serves objects from the R2 bucket bound as BUCKET, so the private code repo
// stays private while release binaries are publicly downloadable.
const MAX_DIAGNOSTIC_BYTES = 2 * 1024 * 1024;
const DIAGNOSTIC_RATE_LIMIT_KEY = "diagnostics";
// Aggregate performance summary the desktop app sends only after separate consent (A17). It is a small JSON object
// with timings and counts; it carries no paths, names or identifiers, and the Worker stores nothing about the sender.
const MAX_METRICS_BYTES = 16 * 1024;
const METRICS_RATE_LIMIT_KEY = "metrics";
const METRICS_SCHEMA = 1;

async function readLimitedBody(request, maxBytes) {
  if (!request.body) {
    const body = await request.arrayBuffer();
    return body.byteLength <= maxBytes ? body : null;
  }
  const reader = request.body.getReader();
  const chunks = [];
  let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      const chunk = value instanceof Uint8Array ? value : new Uint8Array(value);
      total += chunk.byteLength;
      if (total > maxBytes) {
        try {
          await reader.cancel();
        } catch {
          // The body is already rejected; cancellation is only best effort.
        }
        return null;
      }
      chunks.push(chunk);
    }
  } finally {
    reader.releaseLock();
  }
  const body = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return body.buffer;
}

function jsonResponse(value, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

function diagnosticsError(message, status) {
  return jsonResponse({ error: message }, status);
}

async function checkDiagnosticsRateLimit(env, key = DIAGNOSTIC_RATE_LIMIT_KEY) {
  const limiter = env.DIAGNOSTICS_RATE_LIMITER;
  if (!limiter || typeof limiter.limit !== "function") {
    return 503;
  }
  try {
    const result = await limiter.limit({ key });
    return result.success ? null : 429;
  } catch {
    return 503;
  }
}

// Accepts only a JSON object of the known schema; anything else is refused before it is stored.
function parseMetrics(body) {
  let value;
  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(body));
  } catch {
    return null;
  }
  if (value === null || typeof value !== "object" || Array.isArray(value)) return null;
  if (value.schema !== METRICS_SCHEMA) return null;
  if (!Number.isSafeInteger(value.sessions) || value.sessions < 0 || value.sessions > 1000) return null;
  return value;
}

async function receiveMetrics(request, env) {
  if (request.method !== "POST") {
    return diagnosticsError("Method Not Allowed", 405);
  }
  const contentType = (request.headers.get("content-type") || "").split(";", 1)[0].trim().toLowerCase();
  if (contentType !== "application/json") {
    return diagnosticsError("Metrics payload must be application/json", 415);
  }
  const declaredLengthHeader = request.headers.get("content-length");
  const declaredLength = Number(declaredLengthHeader || "0");
  if (declaredLengthHeader !== null && (!Number.isSafeInteger(declaredLength) || declaredLength < 0)) {
    return diagnosticsError("Metrics content length is invalid", 400);
  }
  if (declaredLength > MAX_METRICS_BYTES) {
    return diagnosticsError("Metrics payload is too large", 413);
  }
  const rateLimitStatus = await checkDiagnosticsRateLimit(env, METRICS_RATE_LIMIT_KEY);
  if (rateLimitStatus === 429) {
    return diagnosticsError("Metrics rate limit exceeded", 429);
  }
  if (rateLimitStatus !== null) {
    return diagnosticsError("Metrics service is not configured", 503);
  }
  let body;
  try {
    body = await readLimitedBody(request, MAX_METRICS_BYTES);
  } catch {
    return diagnosticsError("Metrics payload could not be read", 400);
  }
  if (body === null || body.byteLength === 0) {
    return diagnosticsError("Metrics payload is too large or empty", 413);
  }
  if (parseMetrics(body) === null) {
    return diagnosticsError("Metrics payload is not a schema 1 summary", 400);
  }
  const reportId = crypto.randomUUID();
  const key = `metrics/${new Date().toISOString().replace(/[:.]/g, "-")}-${reportId}.json`;
  try {
    await env.BUCKET.put(key, body, {
      httpMetadata: { contentType: "application/json", cacheControl: "private, no-store" },
      customMetadata: { reportId, source: "save-editor-desktop" },
    });
  } catch {
    return diagnosticsError("Metrics storage unavailable", 503);
  }
  return jsonResponse({ report_id: reportId }, 201);
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (url.pathname === "/metrics") {
      return receiveMetrics(request, env);
    }

    if (url.pathname === "/diagnostics") {
      if (request.method !== "POST") {
        return diagnosticsError("Method Not Allowed", 405);
      }
      const contentType = (request.headers.get("content-type") || "").split(";", 1)[0].toLowerCase();
      const declaredLengthHeader = request.headers.get("content-length");
      const declaredLength = Number(declaredLengthHeader || "0");
      if (
        declaredLengthHeader !== null &&
        (!Number.isSafeInteger(declaredLength) || declaredLength < 0)
      ) {
        return diagnosticsError("Diagnostics content length is invalid", 400);
      }
      if (contentType !== "application/gzip") {
        return diagnosticsError("Diagnostics payload must be application/gzip", 415);
      }
      if (declaredLength > MAX_DIAGNOSTIC_BYTES) {
        return diagnosticsError("Diagnostics payload is too large", 413);
      }
      const rateLimitStatus = await checkDiagnosticsRateLimit(env);
      if (rateLimitStatus === 429) {
        return diagnosticsError("Diagnostics rate limit exceeded", 429);
      }
      if (rateLimitStatus !== null) {
        return diagnosticsError("Diagnostics service is not configured", 503);
      }
      let body;
      try {
        body = await readLimitedBody(request, MAX_DIAGNOSTIC_BYTES);
      } catch {
        return diagnosticsError("Diagnostics payload could not be read", 400);
      }
      if (body === null || body.byteLength === 0) {
        return diagnosticsError("Diagnostics payload is too large or empty", 413);
      }
      const reportId = crypto.randomUUID();
      const key = `diagnostics/${new Date().toISOString().replace(/[:.]/g, "-")}-${reportId}.log.gz`;
      try {
        await env.BUCKET.put(key, body, {
          httpMetadata: {
            contentType: "application/gzip",
            cacheControl: "private, no-store",
          },
          customMetadata: {
            reportId,
            source: "save-editor-desktop",
          },
        });
      } catch {
        return diagnosticsError("Diagnostics storage unavailable", 503);
      }
      return jsonResponse({ report_id: reportId }, 201);
    }

    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method Not Allowed", { status: 405 });
    }
    const key = decodeURIComponent(url.pathname.slice(1));
    if (!key) {
      return new Response("save-editor downloads\n", {
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }
    if (key.startsWith("diagnostics/") || key.startsWith("metrics/")) {
      return diagnosticsError("Not found", 404);
    }
    const object = await env.BUCKET.get(key);
    if (object === null) {
      return new Response("Not found", { status: 404 });
    }
    const headers = new Headers();
    object.writeHttpMetadata(headers);
    headers.set("etag", object.httpEtag);
    const manifest = key === "latest.json";
    // Every published name is reused by the next release (latest.json, its signature, the APT indices and the
    // packages themselves), so nothing here is immutable. The publisher stores the policy on the object; an object
    // without one still gets a short, revalidated lifetime. A year-long "immutable" made clients pair a new manifest
    // with an old signature or package and fail verification.
    if (!headers.has("cache-control") || /immutable/i.test(headers.get("cache-control"))) {
      headers.set("cache-control", "public, max-age=60, must-revalidate");
    }
    if (manifest) {
      headers.set("content-type", "application/json; charset=utf-8");
      headers.set("content-disposition", "inline");
    } else {
      headers.set("content-disposition", `attachment; filename="${key.split("/").pop()}"`);
    }
    return new Response(request.method === "HEAD" ? null : object.body, { headers });
  },
};
