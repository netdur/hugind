import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { loginRequest } from "../auth/api";

const Status = Object.freeze({
  idle: "idle",
  submitting: "submitting",
  success: "success",
  error: "error"
});

function normalizeUsername(raw) {
  return String(raw || "").trim();
}

function validate({ username, password }) {
  const errors = {};
  if (!username) errors.username = "Username is required";
  if (!password) errors.password = "Password is required";
  return errors;
}

export default function LoginForm({ onLogin }) {
  const abortRef = useRef(null);

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [status, setStatus] = useState(Status.idle);
  const [errors, setErrors] = useState({});
  const [serverHint, setServerHint] = useState("");

  const canSubmit = useMemo(() => {
    return status !== Status.submitting;
  }, [status]);

  useEffect(() => {
    return () => {
      if (abortRef.current) abortRef.current.abort();
    };
  }, []);

  const submit = useCallback(async () => {
    const payload = {
      username: normalizeUsername(username),
      password
    };

    const v = validate(payload);
    setErrors(v);
    setServerHint("");

    if (Object.keys(v).length > 0) {
      setStatus(Status.error);
      return;
    }

    if (abortRef.current) abortRef.current.abort();
    abortRef.current = new AbortController();

    setStatus(Status.submitting);

    try {
      const result = await loginRequest({
        username: payload.username,
        password: payload.password,
        signal: abortRef.current.signal
      });

      setStatus(Status.success);
      onLogin && onLogin(result.token);
    } catch (e) {
      // Current behavior: only show a small inline hint.
      // TASK will ask to add a proper error dialog here.
      setStatus(Status.error);
      setServerHint(e && e.message ? e.message : "Login failed");
    }
  }, [username, password, onLogin]);

  return (
    <div style={{ maxWidth: 420 }}>
      <h2>Sign in</h2>

      <label style={{ display: "block", marginBottom: 8 }}>
        Username
        <input
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          disabled={!canSubmit}
          style={{ display: "block", width: "100%" }}
        />
        {errors.username ? <div style={{ color: "crimson" }}>{errors.username}</div> : null}
      </label>

      <label style={{ display: "block", marginBottom: 8 }}>
        Password
        <input
          value={password}
          type="password"
          onChange={(e) => setPassword(e.target.value)}
          disabled={!canSubmit}
          style={{ display: "block", width: "100%" }}
        />
        {errors.password ? <div style={{ color: "crimson" }}>{errors.password}</div> : null}
      </label>

      {serverHint ? <div style={{ color: "crimson", marginBottom: 8 }}>{serverHint}</div> : null}

      <button onClick={submit} disabled={!canSubmit}>
        {status === Status.submitting ? "Signing in..." : "Sign in"}
      </button>
    </div>
  );
}
