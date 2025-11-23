# Stardust Pond Bot 🎣

A Discord bot featuring an interactive daily fishing game. Users can fish once per day, track their streaks, and compete on the leaderboard.

## Features

- **Daily Fishing**: Interactive "Fish!" button with a 24-hour cooldown.
- **Beautiful Visuals**: Rich Embed cards with user avatars and gold/blue themes.
- **Smart Timezones**: Timestamps automatically adapt to each user's local time.
- **Streak System**: Tracks consecutive days fished.
- **Leaderboards**: Recognizes the "Best Anglers" with high streaks.
- **Role Tracking**: Optional feature to track specific roles (e.g., "Verified Members") and see who hasn't fished.
- **Daily Summaries**: Automated daily report of total catches and missing members.
- **Optimized Performance**: Efficient data handling with smart backups.

## Installation

### Prerequisites
- Node.js v16.9.0 or higher
- A Discord Bot Token

### Setup

1.  **Clone the repository**
    ```bash
    git clone https://github.com/yourusername/stardust-pond-bot.git
    cd stardust-pond-bot
    ```

2.  **Install dependencies**
    ```bash
    npm install
    ```

3.  **Configure Environment**
    Create a `.env` file in the root directory and add your bot token:
    ```env
    DISCORD_BOT_TOKEN=your_discord_bot_token_here
    ```

4.  **Start the Bot**
    ```bash
    npm start
    ```

## Commands

| Command | Description | Permission |
| :--- | :--- | :--- |
| `/fishsetup` | Spawns the interactive fishing pond message. | Everyone |
| `/setrole` | Sets a specific role to track for daily stats. | Admin |
| `/setsummarychannel` | Sets the channel where daily summaries are posted. | Admin |
| `/fishsummary` | Manually triggers a summary of who hasn't fished today. | Admin |
| `/togglereminder` | Toggles pinging users in the daily summary. | Admin |
| `/setreminderthreshold` | Sets the days of inactivity before a user is pinged. | Admin |

## Inactivity Threshold
By default, the bot pings tracked members if they haven't fished **today** (1 day). You can change this using `/setreminderthreshold`.
- **Example**: `/setreminderthreshold days:3`
- **Result**: Members will only be pinged if they haven't fished for **3 consecutive days**.
- This allows for a more relaxed community where daily fishing isn't mandatory to avoid pings.

## Project Structure

The project follows a modular architecture:

```
/
├── src/
│   ├── commands/       # Slash command definitions
│   ├── events/         # Event handlers (ready, interactionCreate)
│   ├── game/           # Core game logic (FishingManager)
│   ├── data/           # Data persistence and backups
│   └── utils/          # Utility functions
├── index.js            # Entry point
└── fishing_data.json   # Data storage (auto-generated)
```

## License

This project is licensed under the ISC License.
