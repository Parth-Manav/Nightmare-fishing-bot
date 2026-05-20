<div align="center">

# 🎣 Stardust Pond — Discord Fishing Bot

**A production-grade async Discord bot written in Rust**  
*Concurrent state management · Atomic persistence · Scheduled automation*

![CI](https://github.com/Parth-Manav/Nightmare-fishing-bot/actions/workflows/ci.yml/badge.svg)
![Rust](https://img.shields.io/badge/Rust-2021_Edition-orange?logo=rust)
![Tokio](https://img.shields.io/badge/async-Tokio-blue)
![License](https://img.shields.io/badge/license-MIT-green)
![Tests](https://img.shields.io/badge/tests-30_passing-brightgreen)

</div>

---

## Overview

Stardust Pond is a concurrent Rust service that manages daily fishing mechanics for Discord communities. It uses Tokio's multi-threaded async runtime to handle concurrent user interactions safely, persists state with atomic file writes, and runs a cron-scheduled reset pipeline with startup catch-up logic for missed resets.

The bot began as a Node.js Discord automation project for a Tokyo Debunker community and ran for months before being rewritten in Rust. The current version keeps the original one-click attendance idea, while adding stronger persistence, typed errors, structured logging, and a production-focused test suite.

## Features

| Feature | Implementation |
|---|---|
| One-click fishing | Persistent Discord button plus `/fish` slash command |
| Daily fishing with cooldown | Fishing-day boundary at 14:30 UTC; write-lock protected check/update |
| Consecutive streak tracking | Resets on missed day; persists across bot restarts |
| Best Anglers leaderboard | Configurable streak threshold; deterministic sort for stable embeds |
| Daily automated summary | `tokio-cron-scheduler`; posts fish counts, missed members, and top anglers |
| Smart ping reminders | Tracks configured role; pings only members inactive past threshold |
| Admin configuration | Slash commands persist role, summary channel, thresholds, and reminder settings |
| Atomic persistence | `.tmp` -> `rename()` write pattern; save coalescing under concurrent writes |
| Rotating backups | Timestamped backups with configurable retention |
| Crash recovery | Missing, corrupt, or invalid save file -> clean `FishingData::default()` |
| Startup catch-up reset | Detects missed reset after downtime and runs reset immediately |
| DoS protection | Blocks tracking `@everyone` to avoid server-wide member pagination abuse |
| Structured logging | `tracing` + `EnvFilter`; user and guild context on fishing spans |
| Typed error system | `thiserror` `BotError` hierarchy; fallible paths return `BotResult<T>` |
| Production test suite | Boundary, streak, chaos, simulation, and concurrency stress tests |

## Architecture

```text
main.rs
  ├── Loads Config from environment
  ├── Initializes DataManager from fishing_data.json
  ├── Initializes FishingManager around shared state
  ├── Registers Poise slash commands
  ├── Schedules daily reset at 14:30 UTC
  ├── Spawns startup catch-up reset check
  └── Starts the Serenity Discord client
```

### Core Flow

```text
Discord interaction
  └── command / button handler
        └── FishingManager::handle_fishing()
              ├── acquire write lock
              ├── check reset state
              ├── check whether user already fished
              ├── update daily and persistent user state
              ├── mark data dirty
              └── DataManager::save()
                    ├── serialize current state
                    ├── write fishing_data.json.tmp
                    └── rename tmp file over fishing_data.json
```

### Fishing Day Boundary

The bot defines a fishing day as the 24-hour period starting at **14:30 UTC**. Date calculations shift the current UTC timestamp back by the reset offset before formatting the fishing-day key as `YYYY-MM-DD`.

This means `14:29:59 UTC` still belongs to the previous fishing day, while `14:30:00 UTC` belongs to the new one. That boundary is covered by unit tests because it controls cooldowns, streaks, and the daily leaderboard.

### Daily Reset Sequence

At the configured reset time, the bot runs this sequence:

1. Posts the daily summary to the configured summary channel.
2. Creates a pre-reset backup snapshot.
3. Clears the daily `users` map.
4. Resets `daily_count` to zero.
5. Updates `last_reset_timestamp`.
6. Saves the clean post-reset state.
7. Creates a post-reset backup snapshot.

The reset path is idempotent for the same fishing day, so a crash followed by startup catch-up does not wipe a valid streak twice.

### Concurrency Model

| Concern | Solution |
|---|---|
| Concurrent fish clicks | `tokio::sync::RwLock<FishingData>` |
| Duplicate clicks from one user | Check-and-insert happens under one write lock |
| Concurrent disk writes | `tokio::sync::Mutex` save lock |
| Save storms under load | Dirty/saved generation counters coalesce stale queued saves |
| Reset while users fish | Atomic reset flag checked inside the write-locked path |
| Discord pagination latency | State snapshot is cloned before HTTP pagination |
| Panic-free cleanup | Reset guard releases the reset flag on drop |

### Why These Design Decisions?

**Atomic saves** — Writing directly to the target file risks a half-written JSON file if the process crashes during the write. The `.tmp` -> `rename()` pattern is atomic at the OS level: either the rename completes and the new file is visible, or it does not and the old file remains intact. This is the same technique used by SQLite's WAL mode and Redis AOF persistence.

**TOCTOU-safe reset** — Checking `is_resetting` inside the write lock closes the race window between reading the flag and acting on it. A naive pre-lock check would allow two tasks to both see `false` and both proceed into conflicting state transitions.

**Snapshot-before-paginate** — Discord pagination requires multiple HTTP calls. Holding a read lock across network calls would block all fish commands for the full duration. Cloning state out of the lock, then releasing it before any I/O, keeps the hot path responsive.

**Catch-up reset on restart** — If the bot was offline when the 14:30 UTC reset was scheduled, it checks on startup whether the reset was missed and runs it immediately. Without this, the leaderboard would remain stale until the next scheduled reset 24 hours later.

**Typed errors instead of string failures** — All fallible operations return `BotResult<T>`, which keeps I/O errors, JSON errors, Discord API errors, configuration errors, and state errors distinct. That makes propagation with `?` clean without losing operational context.

## Tech Stack

| Layer | Technology |
|---|---|
| Language | Rust 2021 Edition |
| Discord Framework | Poise 0.6 + Serenity |
| Async Runtime | Tokio 1.x multi-threaded runtime |
| Scheduling | tokio-cron-scheduler |
| Serialization | Serde + serde_json |
| Date / Time | Chrono |
| Error Handling | thiserror |
| Logging | tracing + tracing-subscriber EnvFilter |
| Configuration | dotenvy + typed `Config::from_env()` |
| Testing | Tokio tests, tempfile, simulation and stress coverage |
| CI | GitHub Actions: build, test, clippy, fmt check |

## Getting Started

### Prerequisites

- Rust 1.75+ stable
- A Discord application and bot token
- Server Members Intent enabled in the Discord Developer Portal
- `bot` and `applications.commands` OAuth scopes
- Visual Studio Build Tools on Windows, or gcc/clang on Linux/macOS

### Installation

```bash
git clone https://github.com/Parth-Manav/Nightmare-fishing-bot.git
cd Nightmare-fishing-bot

cp .env.example .env
# Edit .env and set DISCORD_BOT_TOKEN

cargo run
```

For a release build:

```bash
cargo build --release
./target/release/stardust-pond-bot
```

On Windows PowerShell:

```powershell
cargo build --release
.\target\release\stardust-pond-bot.exe
```

### Configuration

Create a `.env` file in the project root. The checked-in `.env.example` documents every supported value:

```env
# Required: Discord bot token from the Discord Developer Portal.
DISCORD_BOT_TOKEN=your_discord_bot_token_here

# Optional: tracing verbosity. Default: info
LOG_LEVEL=info

# Optional: JSON data file path. Default: fishing_data.json
DATA_PATH=fishing_data.json

# Optional: directory for rotating data backups. Default: backups/
BACKUP_DIR=backups/

# Optional: number of backup files to keep. Default: 5
MAX_BACKUPS=5

# Optional: daily reset hour in UTC, 0-23. Default: 14
RESET_HOUR=14

# Optional: daily reset minute in UTC, 0-59. Default: 30
RESET_MINUTE=30
```

### First-Time Discord Setup

1. Invite the bot with `bot` and `applications.commands` scopes.
2. Run `/fishsetup` in the channel where the fishing button should appear.
3. Run `/setrole` and select the role whose members should be tracked.
4. Run `/setsummarychannel` in the channel where daily summaries should be posted.
5. Optionally tune `/setbestanglerstreak`, `/setreminderthreshold`, and `/togglereminder`.

### Commands

| Command | Access | Purpose |
|---|---|---|
| `/fish` | User | Records one catch for the current fishing day |
| `/summary` | User | Manually posts the current daily summary |
| `/fishsetup` | Admin | Creates the persistent fishing button |
| `/fishsummary` | Admin | Shows an ephemeral list of members who have not fished |
| `/setrole` | Admin | Sets the tracked Discord role; rejects `@everyone` |
| `/setsummarychannel` | Admin | Stores the current channel as the summary destination |
| `/setbestanglerstreak` | Admin | Configures the minimum streak for Best Anglers |
| `/setreminderthreshold` | Admin | Configures inactivity days before reminder pings |
| `/togglereminder` | Admin | Enables or disables ping mentions in summaries |

## Testing

```bash
cargo test             # run full test suite
cargo test boundary    # fishing day boundary tests only
cargo test streak      # streak logic tests only
cargo test chaos       # adversarial input tests only
cargo test simulation  # long-running simulation tests only
cargo test stress      # concurrency stress tests only
```

See [TEST_REPORT.md](./TEST_REPORT.md) for full documentation of the 30-test production-readiness suite, including chaos testing, corrupted-file recovery, a 365-day simulation, and 1000-request concurrency stress tests.

The full local suite currently reports:

```text
test result: ok. 34 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Project Structure

```text
src/
├── main.rs          # entry point, tracing init, scheduler setup
├── config.rs        # typed Config with from_env() validation
├── error.rs         # BotError hierarchy (thiserror)
├── data.rs          # DataManager: atomic save/load, backup rotation
├── game.rs          # FishingManager: core game logic, streak computation
├── events.rs        # Serenity button interaction handler
└── commands/
    ├── fishing.rs   # /fish and /summary slash commands
    └── admin.rs     # setup, role, reminder, and summary admin commands
```

Additional repository files:

```text
.github/workflows/ci.yml  # build, test, clippy, and fmt checks
Cargo.toml                # crate metadata and dependency declarations
Cargo.lock                # locked dependency graph for reproducible builds
TEST_REPORT.md            # reliability test report
.env.example              # documented environment configuration
```

## What I Learned Building This

- I started this project because my Tokyo Debunker friends needed a simple attendance-style system for daily fishing. Most people did not want to type messages or use slash commands every day, so I learned that good automation is not only about writing code, but about reducing friction for real users.

- The first version was built in Node.js and ran for months, which taught me a lot because people were actually depending on it daily. When the bot's internal state broke or a streak got ruined, it was not just a small bug anymore. It affected my friends, so I had to think more seriously about reliability, recovery, and testing.

- Hosting the bot on free or low-resource platforms forced me to care about memory and runtime cost early. I already had other Discord automation projects running, and I was also building browser automation tools, so I slowly learned why backend systems need to be efficient, not just functional.

- The Node.js stage helped me understand many logic flaws the hard way: missed resets, broken streaks, state getting out of sync, and long-running processes behaving differently from short test runs. Those problems pushed me to add more checks, better persistence, and eventually a proper test suite.

- I recently started moving the project to Rust because I wanted stronger guarantees, lower memory usage, and more control over concurrency. I am still learning Rust, but this rewrite helped me understand ownership, typed errors, async locks, and why systems code rewards careful design.

- This project also connects with what I am learning now: networking, API calls, browser automation, WebSockets, and reverse engineering. In the future, I want to make the bot even more automatic, so my friends may not need to open Discord at all for attendance-style tracking.

## Limitations

- The bot is designed around one configured guild per running instance. Multi-guild support would need per-guild data isolation.
- State is stored in a JSON file. For very large communities, SQLite or Postgres would be a better persistence layer.
- The test suite mocks time by directly passing timestamps into core logic; it does not fast-forward the actual cron scheduler.
- Discord API behavior is exercised through Serenity types and command paths, but the local test suite does not call the live Discord network.
- Summary data is intentionally snapshot-based. A user fishing during summary generation may appear in the next cycle, which is the trade-off for avoiding locks across HTTP calls.

## Future Improvements

- Add per-guild data isolation for multi-server deployments.
- Add SQLite or Postgres persistence behind the existing `DataManager` boundary.
- Add a web dashboard for stats and admin configuration.
- Add fish rarity tiers and seasonal events.
- Add an optional member cache to reduce daily Discord API pagination.
- Add integration tests against a mocked Discord HTTP server.

## License

MIT — see LICENSE
