<script lang="ts">
const { outputText }: { outputText: string } = $props();
</script>

<section
	class="flex min-h-[8rem] flex-col overflow-hidden rounded-none border border-border bg-surface"
>
	<div class="border-b border-border bg-bg-elevated px-3 py-2">
		<h2 class="m-0 text-sm uppercase tracking-[0.03em] text-text-muted">Line Writer Output (Unit 18)</h2>
	</div>
	<pre
		class="m-0 h-full w-full overflow-auto bg-bg-elevated px-3 py-3 font-['IBM_Plex_Mono','Menlo','Consolas',monospace] text-sm leading-6 whitespace-pre text-text"
	>{outputText || 'Program output will appear here after running.'}</pre>
</section>
