<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { env } from '$env/dynamic/public';
	import {
		DEFAULT_EXAMPLE_PROGRAM_ID,
		EXAMPLE_PROGRAMS,
		EXAMPLE_PROGRAMS_BY_ID,
		type ExampleProgram,
		type ExampleProgramId
	} from '$lib/examplePrograms';

	type UiStatus = 'idle' | 'running' | 'success' | 'error';
	type DragMode = 'horizontal' | 'vertical';
	type Theme = 'light' | 'dark';

	type CreateSessionResponse = {
		session_id: string;
		halted: boolean;
	};

	type OutputTextResponse = {
		units: Record<string, string>;
	};

	const API_BASE = env.PUBLIC_API_BASE || '/api/v1';
	const PAPER_TAPE_UNIT = 16;
	const LINE_WRITER_UNIT = 18;
	const DEFAULT_BLOCK_SIZE = 1;
	const LINE_WRAP = 100;
	const THEME_STORAGE_KEY = 'misch-theme';

	function resolveInitialTheme(): Theme {
		if (typeof document === 'undefined') {
			return 'light';
		}

		const datasetTheme = document.documentElement.dataset.theme;
		if (datasetTheme === 'light' || datasetTheme === 'dark') {
			return datasetTheme;
		}

		return 'light';
	}

	const exampleProgramById: Record<ExampleProgramId, ExampleProgram> = EXAMPLE_PROGRAMS_BY_ID;

	const defaultExample = exampleProgramById[DEFAULT_EXAMPLE_PROGRAM_ID];

	let assembly = $state(defaultExample.assembly);
	let paperTapeInput = $state(defaultExample.paperTapeInput);
	let lineWriterOutput = $state('');
	let errorMessage = $state('');
	let status = $state<UiStatus>('idle');
	let topPanePercent = $state(74);
	let leftPanePercent = $state(56);
	let dragMode = $state<DragMode | null>(null);
	let theme = $state<Theme>(resolveInitialTheme());
	let isMounted = $state(false);
	let workspacePane: HTMLDivElement | null = null;
	let editorsPane: HTMLDivElement | null = null;

	const statusLabel: Record<UiStatus, string> = {
		idle: 'Idle',
		running: 'Running',
		success: 'Completed',
		error: 'Failed'
	};

	const statusPillClass: Record<UiStatus, string> = {
		idle: 'border-border bg-bg-elevated text-text-muted',
		running: 'border-link/65 bg-link/15 text-text',
		success: 'border-link-hover/65 bg-link-hover/15 text-text',
		error: 'border-border-strong bg-bg-elevated text-text'
	};

	function applyTheme(nextTheme: Theme): void {
		theme = nextTheme;
		if (typeof document === 'undefined') {
			return;
		}
		document.documentElement.dataset.theme = nextTheme;
		document.documentElement.style.colorScheme = nextTheme;
	}

	function toggleTheme(): void {
		const nextTheme: Theme = theme === 'light' ? 'dark' : 'light';
		applyTheme(nextTheme);

		if (typeof window === 'undefined') {
			return;
		}

		try {
			window.localStorage.setItem(THEME_STORAGE_KEY, nextTheme);
		} catch {
			// best effort persistence only
		}
	}

	onMount(() => {
		isMounted = true;

		if (typeof window === 'undefined') {
			return;
		}

		const datasetTheme = document.documentElement.dataset.theme;
		if (datasetTheme === 'light' || datasetTheme === 'dark') {
			applyTheme(datasetTheme);
			return;
		}

		let initialTheme: Theme = window.matchMedia('(prefers-color-scheme: dark)').matches
			? 'dark'
			: 'light';

		try {
			const storedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
			if (storedTheme === 'light' || storedTheme === 'dark') {
				initialTheme = storedTheme;
			}
		} catch {
			// continue with system default
		}

		applyTheme(initialTheme);
	});

	function findMatchingExampleProgramId(): ExampleProgramId | null {
		for (const program of EXAMPLE_PROGRAMS) {
			if (assembly === program.assembly && paperTapeInput === program.paperTapeInput) {
				return program.id;
			}
		}

		return null;
	}

	const selectedExampleId = $derived<ExampleProgramId | 'custom'>(
		findMatchingExampleProgramId() ?? 'custom'
	);

	function setExampleProgram(nextExampleId: ExampleProgramId): void {
		const selectedProgram = exampleProgramById[nextExampleId];
		assembly = selectedProgram.assembly;
		paperTapeInput = selectedProgram.paperTapeInput;
		errorMessage = '';
	}

	function onExampleProgramChange(event: Event): void {
		const target = event.currentTarget;
		if (!(target instanceof HTMLSelectElement)) {
			return;
		}

		const nextValue = target.value as ExampleProgramId | 'custom';
		if (nextValue === 'custom') {
			return;
		}

		if (
			selectedExampleId === 'custom' &&
			typeof window !== 'undefined' &&
			!window.confirm('You have custom edits. Discard them and load this example?')
		) {
			target.value = 'custom';
			return;
		}

		setExampleProgram(nextValue);
	}

	function clamp(value: number, min: number, max: number): number {
		return Math.min(max, Math.max(min, value));
	}

	function onDragMove(event: PointerEvent): void {
		if (dragMode === 'horizontal' && workspacePane) {
			const rect = workspacePane.getBoundingClientRect();
			if (rect.height > 0) {
				const next = ((event.clientY - rect.top) / rect.height) * 100;
				topPanePercent = clamp(next, 35, 85);
			}
		}

		if (dragMode === 'vertical' && editorsPane) {
			const rect = editorsPane.getBoundingClientRect();
			if (rect.width > 0) {
				const next = ((event.clientX - rect.left) / rect.width) * 100;
				leftPanePercent = clamp(next, 20, 80);
			}
		}
	}

	function stopDragging(): void {
		dragMode = null;
		if (typeof window === 'undefined' || typeof document === 'undefined') {
			return;
		}
		window.removeEventListener('pointermove', onDragMove);
		window.removeEventListener('pointerup', stopDragging);
		document.body.style.userSelect = '';
		document.body.style.cursor = '';
	}

	function startDrag(mode: DragMode): void {
		if (typeof window === 'undefined' || typeof document === 'undefined') {
			return;
		}
		dragMode = mode;
		document.body.style.userSelect = 'none';
		document.body.style.cursor = mode === 'horizontal' ? 'row-resize' : 'col-resize';
		window.addEventListener('pointermove', onDragMove);
		window.addEventListener('pointerup', stopDragging);
	}

	onDestroy(() => {
		stopDragging();
	});

	function wrapLineWriterOutput(text: string): string {
		if (!text) {
			return '';
		}

		const wrappedLines: string[] = [];
		for (const line of text.split('\n')) {
			if (line.length === 0) {
				wrappedLines.push('');
				continue;
			}
			for (let i = 0; i < line.length; i += LINE_WRAP) {
				wrappedLines.push(line.slice(i, i + LINE_WRAP));
			}
		}

		return wrappedLines.join('\n');
	}

	async function readApiError(response: Response): Promise<string> {
		const fallback = `Request failed with status ${response.status}`;
		const contentType = response.headers.get('content-type') ?? '';

		if (contentType.includes('application/json')) {
			const data = (await response.json()) as { error?: string };
			return data.error ?? fallback;
		}

		const body = await response.text();
		return body.trim() || fallback;
	}

	async function runProgram(): Promise<void> {
		status = 'running';
		errorMessage = '';
		lineWriterOutput = '';

		let sessionId: string | null = null;

		try {
			const createResponse = await fetch(`${API_BASE}/sessions`, {
				method: 'POST',
				headers: {
					'content-type': 'application/json'
				},
				body: JSON.stringify({
					assembly,
					input_devices: [{ unit: PAPER_TAPE_UNIT, block_size: DEFAULT_BLOCK_SIZE }],
					output_devices: [{ unit: LINE_WRITER_UNIT, block_size: DEFAULT_BLOCK_SIZE }]
				})
			});

			if (!createResponse.ok) {
				throw new Error(await readApiError(createResponse));
			}

			const createSession = (await createResponse.json()) as CreateSessionResponse;
			sessionId = createSession.session_id;

			const inputResponse = await fetch(`${API_BASE}/sessions/${sessionId}/io/input/text`, {
				method: 'POST',
				headers: {
					'content-type': 'application/json'
				},
				body: JSON.stringify({
					unit: PAPER_TAPE_UNIT,
					text: paperTapeInput
				})
			});

			if (!inputResponse.ok) {
				throw new Error(await readApiError(inputResponse));
			}

			const runResponse = await fetch(`${API_BASE}/sessions/${sessionId}/run`, {
				method: 'POST'
			});

			if (!runResponse.ok) {
				throw new Error(await readApiError(runResponse));
			}

			const outputResponse = await fetch(
				`${API_BASE}/sessions/${sessionId}/io/output/text?unit=${LINE_WRITER_UNIT}`
			);

			if (!outputResponse.ok) {
				throw new Error(await readApiError(outputResponse));
			}

			const output = (await outputResponse.json()) as OutputTextResponse;
			lineWriterOutput = wrapLineWriterOutput(output.units[String(LINE_WRITER_UNIT)] ?? '');
			status = 'success';
		} catch (err) {
			status = 'error';
			errorMessage = err instanceof Error ? err.message : 'Unknown error while running program';
		} finally {
			if (sessionId) {
				try {
					await fetch(`${API_BASE}/sessions/${sessionId}`, { method: 'DELETE' });
				} catch {
					// best effort cleanup only
				}
			}
		}
	}
</script>

<div
	class="misch-shell h-dvh overflow-hidden text-text"
>
	<div
		class="mx-auto flex h-full min-h-0 w-full max-w-[1400px] flex-col gap-3 p-3 font-['Avenir_Next','Segoe_UI','Gill_Sans',sans-serif] md:p-4"
	>
		<header
			class="misch-header flex flex-col gap-2 rounded-none border border-border bg-surface px-4 py-3 shadow-sm backdrop-blur-sm md:flex-row md:items-center md:justify-between"
		>
			<h1 class="m-0 text-xl uppercase tracking-[0.08em] md:text-2xl">Misch</h1>
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
						{#each EXAMPLE_PROGRAMS as program}
							<option value={program.id}>{program.label}</option>
						{/each}
					</select>
				</div>
				<button
					type="button"
					class="cursor-pointer rounded-none border border-border-strong bg-bg-elevated px-3 py-2 text-xs font-semibold uppercase tracking-[0.04em] text-text transition hover:-translate-y-px"
					aria-label={isMounted ? (theme === 'light' ? 'Switch to dark theme' : 'Switch to light theme') : 'Toggle theme'}
					onclick={toggleTheme}
				>
					{isMounted ? (theme === 'light' ? 'Dark Theme' : 'Light Theme') : 'Toggle Theme'}
				</button>
				<button
					type="button"
					class="misch-run-button cursor-pointer rounded-none bg-link px-4 py-2 text-sm font-semibold text-bg-elevated transition hover:-translate-y-px hover:brightness-95 disabled:cursor-wait disabled:opacity-70"
					onclick={runProgram}
					disabled={status === 'running'}
				>
				{status === 'running' ? 'Running...' : 'Run to Completion'}
				</button>
				<span class={`rounded-none border px-3 py-1 text-xs font-semibold ${statusPillClass[status]}`}>
					{statusLabel[status]}
				</span>
			</div>
		</header>

		{#if errorMessage}
			<p
				class="m-0 rounded-none border border-border-strong bg-bg-elevated px-4 py-2 text-sm text-text"
				role="alert"
			>
				{errorMessage}
			</p>
		{/if}

		<div class="flex min-h-0 flex-1 flex-col" bind:this={workspacePane}>
			<div class="min-h-0" style={`flex-basis: ${topPanePercent}%`}>
				<div class="hidden h-full min-h-0 lg:flex" bind:this={editorsPane}>
					<section
						class="flex min-h-0 min-w-[16rem] flex-col overflow-hidden rounded-none border border-border bg-surface"
						style={`flex-basis: ${leftPanePercent}%`}
					>
						<div class="border-b border-border bg-bg-elevated px-3 py-2">
							<h2 class="m-0 text-sm uppercase tracking-[0.03em] text-text-muted">MIXAL Program</h2>
						</div>
						<textarea
							class="h-full w-full flex-1 resize-none overflow-auto border-none bg-bg-elevated px-3 py-3 font-['IBM_Plex_Mono','Menlo','Consolas',monospace] text-sm leading-6 text-text outline-none"
							bind:value={assembly}
							spellcheck="false"
						></textarea>
					</section>

					<div
						role="separator"
						aria-orientation="vertical"
						class="group flex w-3 cursor-col-resize items-center justify-center"
						onpointerdown={() => startDrag('vertical')}
					>
						<div class="h-20 w-1 rounded-none bg-border transition group-hover:bg-border-strong"></div>
					</div>

					<section
						class="flex min-h-0 min-w-[16rem] flex-col overflow-hidden rounded-none border border-border bg-surface"
						style={`flex-basis: ${100 - leftPanePercent}%`}
					>
						<div class="border-b border-border bg-bg-elevated px-3 py-2">
							<h2 class="m-0 text-sm uppercase tracking-[0.03em] text-text-muted">
								Paper Tape Input (Unit 16)
							</h2>
						</div>
						<textarea
							class="h-full w-full flex-1 resize-none overflow-auto border-none bg-bg-elevated px-3 py-3 font-['IBM_Plex_Mono','Menlo','Consolas',monospace] text-sm leading-6 text-text outline-none"
							bind:value={paperTapeInput}
							spellcheck="false"
						></textarea>
					</section>
				</div>

				<div class="flex h-full min-h-0 flex-col gap-3 lg:hidden">
					<section class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-none border border-border bg-surface">
						<div class="border-b border-border bg-bg-elevated px-3 py-2">
							<h2 class="m-0 text-sm uppercase tracking-[0.03em] text-text-muted">MIXAL Program</h2>
						</div>
						<textarea
							class="h-full w-full flex-1 resize-none overflow-auto border-none bg-bg-elevated px-3 py-3 font-['IBM_Plex_Mono','Menlo','Consolas',monospace] text-sm leading-6 text-text outline-none"
							bind:value={assembly}
							spellcheck="false"
						></textarea>
					</section>

					<section class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-none border border-border bg-surface">
						<div class="border-b border-border bg-bg-elevated px-3 py-2">
							<h2 class="m-0 text-sm uppercase tracking-[0.03em] text-text-muted">
								Paper Tape Input (Unit 16)
							</h2>
						</div>
						<textarea
							class="h-full w-full flex-1 resize-none overflow-auto border-none bg-bg-elevated px-3 py-3 font-['IBM_Plex_Mono','Menlo','Consolas',monospace] text-sm leading-6 text-text outline-none"
							bind:value={paperTapeInput}
							spellcheck="false"
						></textarea>
					</section>
				</div>
			</div>

			<div
				role="separator"
				aria-orientation="horizontal"
				class="group flex h-3 cursor-row-resize items-center justify-center"
				onpointerdown={() => startDrag('horizontal')}
			>
				<div class="h-1 w-28 rounded-none bg-border transition group-hover:bg-border-strong"></div>
			</div>

			<section
				class="flex min-h-[8rem] flex-col overflow-hidden rounded-none border border-border bg-surface"
				style={`flex-basis: ${100 - topPanePercent}%`}
			>
				<div class="border-b border-border bg-bg-elevated px-3 py-2">
					<h2 class="m-0 text-sm uppercase tracking-[0.03em] text-text-muted">
						Line Writer Output (Unit 18)
					</h2>
				</div>
				<pre
					class="m-0 h-full w-full overflow-auto bg-bg-elevated px-3 py-3 font-['IBM_Plex_Mono','Menlo','Consolas',monospace] text-sm leading-6 whitespace-pre text-text"
				>{lineWriterOutput || 'Program output will appear here after running.'}</pre>
			</section>
		</div>
	</div>
</div>
