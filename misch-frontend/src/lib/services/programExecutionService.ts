import { SessionsClient } from "$lib/api/sessionsClient";

type ProgramExecutionConfig = {
  inputUnit: number;
  outputUnit: number;
  blockSize: number;
  lineWrap: number;
};

type RunProgramRequest = {
  assembly: string;
  inputText: string;
};

const DEFAULT_CONFIG: ProgramExecutionConfig = {
  inputUnit: 16,
  outputUnit: 18,
  blockSize: 1,
  lineWrap: 100,
};

export class ProgramExecutionService {
  private readonly config: ProgramExecutionConfig;

  constructor(
    private readonly sessionsClient: SessionsClient,
    config: Partial<ProgramExecutionConfig> = {},
  ) {
    this.config = { ...DEFAULT_CONFIG, ...config };
  }

  async runToCompletion(request: RunProgramRequest): Promise<string> {
    let sessionId: string | null = null;

    try {
      const session = await this.sessionsClient.createSession({
        assembly: request.assembly,
        input_devices: [
          { unit: this.config.inputUnit, block_size: this.config.blockSize },
        ],
        output_devices: [
          { unit: this.config.outputUnit, block_size: this.config.blockSize },
        ],
      });

      sessionId = session.session_id;

      await this.sessionsClient.appendInputText(
        sessionId,
        this.config.inputUnit,
        request.inputText,
      );

      await this.sessionsClient.runSession(sessionId);

      const output = await this.sessionsClient.getOutputText(
        sessionId,
        this.config.outputUnit,
      );
      return this.wrapOutput(
        output.units[String(this.config.outputUnit)] ?? "",
      );
    } finally {
      if (sessionId) {
        try {
          await this.sessionsClient.deleteSession(sessionId);
        } catch {
          // best effort cleanup only
        }
      }
    }
  }

  private wrapOutput(text: string): string {
    if (!text) {
      return "";
    }

    const wrappedLines: string[] = [];
    for (let i = 0; i < text.length; i += this.config.lineWrap) {
      wrappedLines.push(text.slice(i, i + this.config.lineWrap));
    }

    return wrappedLines.join("\n");
  }
}
