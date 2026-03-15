export type Theme = 'light' | 'dark';

export class ThemeService {
	constructor(private readonly storageKey: string) {}

	resolveInitialTheme(): Theme {
		if (typeof document === 'undefined') {
			return 'light';
		}

		const datasetTheme = document.documentElement.dataset.theme;
		if (datasetTheme === 'light' || datasetTheme === 'dark') {
			return datasetTheme;
		}

		return 'light';
	}

	initializeTheme(): Theme {
		if (typeof window === 'undefined') {
			return this.resolveInitialTheme();
		}

		const datasetTheme = document.documentElement.dataset.theme;
		if (datasetTheme === 'light' || datasetTheme === 'dark') {
			this.applyTheme(datasetTheme);
			return datasetTheme;
		}

		let initialTheme: Theme = window.matchMedia('(prefers-color-scheme: dark)').matches
			? 'dark'
			: 'light';

		try {
			const storedTheme = window.localStorage.getItem(this.storageKey);
			if (storedTheme === 'light' || storedTheme === 'dark') {
				initialTheme = storedTheme;
			}
		} catch {
			// continue with system default
		}

		this.applyTheme(initialTheme);
		return initialTheme;
	}

	toggleTheme(currentTheme: Theme): Theme {
		const nextTheme: Theme = currentTheme === 'light' ? 'dark' : 'light';
		this.applyTheme(nextTheme);

		if (typeof window === 'undefined') {
			return nextTheme;
		}

		try {
			window.localStorage.setItem(this.storageKey, nextTheme);
		} catch {
			// best effort persistence only
		}

		return nextTheme;
	}

	private applyTheme(nextTheme: Theme): void {
		if (typeof document === 'undefined') {
			return;
		}

		document.documentElement.dataset.theme = nextTheme;
		document.documentElement.style.colorScheme = nextTheme;
	}
}
