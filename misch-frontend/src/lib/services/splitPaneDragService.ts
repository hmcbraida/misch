export type DragMode = 'horizontal' | 'vertical';

type SplitPaneDragServiceConfig = {
	minTopPercent: number;
	maxTopPercent: number;
	minLeftPercent: number;
	maxLeftPercent: number;
};

type SplitPaneDragHandlers = {
	onTopPanePercentChange: (value: number) => void;
	onLeftPanePercentChange: (value: number) => void;
};

const DEFAULT_CONFIG: SplitPaneDragServiceConfig = {
	minTopPercent: 35,
	maxTopPercent: 85,
	minLeftPercent: 20,
	maxLeftPercent: 80
};

export class SplitPaneDragService {
	private dragMode: DragMode | null = null;
	private workspacePane: HTMLDivElement | null = null;
	private editorsPane: HTMLDivElement | null = null;

	constructor(
		private readonly handlers: SplitPaneDragHandlers,
		private readonly config: SplitPaneDragServiceConfig = DEFAULT_CONFIG
	) {}

	setWorkspacePane(element: HTMLDivElement | null): void {
		this.workspacePane = element;
	}

	setEditorsPane(element: HTMLDivElement | null): void {
		this.editorsPane = element;
	}

	startDrag(mode: DragMode): void {
		if (typeof window === 'undefined' || typeof document === 'undefined') {
			return;
		}

		this.dragMode = mode;
		document.body.style.userSelect = 'none';
		document.body.style.cursor = mode === 'horizontal' ? 'row-resize' : 'col-resize';
		window.addEventListener('pointermove', this.onDragMove);
		window.addEventListener('pointerup', this.stopDragging);
	}

	stopDragging = (): void => {
		this.dragMode = null;
		if (typeof window === 'undefined' || typeof document === 'undefined') {
			return;
		}

		window.removeEventListener('pointermove', this.onDragMove);
		window.removeEventListener('pointerup', this.stopDragging);
		document.body.style.userSelect = '';
		document.body.style.cursor = '';
	};

	destroy(): void {
		this.stopDragging();
	}

	private onDragMove = (event: PointerEvent): void => {
		if (this.dragMode === 'horizontal' && this.workspacePane) {
			const rect = this.workspacePane.getBoundingClientRect();
			if (rect.height > 0) {
				const next = ((event.clientY - rect.top) / rect.height) * 100;
				this.handlers.onTopPanePercentChange(
					this.clamp(next, this.config.minTopPercent, this.config.maxTopPercent)
				);
			}
		}

		if (this.dragMode === 'vertical' && this.editorsPane) {
			const rect = this.editorsPane.getBoundingClientRect();
			if (rect.width > 0) {
				const next = ((event.clientX - rect.left) / rect.width) * 100;
				this.handlers.onLeftPanePercentChange(
					this.clamp(next, this.config.minLeftPercent, this.config.maxLeftPercent)
				);
			}
		}
	};

	private clamp(value: number, min: number, max: number): number {
		return Math.min(max, Math.max(min, value));
	}
}
