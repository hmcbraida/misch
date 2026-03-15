<script lang="ts">
	import type { ExampleProgram, ExampleProgramId } from '$lib/examplePrograms';
	import type { Theme } from '$lib/services/themeService';

	type AppHeaderProps = {
		examplePrograms: ExampleProgram[];
		selectedExampleId: ExampleProgramId | 'custom';
		isMounted: boolean;
		theme: Theme;
		isRunning: boolean;
		statusText: string;
		statusClass: string;
		onExampleProgramChange: (event: Event) => void;
		onToggleTheme: () => void;
		onRunProgram: () => void;
	};

	let {
		examplePrograms,
		selectedExampleId,
		isMounted,
		theme,
		isRunning,
		statusText,
		statusClass,
		onExampleProgramChange,
		onToggleTheme,
		onRunProgram
	}: AppHeaderProps = $props();
</script>

<header
	class="misch-header flex flex-col gap-2 rounded-none border border-border bg-surface px-4 py-3 shadow-sm backdrop-blur-sm md:flex-row md:items-start md:justify-between"
>
	<div>
		<h1 class="m-0 text-xl uppercase tracking-[0.08em] md:text-2xl">Misch</h1>
		<p class="py-1">A MIX emulator.</p>
		<p class="py-1"><a href="https://github.com/hmcbraida/misch">Source code</a></p>
	</div>
	<div class="flex flex-wrap items-center justify-end gap-2">
		<div class="flex items-center gap-2">
			<label class="text-xs font-semibold uppercase tracking-[0.04em] text-text-muted" for="example-program">
				Example
			</label>
			<select
				id="example-program"
				class="cursor-pointer rounded-none border border-border bg-bg-elevated px-2 py-1 text-sm text-text outline-none ring-accent/55 focus:ring-2"
				value={selectedExampleId}
				onchange={onExampleProgramChange}
			>
				{#if selectedExampleId === 'custom'}
					<option value="custom">--custom--</option>
				{/if}
				{#each examplePrograms as program}
					<option value={program.id}>{program.label}</option>
				{/each}
			</select>
		</div>
		<button
			type="button"
			class="cursor-pointer rounded-none border border-border-strong bg-bg-elevated px-3 py-2 text-xs font-semibold uppercase tracking-[0.04em] text-text transition hover:-translate-y-px"
			aria-label={isMounted ? (theme === 'light' ? 'Switch to dark theme' : 'Switch to light theme') : 'Toggle theme'}
			onclick={onToggleTheme}
		>
			{isMounted ? (theme === 'light' ? 'Dark Theme' : 'Light Theme') : 'Toggle Theme'}
		</button>
		<button
			type="button"
			class="misch-run-button cursor-pointer rounded-none bg-link px-4 py-2 text-sm font-semibold text-bg-elevated transition hover:-translate-y-px hover:brightness-95 disabled:cursor-wait disabled:opacity-70"
			onclick={onRunProgram}
			disabled={isRunning}
		>
			{isRunning ? 'Running...' : 'Run to Completion'}
		</button>
		<span class={`rounded-none border px-3 py-1 text-xs font-semibold ${statusClass}`}>
			{statusText}
		</span>
	</div>
</header>
