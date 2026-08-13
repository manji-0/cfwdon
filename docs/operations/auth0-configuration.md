# Auth0 Configuration Guide

This guide configures Auth0 as the login boundary for a deployed `cfwdon` instance. `cfwdon` still issues Mastodon-compatible OAuth app tokens locally, but user authentication is delegated to Auth0.

Auth0 setup has three moving parts:

- an Auth0 API whose identifier becomes `AUTH0_AUDIENCE`
- an Auth0 public application whose client ID becomes `AUTH0_CLIENT_ID`
- an access-token e-mail claim that matches local `cfwdon` account e-mail addresses

## Prerequisites
<!-- constrained-by ../reference/configuration.md#auth0-authentication-vars -->
<!-- constrained-by ./cloudflare-deploy.md#provisioning-steps -->

Before configuring Auth0, choose the public HTTPS origin for the instance:

```text
https://<INSTANCE_DOMAIN>
```

The Worker must be deployed on this origin, and the Auth0 callback URL must be:

```text
https://<INSTANCE_DOMAIN>/oauth/auth0/callback
```

Keep these values aligned with `INSTANCE_DOMAIN` in `wrangler.toml`.

Auth0's own references for these Dashboard fields are:

- [Application Settings](https://auth0.com/docs/get-started/applications/application-settings)
- [Configure Cross-Origin Resource Sharing](https://auth0.com/docs/get-started/applications/set-up-cors)
- [Access Tokens](https://auth0.com/docs/tokens/concepts/access-token)
- [Create Custom Claims](https://auth0.com/docs/secure/tokens/json-web-tokens/create-custom-claims)

## Create the Auth0 API
<!-- derived-from #prerequisites -->

In the Auth0 Dashboard, create an API for the `cfwdon` Worker.

| Field | Value |
| --- | --- |
| Name | `cfwdon` or the instance name |
| Identifier | `https://<INSTANCE_DOMAIN>/api` or another stable URI |
| Signing Algorithm | `RS256` |

Use the API Identifier as `AUTH0_AUDIENCE`. The Worker validates that access-token `aud` contains exactly this value and rejects tokens for other APIs.

Do not reuse the Auth0 Management API audience. `cfwdon` expects a custom API access token signed by the tenant and exposed through the tenant JWKS endpoint.

## Create the Auth0 Application
<!-- derived-from #create-the-auth0-api -->

Create an Auth0 application that can use Authorization Code with PKCE and does not require a client secret at the token endpoint. In the Dashboard this is usually a Single Page Application. If you choose another application type, confirm that Token Endpoint Authentication Method is set to `None`.

Set the application URLs:

| Auth0 setting | Value |
| --- | --- |
| Allowed Callback URLs | `https://<INSTANCE_DOMAIN>/oauth/auth0/callback` |
| Allowed Logout URLs | `https://<INSTANCE_DOMAIN>` |
| Allowed Web Origins | `https://<INSTANCE_DOMAIN>` |
| Allowed Origins (CORS) | `https://<INSTANCE_DOMAIN>` |

Avoid wildcard callback URLs in production. The Worker generates a single callback path and stores PKCE verifier state in an HttpOnly, `SameSite=Lax`, secure cookie scoped to `/oauth/auth0/callback`.

## Web UI session lifetime
<!-- derived-from #create-the-auth0-application -->
<!-- dagayn: implemented-by crates/cfwdon-worker/src/oauth_apps.rs::exchange_auth0_refresh_token -->

The browser session cookie is not a 7-day Auth0 access token. After login, `cfwdon` stores the Auth0 access token and a refresh token in HttpOnly cookies. Access tokens stay short-lived; the refresh cookie lasts **7 days** (`Max-Age=604800`) so the Web UI can mint a new access token without another login.

In the Auth0 Dashboard, enable this on the same application and API used for login:

1. On the **API**, enable **Allow Offline Access**.
2. On the **Application**, enable **Refresh Token Rotation**.
3. Set refresh token **Absolute Lifetime** and **Inactivity Lifetime** to at least 7 days (604800 seconds).
4. Set a refresh-token **Reuse Interval** of about 60 seconds so parallel Web UI requests after expiry do not revoke the token family.

Without those Dashboard settings Auth0 will not return a `refresh_token`, and the Web UI still signs out when the access token expires (often one hour or one day).

## Local development
<!-- derived-from ../getting-started/development.md#auth0-on-localhost -->

`wrangler dev` on `http://127.0.0.1:8787` (or `http://localhost:8787`) uses that origin in the Auth0 `redirect_uri`. Register these values on the Auth0 application in addition to the production URLs:

```text
http://127.0.0.1:8787/oauth/auth0/callback
http://localhost:8787/oauth/auth0/callback
```

Also add the matching logout and web-origin URLs (`http://127.0.0.1:8787`, `http://localhost:8787`). When developing the SPA with `devbox run web-ui:dev`, allow the same paths on port `5173`.

You can reuse the production Auth0 application or create a separate local-only application and set `AUTH0_CLIENT_ID` in `.dev.vars` (see `.dev.vars.example`).

Record the application Client ID as `AUTH0_CLIENT_ID`. `cfwdon` does not read or send an Auth0 client secret.

## Configure the E-mail Claim
<!-- derived-from #create-the-auth0-application -->

`cfwdon` maps Auth0 users to local accounts by comparing the configured JWT claim to `accounts.access_email`. The default claim is `email`, but Auth0 custom API access tokens often need an explicit namespaced claim.

Recommended production configuration:

```toml
AUTH0_EMAIL_CLAIM = "https://<INSTANCE_DOMAIN>/claims/email"
```

Add a Post Login Action in Auth0 that copies the verified Auth0 user e-mail into that namespaced access-token claim:

```js
exports.onExecutePostLogin = async (event, api) => {
  const namespace = "https://<INSTANCE_DOMAIN>/claims";

  if (event.user.email && event.user.email_verified) {
    api.accessToken.setCustomClaim(`${namespace}/email`, event.user.email);
    api.accessToken.setCustomClaim(`${namespace}/email_verified`, true);
  }
};
```

`cfwdon` accepts either an explicit `email_verified` / namespaced `email_verified` claim set to true, or a non-empty configured e-mail claim when those verified claims are absent (Auth0 custom API access tokens often omit `email_verified`). An explicit false verified claim is always rejected.

Use an HTTPS namespace that you control. If you keep `AUTH0_EMAIL_CLAIM = "email"`, verify with a real access token that the `email` claim is present in the access token issued for `AUTH0_AUDIENCE`; otherwise protected routes and the Auth0 callback will fail after JWT validation.

## Configure Admin Roles
<!-- derived-from #configure-the-e-mail-claim -->

`/admin` access can be granted through Auth0 roles instead of, or in addition to, `ADMIN_EMAILS`.

1. In the Auth0 API settings, enable **RBAC** and **Add Roles in the Access Token**.
2. Create an Auth0 role such as `admin` and assign it to the users who should access `/admin`.
3. Set the Worker vars:

```toml
AUTH0_ADMIN_ROLES = "admin"
# Optional when the default claim name differs from {AUTH0_AUDIENCE}/roles
# AUTH0_ROLES_CLAIM = "https://<INSTANCE_DOMAIN>/api/roles"
```

The Worker reads role names from `AUTH0_ROLES_CLAIM`. When unset, it defaults to `{AUTH0_AUDIENCE}/roles`, which matches Auth0 RBAC access tokens for a custom API.

If you prefer a Post Login Action instead of Auth0 RBAC, copy role names into the same claim namespace:

```js
exports.onExecutePostLogin = async (event, api) => {
  const namespace = "https://<INSTANCE_DOMAIN>/api";

  if (event.authorization?.roles?.length) {
    api.accessToken.setCustomClaim(
      `${namespace}/roles`,
      event.authorization.roles.map((role) => role.name),
    );
  }
};
```

`ADMIN_EMAILS` still grants `/admin` access and continues to control admin notification delivery when set.

## Configure `wrangler.toml`
<!-- derived-from #create-the-auth0-api -->
<!-- derived-from #create-the-auth0-application -->
<!-- derived-from #configure-the-e-mail-claim -->

Set the Auth0 vars under `[vars]`.

```toml
AUTH0_JWT_HEADER = "Authorization"
AUTH0_DOMAIN = "<tenant>.<region>.auth0.com"
AUTH0_CLIENT_ID = "<auth0-application-client-id>"
AUTH0_AUDIENCE = "https://<INSTANCE_DOMAIN>/api"
AUTH0_EMAIL_CLAIM = "https://<INSTANCE_DOMAIN>/claims/email"
```

`AUTH0_DOMAIN` may include or omit `https://`; the Worker normalizes it before checking `iss` and fetching `/.well-known/jwks.json`.

These Auth0 values are deployment configuration rather than secrets. Keep actual tenant values out of shared examples unless the deployment itself is public and intentional.

## Link Local Accounts
<!-- derived-from #configure-the-e-mail-claim -->

Auth0 authentication only proves the user identity. A matching local account must already exist in D1.

For each user who should log in through Auth0:

1. Confirm the Auth0 user has a verified e-mail address.
2. Confirm the configured `AUTH0_EMAIL_CLAIM` resolves to that e-mail address in the access token.
3. Confirm a local `cfwdon` account exists with the same e-mail address in `accounts.access_email`.

If Auth0 succeeds but no local account matches, `cfwdon` returns a 403 response with a registration URL and an Auth0 logout URL.

## Validate the Flow
<!-- derived-from #configure-wranglertoml -->
<!-- derived-from #link-local-accounts -->

After deploying the Worker:

1. Start a Mastodon OAuth authorization flow from a client or visit `/oauth/authorize` with a registered local OAuth client.
2. Confirm the browser is redirected to the Auth0 tenant `/authorize` endpoint.
3. Log in with a user whose e-mail maps to a local account.
4. Confirm Auth0 returns to `/oauth/auth0/callback`.
5. Confirm `cfwdon` continues to the local Mastodon OAuth consent flow and issues the local authorization code.

For direct protected API calls, send an Auth0 access token:

```sh
curl \
  -H "Authorization: Bearer <auth0-access-token>" \
  "https://<INSTANCE_DOMAIN>/api/v1/accounts/verify_credentials"
```

The token must be signed with `RS256`, have `iss` equal to the Auth0 tenant issuer, include `AUTH0_AUDIENCE` in `aud`, and contain the configured e-mail claim.

## Troubleshooting
<!-- derived-from #validate-the-flow -->

| Symptom | Check |
| --- | --- |
| Browser never returns from Auth0 | Allowed Callback URLs must include exactly `https://<INSTANCE_DOMAIN>/oauth/auth0/callback`. |
| Auth0 logout fails or ignores `returnTo` | Allowed Logout URLs must include `https://<INSTANCE_DOMAIN>`. |
| Token endpoint rejects the callback code | The Auth0 application must allow Authorization Code with PKCE and must not require a client secret. |
| Web UI signs out after about an hour | Enable API **Allow Offline Access**, application **Refresh Token Rotation**, and 7-day refresh-token lifetimes. Confirm login requests include `offline_access` and that a `cfwdon_auth0_refresh_token` cookie is set. |
| Protected routes reject a Bearer token | Confirm `AUTH0_DOMAIN`, `AUTH0_AUDIENCE`, token `iss`, token `aud`, and signing algorithm `RS256`. |
| Login succeeds but `cfwdon` returns 403 | The configured e-mail claim does not match a local `accounts.access_email`, or the local account does not exist. |
| Error says the e-mail claim is missing | Add or fix the Auth0 Post Login Action, then request a new access token for `AUTH0_AUDIENCE`. |

## Summary
<!-- derived-from #create-the-auth0-api -->
<!-- derived-from #create-the-auth0-application -->
<!-- derived-from #configure-the-e-mail-claim -->
<!-- derived-from #validate-the-flow -->

A working Auth0 deployment has a custom RS256 API identifier in `AUTH0_AUDIENCE`, a public PKCE application client ID in `AUTH0_CLIENT_ID`, exact callback/logout/origin URLs for the instance origin, and an access-token e-mail claim that maps to local `cfwdon` accounts.
