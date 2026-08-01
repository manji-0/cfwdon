# nixbuild.net Setup

[nixbuild.net](https://nixbuild.net/) offloads Nix builds in CI and locally. In this repository it targets **devbox package installation** (for example `wrangler` when the GitHub Actions Nix store cache misses). It does **not** speed up `cargo test`, `cargo clippy`, or `wrangler deploy --dry-run` Rust builds — those stay on `rust-cache` and the runner.

For the CI workflow shape, see [Development Workflow](../getting-started/development.md).

## What You Need

| Item | CI (GitHub Actions) | Local (optional) |
|------|---------------------|------------------|
| nixbuild.net account | Yes | Yes (same account as CI) |
| Auth token | Yes — repository secret | No (use SSH key instead) |
| Ed25519 SSH key | No | Yes — added in nixbuild.net dashboard |
| GitHub secret name | `NIXBUILD_TOKEN` | — |

Registration is free to start (25 CPU hours/month, no credit card). Billing details are only required after the free quota is used.

## 1. Create an Account

1. Register at [nixbuild.net](https://nixbuild.net/#register) with your email.
2. Confirm the activation link from your inbox.
3. Add at least one **Ed25519** public SSH key on the SSH keys settings page (needed for the admin shell and for local builds).

Generate a key if you do not have one:

```sh
ssh-keygen -t ed25519 -C "cfwdon-nixbuild" -f ~/.ssh/nixbuild_ed25519
```

Upload the **full line** from `~/.ssh/nixbuild_ed25519.pub` to nixbuild.net (type, base64 key, and trailing comment). nixbuild.net requires a **unique key label** (the OpenSSH comment after the key material, or the `KEY_NAME` when using `ssh-keys add`). A bare `ssh-ed25519 AAAA...` line without a label is rejected as invalid.

```text
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI... cfwdon-nixbuild
```

Keep the private key out of the repository.

Verify shell access:

```sh
ssh -i ~/.ssh/nixbuild_ed25519 eu.nixbuild.net shell
```

## 2. CI: Create and Store an Auth Token

In the nixbuild.net shell:

```text
tokens create -p build:read -p build:write -p store:read -p store:write
```

Copy the token (shown once). Create a GitHub repository secret:

- **Name:** `NIXBUILD_TOKEN`
- **Value:** the token string from `tokens create`

The workflow in [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) uses this secret. When it is unset (for example on forks), CI falls back to the previous behaviour: devbox installs its own Nix and builds on the runner.

After the next CI run with the secret set, check the **Configure nixbuild.net** step summary for CPU time used.

## 3. Local: SSH and Nix Configuration

Local setup is optional. It lets your machine reuse build results from CI (and vice versa) when devbox triggers a Linux Nix build.

nixbuild.net runs **Linux** builds (`x86_64-linux`, `aarch64-linux`). On macOS, devbox usually installs `aarch64-darwin` packages from `cache.nixos.org` without remote builders; local nixbuild is still useful if you want shared artifacts with CI or run Linux-targeted Nix work.

### SSH client (`~/.ssh/config`)

```sshconfig
Host eu.nixbuild.net
  HostName eu.nixbuild.net
  IdentityFile ~/.ssh/nixbuild_ed25519
  IdentitiesOnly yes
  PubkeyAcceptedKeyTypes ssh-ed25519
  ServerAliveInterval 60
```

Add the host key (required for non-interactive Nix):

```sh
mkdir -p ~/.ssh
chmod 700 ~/.ssh
if ! grep -q 'eu.nixbuild.net' ~/.ssh/known_hosts 2>/dev/null; then
  printf '%s\n' \
    'eu.nixbuild.net ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPIQCZc54poJ8vqawd8TraNryQeJnvH1eLpIDgbiqymM' \
    >> ~/.ssh/known_hosts
fi
```

On Linux with `nix-daemon`, repeat the same `Host` block and `known_hosts` entry for **root** (`/root/.ssh/`), because the daemon connects to remote builders.

### Nix (`~/.config/nix/nix.conf`)

Append (or merge with existing settings):

```ini
builders = ssh-ng://eu.nixbuild.net x86_64-linux,aarch64-linux - 100 1 big-parallel,benchmark
builders-use-substitutes = true
```

If you use a system-wide Nix install with `/etc/nix/nix.conf`, put the same lines there instead.

### Smoke test

From a machine with Nix available (inside `devbox shell` is fine):

```sh
nix-build \
  --max-jobs 0 \
  --builders "ssh-ng://eu.nixbuild.net x86_64-linux - 100 1 big-parallel,benchmark" \
  -E 'with import <nixpkgs> {}; runCommand "nixbuild-smoke" {} "echo ok > $out"'
```

You should see output containing `building '...' on 'ssh-ng://eu.nixbuild.net'...` or a substituter fetch from nixbuild.net.

### devbox

No `devbox.json` changes are required. After the SSH and Nix settings above, `devbox install` and `devbox shell` use the same remote builder configuration as plain Nix.

## Cost and Monitoring

- **Free tier:** 25 CPU hours per month per account.
- **Paid:** about €0.12 per CPU hour after the free quota ([pricing](https://nixbuild.net/#pricing)).
- **Monitor usage:** `ssh eu.nixbuild.net shell`, then `usage`.

Typical devbox CI installs only spend noticeable CPU time when packages such as `wrangler` are built from source (Nix store cache miss). Warm runs that only copy from `cache.nixos.org` or nixbuild substituters use little or no build time.

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| CI still shows `building '...wrangler...'` on the runner | `NIXBUILD_TOKEN` missing or invalid | Set the repository secret; re-run workflow |
| `Invalid` / rejected SSH public key | Key pasted without comment/label | Use the full `.pub` line, including the trailing label (for example `cfwdon-nixbuild`) |
| Builds run locally on Linux despite config | `nix-daemon` uses root SSH config | Configure `/root/.ssh/config` and `known_hosts` |
| Fork PR CI is slow | Secrets are not available on forks | Expected; upstream repo with secret configured is fast |

## References

- [nixbuild.net Getting Started](https://docs.nixbuild.net/getting-started/)
- [Remote builds (builders vs remote store)](https://docs.nixbuild.net/remote-builds/)
- [nixbuild-action](https://github.com/nixbuild/nixbuild-action)
- [devbox-install-action](https://github.com/jetify-com/devbox-install-action)
