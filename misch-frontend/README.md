# misch-frontend

SvelteKit for the Misch simulator UI.

## Project structure

```text
misch-frontend/
|- src/
|  |- app.html                    # HTML shell template
|  |- app.d.ts                    # app-level TypeScript declarations
|  |- routes/
|  |  |- +layout.svelte           # global layout wrapper
|  |  |- +layout.ts               # layout-level setup/data
|  |  |- +page.svelte             # main simulator page
|  |  |- layout.css               # app-wide styling tokens and base styles
|  |- lib/
|     |- api/
|     |  |- sessionsClient.ts     # backend session API client
|     |- components/
|     |  |- AppHeader.svelte      # top bar controls and status
|     |  |- WorkspaceLayout.svelte# split-pane workspace container
|     |  |- EditorPane.svelte     # editor/input pane
|     |  |- OutputPane.svelte     # output display pane
|     |- services/
|     |  |- programExecutionService.ts # run program flow orchestration
|     |  |- splitPaneDragService.ts    # pane resize drag behavior
|     |  |- exampleProgramService.ts   # example program selection logic
|     |  |- themeService.ts            # light/dark theme persistence
|     |- examplePrograms.ts       # bundled sample programs
|     |- assets/
|     |  |- favicon.svg           # app icon source
|     |- index.ts                 # library exports
|- static/
|  |- robots.txt                  # static public asset
|- svelte.config.js               # SvelteKit + static adapter config
|- vite.config.ts                 # Vite config and backend proxy setup
|- tsconfig.json                  # TypeScript config
|- biome.json                     # formatter/lint config
|- package.json                   # scripts and dependencies
|- .env.example                   # environment variable template
|- build/                         # production build output (generated)
|- .svelte-kit/                   # SvelteKit generated artifacts
|- node_modules/                  # installed dependencies
```

## Scripts

```sh
bun run dev       # start local dev server
bun run build     # create production build
bun run preview   # preview production build locally
bun run check     # run Svelte + TypeScript checks
```

## Notes

- During local development, API routes are proxied to <http://127.0.0.1:8000>
  in `vite.config.ts`.
- Set `PUBLIC_API_BASE` to override the default API base path used by the app.
