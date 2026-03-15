import type { ExampleProgram, ExampleProgramId } from '$lib/examplePrograms';

export class ExampleProgramService {
	private readonly byId: Record<ExampleProgramId, ExampleProgram>;

	constructor(private readonly programs: ExampleProgram[]) {
		this.byId = Object.fromEntries(programs.map((program) => [program.id, program])) as Record<
			ExampleProgramId,
			ExampleProgram
		>;
	}

	findMatchingExampleId(assembly: string, paperTapeInput: string): ExampleProgramId | null {
		for (const program of this.programs) {
			if (program.assembly === assembly && program.paperTapeInput === paperTapeInput) {
				return program.id;
			}
		}

		return null;
	}

	getById(id: ExampleProgramId): ExampleProgram {
		return this.byId[id];
	}
}
