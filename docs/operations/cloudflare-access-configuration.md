# Cloudflare Access Configuration Guide

This guide configures Cloudflare Access in front of a deployed `cfwdon` Worker. Access can protect who reaches the hostname before the request enters the Worker. It is not currently a replacement for the Auth0 JWTs that `cfwdon` validates for local user authentication.

Use this guide when you want one of these deployment shapes:

- protect a staging or private `cfwdon` hostname with Cloudflare Access
- require Cloudflare Access before the Auth0-backed Mastodon OAuth flow
- allow automation through Access with service tokens while still using `cfwdon` auth for protected API routes

Do not put a public federated production hostname behind a blanket Access policy unless you intentionally want WebFinger, ActivityPub, public timelines, media fallback routes, and Mastodon client discovery to be private.

## Current Integration Boundary
<!-- constrained-by ../reference/configuration.md#auth0-authentication-vars -->
<!-- constrained-by ./auth0-configuration.md -->

Cloudflare Access and Auth0 sit at different layers:

| Layer | Product | What it proves | What `cfwdon` does with it |
| --- | --- | --- | --- |
| Edge access gate | Cloudflare Access | The request satisfied an Access policy before reaching the Worker. | Current code does not map Access identity to a local account. |
| Application authentication | Auth0 | The request has an Auth0-issued RS256 JWT with the configured issuer, audience, and e-mail claim. | Protected routes map the claim to `accounts.access_email`. |
| Mastodon client authorization | `cfwdon` local OAuth | The client has a local app token and scopes. | Mastodon API routes use local OAuth tokens where supported. |

Cloudflare Access forwards an application token in the `Cf-Access-Jwt-Assertion` request header and, for browser requests, also uses a `CF_Authorization` cookie. `cfwdon` does not currently validate `Cf-Access-Jwt-Assertion`; keep `AUTH0_DOMAIN`, `AUTH0_CLIENT_ID`, `AUTH0_AUDIENCE`, and `AUTH0_EMAIL_CLAIM` configured if protected Mastodon API routes or browser OAuth login should work.

Cloudflare's own references for these fields and behaviors are:

- [Choose an application type](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/choose-application-type/)
- [Publish a self-hosted application](https://developers.cloudflare.com/cloudflare-one/applications/configure-apps/self-hosted-apps/)
- [Access policies](https://developers.cloudflare.com/cloudflare-one/policies/access/)
- [Validate JWTs](https://developers.cloudflare.com/cloudflare-one/identity/authorization-cookie/validating-json/)
- [Application token](https://developers.cloudflare.com/cloudflare-one/identity/authorization-cookie/application-token/)
- [Service tokens](https://developers.cloudflare.com/cloudflare-one/access-controls/service-credentials/service-tokens/)

## Choose What Access Protects
<!-- derived-from #current-integration-boundary -->

Pick the smallest hostname or path set that matches the deployment goal.

| Goal | Recommended Access scope | Notes |
| --- | --- | --- |
| Private staging instance | Whole staging hostname, for example `staging.example.com` | Safe because federation and public discovery are expected to be private. |
| Private single-user instance | Whole production hostname | Mastodon clients and federation peers outside Access will not reach the instance. |
| Public federated instance with private operations | Separate operations hostname or narrow private paths | Keep public discovery, ActivityPub, media, and Mastodon API discovery reachable. |
| Automation through Access | Same Access app plus a Service Auth policy | Service tokens only pass the Access gate; protected `cfwdon` APIs still need application auth. |

For normal public federation, avoid protecting these surfaces with Access:

- `/.well-known/webfinger`
- `/.well-known/nodeinfo`
- `/nodeinfo/*`
- `/users/*`
- `/inbox`
- `/api/v1/instance`
- `/api/v2/instance`
- `/oauth/*` when Mastodon client login must be public
- `/media/*` when using Worker media fallback routes

Prefer a separate staging hostname for full Access protection. It avoids trying to maintain a long list of path exceptions for federation and client compatibility.

## Create the Access Application
<!-- derived-from #choose-what-access-protects -->

In Cloudflare Zero Trust:

1. Go to **Access controls > Applications**.
2. Select **Add an application**.
3. Choose **Self-hosted**.
4. Name the application, for example `cfwdon-staging`.
5. Choose the session duration.
6. Add the public hostname that should be protected, for example `staging.example.com`.

For a Worker on a proxied Cloudflare DNS hostname, Access evaluates the request before forwarding it to the Worker route.

After creating the application, copy the **Application Audience (AUD) Tag** from the application settings. You do not need this value for current `cfwdon` runtime configuration, but keep it in private operational notes if you later add Worker-side Access JWT validation.

## Configure Access Policies
<!-- derived-from #create-the-access-application -->

Add at least one policy. Access applications are deny-by-default unless a request matches an Allow, Bypass, or Service Auth policy.

For human browser access:

| Policy field | Recommended value |
| --- | --- |
| Action | `Allow` |
| Include | Your user e-mails, e-mail domain, IdP group, or Access group |
| Require | Device posture or country rules, if needed |
| Exclude | Break-glass exclusions, if needed |

For automation:

| Policy field | Recommended value |
| --- | --- |
| Action | `Service Auth` |
| Include | Specific service token |

Keep broad rules such as "Everyone" or "All valid emails" out of production unless the hostname is intentionally public behind an audit gate.

## Keep Auth0 Configured
<!-- derived-from #current-integration-boundary -->
<!-- derived-from ./auth0-configuration.md#configure-wranglertoml -->

If users should sign in to `cfwdon` through Mastodon OAuth, keep the Auth0 configuration from [Auth0 Configuration Guide](./auth0-configuration.md):

```toml
AUTH0_JWT_HEADER = "Authorization"
AUTH0_DOMAIN = "<tenant>.<region>.auth0.com"
AUTH0_CLIENT_ID = "<auth0-application-client-id>"
AUTH0_AUDIENCE = "https://<INSTANCE_DOMAIN>/api"
AUTH0_EMAIL_CLAIM = "https://<INSTANCE_DOMAIN>/claims/email"
```

Do not set `AUTH0_DOMAIN` to a Cloudflare Access team domain, and do not set `AUTH0_AUDIENCE` to an Access AUD tag. Access application tokens and Auth0 API access tokens have different issuers, audiences, and identity claims.

When Access protects the same hostname as Auth0 browser login, update the Auth0 application URLs to match the protected origin:

- Allowed Callback URLs: `https://<INSTANCE_DOMAIN>/oauth/auth0/callback`
- Allowed Logout URLs: `https://<INSTANCE_DOMAIN>`
- Allowed Web Origins: `https://<INSTANCE_DOMAIN>`
- Allowed Origins (CORS): `https://<INSTANCE_DOMAIN>`

The browser will pass Access first, then Auth0, then the local `cfwdon` OAuth consent flow.

## Add Service Tokens for Automation
<!-- derived-from #configure-access-policies -->

Use service tokens for CI, smoke tests, or trusted jobs that need to pass the Access gate without an interactive identity provider login.

1. In Cloudflare Zero Trust, go to **Access controls > Service credentials > Service Tokens**.
2. Create a service token with a clear name and expiration.
3. Copy the Client ID and Client Secret immediately; the secret is shown only once.
4. Add a `Service Auth` policy for that token on the Access application.

Initial requests can include the token headers:

```sh
curl \
  -H "CF-Access-Client-Id: <client-id>" \
  -H "CF-Access-Client-Secret: <client-secret>" \
  "https://<INSTANCE_DOMAIN>/api/v1/instance"
```

For protected `cfwdon` API routes, the request must also satisfy `cfwdon` authentication. Passing Cloudflare Access alone does not create a local account session.

```sh
curl \
  -H "CF-Access-Client-Id: <client-id>" \
  -H "CF-Access-Client-Secret: <client-secret>" \
  -H "Authorization: Bearer <auth0-access-token-or-local-oauth-token>" \
  "https://<INSTANCE_DOMAIN>/api/v1/accounts/verify_credentials"
```

Store service token secrets in the CI secret manager. Do not commit them to `wrangler.toml`, docs, scripts, or examples.

## Validate the Flow
<!-- derived-from #create-the-access-application -->
<!-- derived-from #configure-access-policies -->
<!-- derived-from #keep-auth0-configured -->

For a browser user:

1. Open `https://<INSTANCE_DOMAIN>`.
2. Confirm Cloudflare Access prompts for the configured identity provider or accepts the existing Access session.
3. Start a Mastodon OAuth authorization flow.
4. Confirm the browser redirects to Auth0 and returns to `/oauth/auth0/callback`.
5. Confirm `cfwdon` maps the Auth0 e-mail claim to a local account and continues the local OAuth flow.

For a direct request:

1. Send a request without Access credentials and confirm Access blocks it.
2. Send a request with Access service token headers and confirm public `cfwdon` endpoints respond.
3. Send a protected API request with both Access credentials and `cfwdon` auth credentials.
4. Confirm federation-critical public routes remain reachable if this is a public instance.

## Future Worker-Side Access JWT Support
<!-- derived-from #current-integration-boundary -->

If `cfwdon` later supports Cloudflare Access as an application authentication provider, the Worker should validate the Access JWT instead of trusting the header by presence alone.

Required checks would include:

- read `Cf-Access-Jwt-Assertion`
- verify the JWT signature against `https://<team-name>.cloudflareaccess.com/cdn-cgi/access/certs`
- validate `iss` against the Cloudflare Access team domain
- validate `aud` against the Access Application Audience tag
- map a stable e-mail claim to `accounts.access_email`
- preserve Auth0 and local OAuth behavior for existing deployments

Until that support exists, Cloudflare Access remains an edge gate for `cfwdon`, not the local user identity provider.

## Troubleshooting
<!-- derived-from #validate-the-flow -->

| Symptom | Check |
| --- | --- |
| Federation or WebFinger stops working | The production hostname is likely covered by a blanket Access policy. Move Access to staging or narrow the protected paths. |
| Mastodon clients cannot start OAuth | `/oauth/*` may be behind Access, or the client cannot follow the Access browser login flow. |
| Auth0 callback is blocked | The browser must pass Access on `https://<INSTANCE_DOMAIN>/oauth/auth0/callback`, and Auth0 must allow that callback URL. |
| Protected API still returns unauthorized after Access login | Add a valid Auth0 access token or local OAuth token. Access alone is not `cfwdon` user auth. |
| Service token works for public routes but not protected routes | The service token only satisfies Cloudflare Access. Send `cfwdon` auth credentials too. |
| Worker needs to trust Access identity | Implement Worker-side Access JWT validation first; do not trust `Cf-Access-Jwt-Assertion` without signature, issuer, and audience checks. |

## Summary
<!-- derived-from #current-integration-boundary -->
<!-- derived-from #choose-what-access-protects -->
<!-- derived-from #keep-auth0-configured -->
<!-- derived-from #validate-the-flow -->

Cloudflare Access is useful for staging, private deployments, and an extra edge gate in front of `cfwdon`. For current code, keep Auth0 configured for local user authentication, avoid blanket Access protection on public federated hostnames, and use service tokens only as Access credentials for automation.
