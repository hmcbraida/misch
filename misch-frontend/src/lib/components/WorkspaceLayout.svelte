<script lang="ts">
	import EditorPane from '$lib/components/EditorPane.svelte';
	import OutputPane from '$lib/components/OutputPane.svelte';

	type WorkspaceLayoutProps = {
		topPanePercent: number;
		leftPanePercent: number;
		assembly: string;
		paperTapeInput: string;
		lineWriterOutput: string;
		onStartVerticalDrag: () => void;
		onStartHorizontalDrag: () => void;
		onWorkspacePaneChange: (element: HTMLDivElement | null) => void;
		onEditorsPaneChange: (element: HTMLDivElement | null) => void;
	};

	let {
		topPanePercent,
		leftPanePercent,
		assembly = $bindable(),
		paperTapeInput = $bindable(),
		lineWriterOutput,
		onStartVerticalDrag,
		onStartHorizontalDrag,
		onWorkspacePaneChange,
		onEditorsPaneChange
	}: WorkspaceLayoutProps = $props();

	let workspacePane: HTMLDivElement | null = null;
	let editorsPane: HTMLDivElement | null = null;

	$effect(() => {
		onWorkspacePaneChange(workspacePane);
	});

	$effect(() => {
		onEditorsPaneChange(editorsPane);
	});
</script>

<div class="flex min-h-0 flex-1 flex-col" bind:this={workspacePane}>
	<div class="min-h-0" style={`flex-basis: ${topPanePercent}%`}>
		<div class="hidden h-full min-h-0 lg:flex" bind:this={editorsPane}>
			<div
				class="flex min-h-0 min-w-[16rem]"
				style={`flex-basis: ${leftPanePercent}%`}
			>
				<EditorPane title="MIXAL Program" bind:value={assembly} />
			</div>

			<div
				role="separator"
				aria-orientation="vertical"
				class="group flex w-3 cursor-col-resize items-center justify-center"
				onpointerdown={onStartVerticalDrag}
			>
				<div class="h-20 w-1 rounded-none bg-border transition group-hover:bg-border-strong"></div>
			</div>

			<div
				class="flex min-h-0 min-w-[16rem]"
				style={`flex-basis: ${100 - leftPanePercent}%`}
			>
				<EditorPane title="Paper Tape Input (Unit 16)" bind:value={paperTapeInput} />
			</div>
		</div>

		<div class="flex h-full min-h-0 flex-col gap-3 lg:hidden">
			<EditorPane title="MIXAL Program" bind:value={assembly} />
			<EditorPane title="Paper Tape Input (Unit 16)" bind:value={paperTapeInput} />
		</div>
	</div>

	<div
		role="separator"
		aria-orientation="horizontal"
		class="group flex h-3 cursor-row-resize items-center justify-center"
		onpointerdown={onStartHorizontalDrag}
	>
		<div class="h-1 w-28 rounded-none bg-border transition group-hover:bg-border-strong"></div>
	</div>

	<div style={`flex-basis: ${100 - topPanePercent}%`}>
		<OutputPane outputText={lineWriterOutput} />
	</div>
</div>
