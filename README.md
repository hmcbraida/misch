# misch

`misch` is a project for emulating Donald Knuth's MIX machine. It
is split into a Rust workspace for the emulator/runtime surfaces and a SvelteKit
frontend.

<img width="1876" height="922" alt="image" src="https://github.com/user-attachments/assets/d61784ef-2799-4f36-b839-b0b0a5d4f365" />


## Repository layout

- `misch-core` - Rust core lib
- `misch-cli` - Rust command-line runner for MIXAL programs.
- `misch-api` - Rocket HTTP API for stateful MIX sessions.
- `misch-frontend` - SvelteKit UI for interacting with the emulator via the API.
- `examples` - Sample MIXAL programs and input fixtures.

## What you need installed:

- Cargo
- Bun

## Running tests

From the repository root:

```sh
cargo test --workspace
```

Frontend checks (from `misch-frontend`):

```sh
bun install
bun run check
```

## Development: API + Frontend

Run the API (from repo root):

```sh
cargo run -p misch-api
```

This starts Rocket on `http://127.0.0.1:8000` by default, with routes mounted under `/api/v1`.

In a second terminal, run the frontend:

```sh
cd misch-frontend
bun install
bun run dev
```

Then open the local Vite/SvelteKit dev URL shown in the terminal.

Frontend API base path is configurable via `PUBLIC_API_BASE` (see
`misch-frontend/.env.example`). Local development defaults to `/api/v1`.
Frontend URL base path is configurable at build time via `BASE_PATH`
(`''` for local development, `/misch` for production subpath hosting).

## Running the CLI

From the repository root:

```sh
cargo run -p misch-cli -- <assembly-file> [options]
```

Example:

```sh
cargo run -p misch-cli -- examples/primes.mixal --paper-tape examples/inputs/primes_input.txt
```
