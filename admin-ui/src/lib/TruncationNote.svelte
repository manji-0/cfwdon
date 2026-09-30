<script lang="ts">
  import { ADMIN_LIST_LIMIT } from "./format";

  export let count: number;
  export let limit = ADMIN_LIST_LIMIT;
</script>

{#if count >= limit}
  <p class="truncated">新しい順に最大 {limit} 件を表示しています。古い項目は絞り込みで確認してください。</p>
{/if}
