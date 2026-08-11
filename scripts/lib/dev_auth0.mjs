export const LOCAL_DEV_ORIGINS = [
  "http://127.0.0.1:8787",
  "http://localhost:8787",
  "http://127.0.0.1:5173",
  "http://localhost:5173",
] as const;

export function localAuth0CallbackUrls() {
  return LOCAL_DEV_ORIGINS.map((origin) => `${origin}/oauth/auth0/callback`);
}

export function printLocalAuth0Setup() {
  const callbacks = localAuth0CallbackUrls();
  const origins = [...LOCAL_DEV_ORIGINS];
  process.stdout.write(`
Local Auth0 setup (one-time in the Auth0 application):
  Allowed Callback URLs:
    ${callbacks.join("\n    ")}
  Allowed Logout URLs:
    ${origins.join("\n    ")}
  Allowed Web Origins / CORS:
    ${origins.join("\n    ")}

Use the same Auth0 application as production, or set AUTH0_CLIENT_ID in .dev.vars
for a dedicated local-dev application.
`);
}
