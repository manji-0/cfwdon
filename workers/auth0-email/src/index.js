const JSON_HEADERS = {
  "Content-Type": "application/json; charset=utf-8",
};

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (request.method === "GET" && url.pathname === "/health") {
      return json({ ok: true });
    }

    if (request.method !== "POST" || url.pathname !== "/send-auth0-email") {
      return json({ error: "Not found" }, 404);
    }

    const auth = request.headers.get("Authorization") || "";
    if (!env.AUTH0_EMAIL_TOKEN || auth !== `Bearer ${env.AUTH0_EMAIL_TOKEN}`) {
      return json({ error: "Unauthorized" }, 401);
    }

    let payload;
    try {
      payload = await request.json();
    } catch {
      return json({ error: "Invalid JSON" }, 400);
    }

    const message = buildEmailMessage(payload, env);
    const validationError = validateEmailMessage(message);
    if (validationError) {
      return json({ error: validationError }, 400);
    }

    try {
      const result = await env.EMAIL.send(message);
      return json({ ok: true, messageId: result.messageId });
    } catch (error) {
      console.error("Cloudflare Email Service send failed", {
        code: error.code,
        message: error.message,
      });

      const status = error.code === "E_RATE_LIMIT_EXCEEDED" ? 429 : 502;
      return json(
        {
          ok: false,
          code: error.code || "E_SEND_FAILED",
          error: error.message || "Email send failed",
        },
        status,
      );
    }
  },
};

function buildEmailMessage(payload, env) {
  const from = parseAddress(env.DEFAULT_FROM || payload.from);
  const to = parseRecipients(payload.to || payload.email);

  const headers = {
    "X-Auth0-Message-Type": safeHeaderValue(payload.messageType || "unknown"),
  };

  if (payload.userId) {
    headers["X-Auth0-User-Id"] = safeHeaderValue(payload.userId);
  }

  return {
    to,
    from,
    subject: String(payload.subject || ""),
    html: payload.html ? String(payload.html) : undefined,
    text: payload.text ? String(payload.text) : undefined,
    headers,
  };
}

function validateEmailMessage(message) {
  if (!message.to || (Array.isArray(message.to) && message.to.length === 0)) {
    return "Missing recipient";
  }

  if (!message.from) {
    return "Missing sender";
  }

  if (!message.subject) {
    return "Missing subject";
  }

  if (!message.html && !message.text) {
    return "Missing email body";
  }

  return null;
}

function parseRecipients(value) {
  if (Array.isArray(value)) {
    return value.map(parseAddress).filter(Boolean);
  }

  return parseAddress(value);
}

function parseAddress(value) {
  if (!value) {
    return null;
  }

  if (typeof value === "object" && value.email) {
    return value.name
      ? { email: String(value.email), name: String(value.name) }
      : String(value.email);
  }

  const raw = String(value).trim();
  const namedMatch = raw.match(/^"?([^"<]*)"?\s*<([^<>@\s]+@[^<>@\s]+)>$/);
  if (namedMatch) {
    const name = namedMatch[1].trim();
    const email = namedMatch[2].trim();
    return name ? { email, name } : email;
  }

  return raw;
}

function safeHeaderValue(value) {
  return String(value).replace(/[\r\n]/g, " ").slice(0, 2048);
}

function json(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: JSON_HEADERS,
  });
}
