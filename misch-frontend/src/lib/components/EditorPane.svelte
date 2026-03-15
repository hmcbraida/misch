<script lang="ts">
let {
  title,
  value = $bindable(),
}: {
  title: string;
  value: string;
} = $props();
</script>

<section class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-none border border-border bg-surface">
	<div class="border-b border-border bg-bg-elevated px-3 py-2">
		<h2 class="m-0 text-sm uppercase tracking-[0.03em] text-text-muted">{title}</h2>
	</div>
	<textarea
		class="h-full w-full flex-1 resize-none overflow-auto border-none bg-bg-elevated px-3 py-3 font-['IBM_Plex_Mono','Menlo','Consolas',monospace] text-sm leading-6 text-text outline-none"
		bind:value
		spellcheck="false"
	></textarea>
</section>
