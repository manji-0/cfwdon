<script lang="ts">
  import { tick } from "svelte";
  import { confirmRequest, type ConfirmRequest } from "./confirm";

  let dialog: HTMLDialogElement;
  let cancelButton: HTMLButtonElement;
  let current: ConfirmRequest | null = null;

  // Native showModal() gives us the focus trap, Escape handling and focus restore.
  $: void open($confirmRequest);

  async function open(request: ConfirmRequest | null) {
    current = request;
    if (!request || !dialog) {
      return;
    }
    await tick();
    if (!dialog.open) {
      dialog.showModal();
    }
    // Destructive actions start on Cancel so Enter does not run them.
    if (request.danger) {
      cancelButton?.focus();
    }
  }

  // Clicks on the dialog's own padding also target the <dialog>, so compare
  // against its box to treat only clicks on the backdrop as cancel.
  function onBackdropClick(event: MouseEvent) {
    if (event.target !== dialog) {
      return;
    }
    const rect = dialog.getBoundingClientRect();
    const inside =
      event.clientX >= rect.left &&
      event.clientX <= rect.right &&
      event.clientY >= rect.top &&
      event.clientY <= rect.bottom;
    if (!inside) {
      settle(false);
    }
  }

  function settle(ok: boolean) {
    const request = current;
    if (!request) {
      return;
    }
    confirmRequest.set(null);
    current = null;
    if (dialog?.open) {
      dialog.close();
    }
    request.resolve(ok);
  }
</script>

<dialog
  bind:this={dialog}
  class="confirm-dialog"
  aria-labelledby="confirm-dialog-title"
  on:cancel|preventDefault={() => settle(false)}
  on:click={onBackdropClick}
>
  {#if current}
    <h2 id="confirm-dialog-title">{current.title}</h2>
    <p>{current.message}</p>
    <div class="actions">
      <button type="button" class="btn" bind:this={cancelButton} on:click={() => settle(false)}>
        キャンセル
      </button>
      <button
        type="button"
        class="btn"
        class:btn-danger={current.danger}
        class:btn-primary={!current.danger}
        on:click={() => settle(true)}
      >
        {current.confirmLabel}
      </button>
    </div>
  {/if}
</dialog>

<style>
  .confirm-dialog {
    width: min(28rem, calc(100vw - 2rem));
    padding: 1.25rem;
    border: 1px solid var(--border);
    border-radius: 0.75rem;
    background: var(--panel);
    color: var(--text);
  }

  .confirm-dialog::backdrop {
    background: rgba(0, 0, 0, 0.55);
  }

  h2 {
    margin: 0 0 0.5rem;
    font-size: 1.05rem;
  }

  p {
    margin: 0 0 1.25rem;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
