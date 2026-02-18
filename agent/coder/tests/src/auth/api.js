export async function loginRequest({ username, password, signal }) {
  const res = await fetch("/api/login", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ username, password }),
    signal
  });

  // Convention: server returns { ok: true, token } or { ok: false, error: { code, message } }
  const data = await res.json().catch(() => ({}));

  if (!res.ok) {
    const msg = (data && data.error && data.error.message) || `HTTP ${res.status}`;
    const err = new Error(msg);
    err.code = (data && data.error && data.error.code) || "HTTP_ERROR";
    err.httpStatus = res.status;
    throw err;
  }

  if (!data || data.ok !== true || !data.token) {
    const err = new Error("Malformed login response");
    err.code = "BAD_RESPONSE";
    throw err;
  }

  return { token: data.token };
}
