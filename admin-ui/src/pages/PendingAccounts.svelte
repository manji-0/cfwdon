<script lang="ts">
  import {
    approveAccount,
    fetchPendingAccounts,
    rejectAccount,
    type AdminPendingAccount,
  } from "../lib/api";
  import { confirmAction } from "../lib/confirm";
  import Time from "../lib/Time.svelte";

  let accounts: AdminPendingAccount[] = [];
  let loading = true;
  let error = "";
  let actionError = "";
  let notice = "";
  let actingId = "";

  async function loadAccounts() {
    loading = true;
    error = "";
    try {
      accounts = await fetchPendingAccounts();
    } catch (err) {
      accounts = [];
      error = err instanceof Error ? err.message : "failed to load pending accounts";
    } finally {
      loading = false;
    }
  }

  async function review(account: AdminPendingAccount, action: "approve" | "reject") {
    if (action === "reject") {
      const ok = await confirmAction(
        `@${account.username} を拒否して削除しますか？同じユーザー名とメールアドレスで再登録できるようになります。`,
        { title: "登録の拒否", confirmLabel: "拒否" },
      );
      if (!ok) {
        return;
      }
    }
    actingId = account.id;
    actionError = "";
    notice = "";
    try {
      if (action === "approve") {
        await approveAccount(account.id);
        notice = `@${account.username} を承認しました。`;
      } else {
        await rejectAccount(account.id);
        notice = `@${account.username} を拒否しました。`;
      }
      accounts = accounts.filter((entry) => entry.id !== account.id);
    } catch (err) {
      actionError = err instanceof Error ? err.message : `failed to ${action} account`;
      await loadAccounts();
    } finally {
      actingId = "";
    }
  }

  loadAccounts();
</script>

<section class="panel">
  <h2>承認待ちアカウント</h2>
  <p class="muted">
    承認制の登録（<code>REGISTRATIONS_MODE = "approved"</code>）や、IP ブロックの
    <code>sign_up_requires_approval</code> に該当する登録は、承認されるまで API を使えません。
  </p>

  {#if actionError}
    <p class="notice error" role="alert">{actionError}</p>
  {/if}
  {#if notice}
    <p class="notice ok" role="status">{notice}</p>
  {/if}

  {#if loading}
    <div class="loading">読み込み中…</div>
  {:else if error}
    <p class="notice error" role="alert">{error}</p>
  {:else if accounts.length === 0}
    <div class="empty">承認待ちのアカウントはありません。</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>アカウント</th>
            <th>メール</th>
            <th>登録理由</th>
            <th>登録日時</th>
            <th><span class="visually-hidden">操作</span></th>
          </tr>
        </thead>
        <tbody>
          {#each accounts as account (account.id)}
            <tr>
              <td class="mono">@{account.username}</td>
              <td>
                <span class="mono">{account.email}</span>
                {#if !account.confirmed}
                  <div class="muted">未確認</div>
                {/if}
              </td>
              <td>{account.invite_request ?? ""}</td>
              <td><Time value={account.created_at} /></td>
              <td>
                <div class="row-actions">
                  <button
                    type="button"
                    class="btn btn-primary"
                    disabled={actingId === account.id}
                    on:click={() => review(account, "approve")}
                  >
                    承認
                  </button>
                  <button
                    type="button"
                    class="btn btn-danger"
                    disabled={actingId === account.id}
                    on:click={() => review(account, "reject")}
                  >
                    拒否
                  </button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
