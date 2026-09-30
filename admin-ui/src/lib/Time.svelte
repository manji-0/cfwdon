<script lang="ts">
  import { formatDateTime, isoDateTime } from "./format";

  export let value: string | null | undefined;
</script>

{#if value}
  <time datetime={isoDateTime(value)} title={value}>{formatDateTime(value)}</time>
{:else}
  <span class="muted">—</span>
{/if}
