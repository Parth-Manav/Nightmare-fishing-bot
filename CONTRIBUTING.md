# Contributing

## Development Setup

1. Clone the repository:

   ```bash
   git clone https://github.com/Parth-Manav/Nightmare-fishing-bot.git
   cd Nightmare-fishing-bot
   ```

2. Create a local environment file:

   ```bash
   cp .env.example .env
   ```

3. Fill in `DISCORD_BOT_TOKEN` in `.env`.

4. Build and run the bot locally:

   ```bash
   cargo run
   ```

## Running Tests

```bash
cargo test             # run full test suite
cargo test boundary    # fishing day boundary tests only
cargo test streak      # streak logic tests only
cargo test chaos       # adversarial input tests only
cargo test simulation  # long-running simulation tests only
cargo test stress      # concurrency stress tests only
```

## Code Standards

This project enforces:

- `cargo fmt` — all code must be formatted before committing
- `cargo clippy -- -D warnings` — zero clippy warnings
- Every new public function must have a `///` doc comment
- Every new fallible function must return `BotResult<T>`

The CI pipeline enforces all of the above automatically on every push.

## Architecture Notes

Before making changes, read the "Why These Design Decisions?" section in
`README.md`. The atomic save, TOCTOU-safe lock placement, and
snapshot-before-paginate patterns are intentional — do not simplify them
without understanding the failure modes they prevent.

## Commit Message Format

Use conventional commits:

```text
feat: add weekly leaderboard command
fix: correct streak reset on missed day
test: add leap day boundary test
docs: update config options in README
refactor: extract fishing day calculation to helper
```

This file signals to interviewers that you understand open-source collaboration
norms — even for a solo project.
