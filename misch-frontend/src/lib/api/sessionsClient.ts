export type DeviceConfig = {
	unit: number;
	block_size: number;
};

export type CreateSessionRequest = {
	assembly: string;
	input_devices: DeviceConfig[];
	output_devices: DeviceConfig[];
};

export type CreateSessionResponse = {
	session_id: string;
	halted: boolean;
};

export type RunSessionResponse = {
	halted: boolean;
	steps_executed: number;
	reached_step_limit: boolean;
};

export type OutputTextResponse = {
	units: Record<string, string>;
};

export class SessionsClient {
	constructor(
		private readonly baseUrl: string,
		private readonly fetchImpl: typeof fetch = fetch
	) {}

	async createSession(request: CreateSessionRequest): Promise<CreateSessionResponse> {
		const response = await this.fetchImpl(`${this.baseUrl}/sessions`, {
			method: 'POST',
			headers: {
				'content-type': 'application/json'
			},
			body: JSON.stringify(request)
		});

		if (!response.ok) {
			throw new Error(await this.readApiError(response));
		}

		return (await response.json()) as CreateSessionResponse;
	}

	async appendInputText(sessionId: string, unit: number, text: string): Promise<void> {
		const response = await this.fetchImpl(`${this.baseUrl}/sessions/${sessionId}/io/input/text`, {
			method: 'POST',
			headers: {
				'content-type': 'application/json'
			},
			body: JSON.stringify({
				unit,
				text
			})
		});

		if (!response.ok) {
			throw new Error(await this.readApiError(response));
		}
	}

	async runSession(sessionId: string): Promise<RunSessionResponse> {
		const response = await this.fetchImpl(`${this.baseUrl}/sessions/${sessionId}/run`, {
			method: 'POST'
		});

		if (!response.ok) {
			throw new Error(await this.readApiError(response));
		}

		return (await response.json()) as RunSessionResponse;
	}

	async getOutputText(sessionId: string, unit: number): Promise<OutputTextResponse> {
		const searchParams = new URLSearchParams({
			unit: String(unit)
		});

		const response = await this.fetchImpl(
			`${this.baseUrl}/sessions/${sessionId}/io/output/text?${searchParams.toString()}`
		);

		if (!response.ok) {
			throw new Error(await this.readApiError(response));
		}

		return (await response.json()) as OutputTextResponse;
	}

	async deleteSession(sessionId: string): Promise<void> {
		const response = await this.fetchImpl(`${this.baseUrl}/sessions/${sessionId}`, {
			method: 'DELETE'
		});

		if (!response.ok && response.status !== 404) {
			throw new Error(await this.readApiError(response));
		}
	}

	private async readApiError(response: Response): Promise<string> {
		const fallback = `Request failed with status ${response.status}`;
		const contentType = response.headers.get('content-type') ?? '';

		if (contentType.includes('application/json')) {
			const data = (await response.json()) as { error?: string };
			return data.error ?? fallback;
		}

		const body = await response.text();
		return body.trim() || fallback;
	}
}
