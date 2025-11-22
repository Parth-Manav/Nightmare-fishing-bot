# Stardust Pond Bot 🎣

A professional Discord bot featuring an interactive daily fishing game. Users can fish once per day, track their streaks, and compete on the leaderboard.

## Features

- **Daily Fishing**: Interactive "Fish!" button with a 24-hour cooldown.
- **Streak System**: Tracks consecutive days fished.
- **Leaderboards**: Recognizes the "Best Anglers" with high streaks.
- **Role Tracking**: Optional feature to track specific roles (e.g., "Verified Members") and see who hasn't fished.
- **Daily Summaries**: Automated daily report of total catches and missing members.
- **Data Safety**: Robust data management with automatic backups to prevent data loss.

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
