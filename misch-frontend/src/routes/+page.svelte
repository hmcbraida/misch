<script lang="ts">
import { onDestroy, onMount } from "svelte";
import { env } from "$env/dynamic/public";
import { SessionsClient } from "$lib/api/sessionsClient";
import AppHeader from "$lib/components/AppHeader.svelte";
import WorkspaceLayout from "$lib/components/WorkspaceLayout.svelte";
import {
  DEFAULT_EXAMPLE_PROGRAM_ID,
  EXAMPLE_PROGRAMS,
  type ExampleProgramId,
} from "$lib/examplePrograms";
import { ExampleProgramService } from "$lib/services/exampleProgramService";
import { ProgramExecutionService } from "$lib/services/programExecutionService";
import { SplitPaneDragService } from "$lib/services/splitPaneDragService";
import { type Theme, ThemeService } from "$lib/services/themeService";

type UiStatus = "idle" | "running" | "success" | "error";

const API_BASE = env.PUBLIC_API_BASE || "/api/v1";
const PAPER_TAPE_UNIT = 16;
const LINE_WRITER_UNIT = 18;
const DEFAULT_BLOCK_SIZE = 1;
const THEME_STORAGE_KEY = "misch-theme";

const sessionsClient = new SessionsClient(API_BASE);
const programExecutionService = new ProgramExecutionService(sessionsClient, {
  inputUnit: PAPER_TAPE_UNIT,
  outputUnit: LINE_WRITER_UNIT,
  blockSize: DEFAULT_BLOCK_SIZE,
});
const themeService = new ThemeService(THEME_STORAGE_KEY);
const exampleProgramService = new ExampleProgramService(EXAMPLE_PROGRAMS);
const splitPaneDragService = new SplitPaneDragService({
  onTopPanePercentChange: (value) => {
    topPanePercent = value;
  },
  onLeftPanePercentChange: (value) => {
    leftPanePercent = value;
  },
});

const defaultExample = exampleProgramService.getById(
  DEFAULT_EXAMPLE_PROGRAM_ID,
);

let assembly = $state(defaultExample.assembly);
let paperTapeInput = $state(defaultExample.paperTapeInput);
let lineWriterOutput = $state("");
let errorMessage = $state("");
let status = $state<UiStatus>("idle");
let topPanePercent = $state(74);
let leftPanePercent = $state(56);
let theme = $state<Theme>(themeService.resolveInitialTheme());
let isMounted = $state(false);

const statusLabel: Record<UiStatus, string> = {
  idle: "Idle",
  running: "Running",
  success: "Completed",
  error: "Failed",
};

const statusPillClass: Record<UiStatus, string> = {
  idle: "border-border bg-bg-elevated text-text-muted",
  running: "border-link/65 bg-link/15 text-text",
  success: "border-link-hover/65 bg-link-hover/15 text-text",
  error: "border-border-strong bg-bg-elevated text-text",
};

function toggleTheme(): void {
  theme = themeService.toggleTheme(theme);
}

onMount(() => {
  isMounted = true;
  theme = themeService.initializeTheme();
});

const selectedExampleId = $derived<ExampleProgramId | "custom">(
  exampleProgramService.findMatchingExampleId(assembly, paperTapeInput) ??
    "custom",
);

function setExampleProgram(nextExampleId: ExampleProgramId): void {
  const selectedProgram = exampleProgramService.getById(nextExampleId);
  assembly = selectedProgram.assembly;
  paperTapeInput = selectedProgram.paperTapeInput;
  errorMessage = "";
}

function onExampleProgramChange(event: Event): void {
  const target = event.currentTarget;
  if (!(target instanceof HTMLSelectElement)) {
    return;
  }

  const nextValue = target.value as ExampleProgramId | "custom";
  if (nextValue === "custom") {
    return;
  }

  if (
    selectedExampleId === "custom" &&
    typeof window !== "undefined" &&
    !window.confirm(
      "You have custom edits. Discard them and load this example?",
    )
  ) {
    target.value = "custom";
    return;
  }

  setExampleProgram(nextValue);
}

onDestroy(() => {
  splitPaneDragService.destroy();
});

async function runProgram(): Promise<void> {
  status = "running";
  errorMessage = "";
  lineWriterOutput = "";

  try {
    lineWriterOutput = await programExecutionService.runToCompletion({
      assembly,
      inputText: paperTapeInput,
    });
    status = "success";
  } catch (err) {
    status = "error";
    errorMessage =
      err instanceof Error
        ? err.message
        : "Unknown error while running program";
  }
}
</script>

<div
	class="misch-shell h-dvh overflow-hidden text-text"
>
	<div
		class="mx-auto flex h-full min-h-0 w-full max-w-[1400px] flex-col gap-3 p-3 font-['Avenir_Next','Segoe_UI','Gill_Sans',sans-serif] md:p-4"
	>
		{#if errorMessage}
			<p
				class="m-0 rounded-none border border-border-strong bg-bg-elevated px-4 py-2 text-sm text-text"
				role="alert"
			>
				{errorMessage}
			</p>
		{/if}

		<AppHeader
			examplePrograms={EXAMPLE_PROGRAMS}
			selectedExampleId={selectedExampleId}
			isMounted={isMounted}
			theme={theme}
			isRunning={status === 'running'}
			statusText={statusLabel[status]}
			statusClass={statusPillClass[status]}
			onExampleProgramChange={onExampleProgramChange}
			onToggleTheme={toggleTheme}
			onRunProgram={runProgram}
		/>

		<WorkspaceLayout
			topPanePercent={topPanePercent}
			leftPanePercent={leftPanePercent}
			bind:assembly={assembly}
			bind:paperTapeInput={paperTapeInput}
			lineWriterOutput={lineWriterOutput}
			onStartVerticalDrag={() => splitPaneDragService.startDrag('vertical')}
			onStartHorizontalDrag={() => splitPaneDragService.startDrag('horizontal')}
			onWorkspacePaneChange={(element: HTMLDivElement | null) => splitPaneDragService.setWorkspacePane(element)}
			onEditorsPaneChange={(element: HTMLDivElement | null) => splitPaneDragService.setEditorsPane(element)}
		/>
	</div>
</div>
