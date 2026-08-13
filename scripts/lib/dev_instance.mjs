#!/usr/bin/env node
/**
 * Normalize a Mastodon/cfwdon instance reference to an HTTPS origin.
 *
 *   fedi.manji.app              -> https://fedi.manji.app
 *   https://mastodon.social/    -> https://mastodon.social
 *   http://127.0.0.1:8787       -> http://127.0.0.1:8787
 */
export function normalizeInstanceOrigin(input) {
  const trimmed = input.trim().replace(/\/+$/, "");
  if (!trimmed) {
    throw new Error("instance origin is empty");
  }
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed;
  }
  return `https://${trimmed}`;
}

export function instanceDomainFromOrigin(origin) {
  const url = new URL(origin);
  return url.host.includes(":") ? url.host : url.hostname;
}

export function parseDevArgs(argv) {
  const args = [...argv];
  let remote = false;
  let instance;
  let skipWebUiBuild = false;

  while (args.length > 0) {
    const current = args.shift();
    if (current === "--remote") {
      remote = true;
      continue;
    }
    if (current === "--skip-web-ui-build") {
      skipWebUiBuild = true;
      continue;
    }
    if (current === "--instance") {
      const value = args.shift();
      if (!value) {
        throw new Error("--instance requires a domain or URL");
      }
      instance = value;
      continue;
    }
    if (current === "--help" || current === "-h") {
      return { help: true, remote, instance, skipWebUiBuild };
    }
    if (current.startsWith("-")) {
      throw new Error(`unknown option: ${current}`);
    }
    if (!instance) {
      instance = current;
      continue;
    }
    throw new Error(`unexpected argument: ${current}`);
  }

  return { help: false, remote, instance, skipWebUiBuild };
}

export function workerDevUsage() {
  return `Usage:
  devbox run worker:dev
  devbox run worker:dev -- --remote
  devbox run worker:dev -- --instance fedi.manji.app
  devbox run worker:dev -- --instance https://fedi.manji.app --remote

Options:
  --instance <domain-or-url>  Override INSTANCE_DOMAIN (and related Auth0 vars)
  --remote                    Use remote Cloudflare bindings (D1/KV/R2)
  --skip-web-ui-build         Skip rebuilding web-ui/dist before start
`;
}

export function webUiDevUsage() {
  return `Usage:
  devbox run web-ui:dev
  devbox run web-ui:dev -- --instance https://fedi.manji.app

Options:
  --instance <domain-or-url>  Proxy API/auth routes to this origin
                              (default: http://127.0.0.1:8787)
`;
}
