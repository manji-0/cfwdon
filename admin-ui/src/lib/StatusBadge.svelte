<script lang="ts">
  import type { StatusLabel } from "./format";

  export let status: StatusLabel;
</script>

<span class="badge {status.tone}">{status.label}</span>
