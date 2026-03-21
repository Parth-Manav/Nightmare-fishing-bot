# 🎣 Stardust Pond — Discord Fishing Bot

[![Built with Rust](https://img.shields.io/badge/Built%20with-Rust-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Framework: Poise](https://img.shields.io/badge/Framework-Poise%200.6-blue?logo=discord&logoColor=white)](https://github.com/serenity-rs/poise)
[![Async: Tokio](https://img.shields.io/badge/Async-Tokio-green?logo=rust)](https://tokio.rs/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Stardust Pond** is a production-grade, fully thread-safe Discord bot that drives daily community engagement through a one-click fishing minigame. Members fish once per day, build streaks, and compete on leaderboards. Admins receive automated daily summaries and can fine-tune reminder behavior.

Originally a Node.js application, this is a complete Rust rewrite built for correctness, safety, and zero data loss under concurrent load.

---

## ✨ Features

- 🎣 **One-click fishing** via a persistent Discord button (no slash command required)
- 🔥 **Daily streak tracking** — streaks increment on consecutive fishing days and reset on a miss
- 🏆 **Best Anglers leaderboard** — configurable minimum streak threshold
- 📊 **Daily automated summary** — posts fish counts, missed members, and top anglers
- 🔔 **Smart ping reminders** — only pings members inactive for a configurable number of days
- 💾 **Atomic file saves** — writes to `.tmp` then renames; zero risk of corrupt saves
- 🔒 **TOCTOU-safe concurrency** — reset flag checked inside the write lock, not before it
- 🛡️ **Missed reset catch-up** — if the bot was offline at 14:30 GMT, it auto-runs the reset on next boot
- ❌ **DoS protection** — `/setrole` blocks the `@everyone` role from being tracked
- 🗂️ **Rotating backups** — keeps the last 5 timestamped backups automatically

---

## 🛠 Tech Stack

| Layer | Technology |
|---|---|
| Language | Rust (2021 Edition) |
| Discord Framework | [Poise 0.6](https://github.com/serenity-rs/poise) + [Serenity](https://github.com/serenity-rs/serenity) |
| Async Runtime | [Tokio 1.33](https://tokio.rs/) with `rt-multi-thread` |
| Scheduling | [tokio-cron-scheduler 0.9](https://github.com/mvniekerk/tokio-cron-scheduler) |
| Serialization | [Serde](https://serde.rs/) + `serde_json` |
| Date / Time | [Chrono 0.4](https://docs.rs/chrono) |
| Logging | [Tracing](https://docs.rs/tracing) + `tracing-subscriber` |
| Config | [Dotenvy 0.15](https://docs.rs/dotenvy) |

---

## 📁 Project Structure

```
src/
├── main.rs               # Bot startup, cron scheduler, framework init
├── data.rs               # DataManager: persistence, atomic save, backup
├── game.rs               # FishingManager: core game logic, daily reset
├── events.rs             # Button interaction handler
└── commands/
    ├── fishing.rs        # /fish, /summary (user commands)
    └── admin.rs          # /fishsetup, /setrole, /setsummarychannel, etc.
```

---

## ⚙️ Prerequisites

- **Rust stable** — install via [rustup.rs](https://rustup.rs/)
- A **Discord Bot Token** with the following enabled in the [Developer Portal](https://discord.com/developers/applications):
  - `Server Members Intent` (required for member pagination)
  - `Message Content Intent` (optional, for safety)
  - Slash command scopes: `bot` + `applications.commands`

---

## 🚀 Installation

```bash
# 1. Clone the repository
git clone https://github.com/Parth-Manav/Nightmare-fishing-bot.git
cd Nightmare-fishing-bot

# 2. Create your environment file
cp .env.example .env
# Then open .env and set your token (see Configuration below)

# 3. Build and run in development
cargo run

# 4. Build for production
cargo build --release
./target/release/stardust-pond-bot
```

---

## 🔧 Configuration

Create a `.env` file in the project root:

```env
DISCORD_BOT_TOKEN=your_discord_bot_token_here
```

| Variable | Required | Description |
|---|---|---|
| `DISCORD_BOT_TOKEN` | ✅ Yes | Your Discord bot token from the Developer Portal |

All other configuration (tracked role, summary channel, reminder thresholds) is set at runtime via admin slash commands and persisted to `fishing_data.json`.

---

## 📖 Usage

### Starting the Bot
```bash
cargo run          # Development (with debug logs)
cargo run --release  # Production (optimised)
```

Logs are written to stdout using `tracing`. On first run, the bot registers all slash commands globally — this can take up to 1 hour to propagate to all Discord servers.

### First-Time Server Setup (Admin)
1. Run `/fishsetup` in the channel where you want the fishing button to appear
2. Run `/setrole` and select the role whose members you want to track
3. Run `/setsummarychannel` in the channel where daily summaries should be posted
4. Optionally run `/setbestanglerstreak` and `/setreminderthreshold` to tune behaviour

---

## 🤖 Commands

### 👤 User Commands

| Command | Description |
|---|---|
| `/fish` | Cast your line. One catch per fishing day (resets at 14:30 GMT). |
| `/summary` | Manually trigger today's summary embed (shows current daily stats). |

### 🔑 Admin Commands

| Command | Description |
|---|---|
| `/fishsetup` | Spawns the persistent **"🎣 Fish!"** button in the current channel. |
| `/fishsummary` | Shows a private (ephemeral) list of members who haven't fished today. |
| `/setrole @role` | Sets the role to track for reminders and stats. Blocks `@everyone`. |
| `/setsummarychannel` | Sets the current channel as the daily summary destination. |
| `/setbestanglerstreak <n>` | Sets the minimum streak to appear on the Best Anglers list. |
| `/setreminderthreshold <n>` | Sets inactivity days before a member gets pinged (default: 1). |
| `/togglereminder <true/false>` | Enables or disables `@ping` mentions in the daily reminder. |

---

## 🏗️ Architecture & How It Works

```
main.rs
  ├── Initialises DataManager (loads fishing_data.json)
  ├── Initialises FishingManager (wraps DataManager)
  ├── Schedules daily cron job at 14:30 GMT (0 30 14 * * *)
  ├── Spawns startup catch-up task (checks if reset was missed)
  └── Starts Poise/Serenity Discord client
```

### Fishing Day Boundary
The bot defines a "fishing day" as the 24-hour period starting at **14:30 GMT**. All date comparisons shift the wall-clock time back by 14h 30m before formatting to `YYYY-MM-DD`. This prevents double-fishing exploits around the midnight UTC boundary.

### Daily Reset Sequence (Atomic)
At 14:30 GMT every day, the bot executes the following in a single locked sequence:
1. **Post summary** — sends the daily embed to the summary channel
2. **Backup** — saves a timestamped copy to `backups/`
3. **Reset** — clears `users` map, zeros `daily_count`, updates `last_reset_timestamp`
4. **Save + Backup** — persists the clean state and takes a post-reset backup

The entire sequence is guarded by `is_resetting: Arc<AtomicBool>`. The flag is set *inside* the write lock to prevent TOCTOU race conditions.

### Concurrency Model

| Concern | Solution |
|---|---|
| Concurrent fish clicks | `tokio::sync::RwLock` on `FishingData` |
| Concurrent disk writes | `tokio::sync::Mutex` (`save_lock`) — sequential writes only |
| Reset during fishing | `is_resetting` checked inside write lock scope |
| Long HTTP calls blocking data | State cloned out of read lock before pagination loop |

### Atomic File Saves
```
fishing_data.json.tmp  ←  write full JSON
       ↓ fs::rename()   (atomic on all major OS)
fishing_data.json      ←  swap
```
If the process crashes mid-write, the original file is untouched.

---

## 🛡️ Error Handling & Safety

| Scenario | Behaviour |
|---|---|
| Disk full / write error | `save()` returns `Err`, user sees "Disk save failed" message, data preserved in memory |
| Bot crash during reset | `is_resetting` RAII guard resets flag on drop; catch-up runs on next boot |
| Invalid role ID in config | Admin commands validate and return clear error messages |
| `@everyone` tracked as role | Blocked at the command level with an explicit rejection message |
| Discord API 429 rate limit | Handled by Serenity's built-in rate limiter; no manual handling needed |
| Member fetch failure | Logged as error, pagination loop breaks safely, summary posts with partial data |
| Corrupt `fishing_data.json` | Falls back to `FishingData::default()` and starts fresh with an error log |
| Negative inactivity days (clock desync) | `.max(0)` clamp prevents negative values from triggering false reminders |

---

## ⚠️ Limitations

- **Summary snapshot freshness**: The member list cloned before pagination may be up to a few seconds stale if a user fishes exactly during the summary generation window. This is an intentional trade-off to prevent deadlocking the bot's write lock during HTTP calls.
- **Single-guild design**: The bot stores one `guild_id` in `fishing_data.json`. Running across multiple servers requires a separate instance per server.
- **No database backend**: All state is stored in a single JSON file. For servers with 50k+ persistent users, an SQLite or Postgres backend would be more appropriate.
- **Member cache not maintained**: The bot paginates Discord's API on demand (once per day during the summary). It does not maintain an in-memory member cache between resets.

---

## 🔮 Future Improvements

- [ ] Multi-guild support with per-guild data isolation
- [ ] SQLite or Postgres backend for scalability
- [ ] Web dashboard for viewing stats and managing configuration
- [ ] Fish variety system (common, rare, legendary catches with different rewards)
- [ ] Optional webhook-based summary delivery instead of channel message
- [ ] Persistent in-memory member cache to eliminate daily API pagination

---

## 📝 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.

---

*Built with ❤️ for the Stardust Pond community.*