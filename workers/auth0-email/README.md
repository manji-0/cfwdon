# Auth0 Email Worker

Small Cloudflare Worker used by Auth0 Custom Email Provider Actions to send
rendered Auth0 notification emails through Cloudflare Email Service.

The Worker intentionally uses `DEFAULT_FROM` as the sender, even if Auth0 sends
`event.notification.from`, so Auth0 dashboard sender settings cannot select a
domain that Cloudflare Email Service has not authorized.

## Deploy

```sh
npm install
npm run deploy
```

Set the shared bearer token before production use:

```sh
printf '%s' '<token>' | npx wrangler secret put AUTH0_EMAIL_TOKEN
```

Auth0 Custom Email Provider secrets:

```txt
CLOUDFLARE_EMAIL_WORKER_URL=https://sendmail.manji.app/send-auth0-email
CLOUDFLARE_EMAIL_WORKER_TOKEN=<token>
```

Paste `auth0-action.js` into the Auth0 Custom Email Provider Action.
