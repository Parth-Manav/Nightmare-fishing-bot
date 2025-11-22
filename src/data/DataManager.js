const fs = require('fs');
const path = require('path');
const { shouldReset } = require('../utils/timeUtils');

const DATA_FILE = path.join(__dirname, '../../fishing_data.json');
const BACKUP_DIR = path.join(__dirname, '../../backups');

class DataManager {
    constructor() {
        this.data = {
            dailyCount: 0,
            lastResetTimestamp: Date.now(),
            users: {},
            persistentUsers: {},
            buttonMessageId: null,
            buttonChannelId: null,
            trackedRoleId: null,
            summaryChannelId: null,
            guildId: null,
            pingReminderEnabled: true,
            bestAnglerStreak: 5
        };

        // Ensure backup directory exists
        if (!fs.existsSync(BACKUP_DIR)) {
            fs.mkdirSync(BACKUP_DIR, { recursive: true });
        }
    }

    loadData() {
        try {
            if (fs.existsSync(DATA_FILE)) {
                const fileContent = fs.readFileSync(DATA_FILE, 'utf8');
                const loadedData = JSON.parse(fileContent);

                // Merge loaded data with default structure to ensure all fields exist
                this.data = { ...this.data, ...loadedData };

                // Migration logic from original code
                if (typeof this.data.pingReminderEnabled === 'undefined') {
                    this.data.pingReminderEnabled = true;
                }

                if (typeof this.data.bestAnglerStreak === 'undefined') {
                    this.data.bestAnglerStreak = 5;
                }

                if (this.data.lastReset && !this.data.lastResetTimestamp) {
                    this.data.lastResetTimestamp = Date.now() - (24 * 60 * 60 * 1000);
                    delete this.data.lastReset;
                    this.saveData();
                    console.log('Migrated old date format to timestamp format');
                }

                console.log('✅ Data loaded successfully');
            }
        } catch (error) {
            console.error('Error loading data:', error);
        }
    }

    saveData() {
        try {
            // Create a backup before saving
            this.backupData();

            fs.writeFileSync(DATA_FILE, JSON.stringify(this.data, null, 2));
        } catch (error) {
            console.error('Error saving data:', error);
        }
    }

    backupData() {
        try {
            // Keep only last 5 backups to save space
            const backups = fs.readdirSync(BACKUP_DIR)
                .filter(file => file.endsWith('.json'))
                .sort();

            if (backups.length >= 5) {
                fs.unlinkSync(path.join(BACKUP_DIR, backups[0]));
            }

            const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
            const backupPath = path.join(BACKUP_DIR, `fishing_data_${timestamp}.json`);

            if (fs.existsSync(DATA_FILE)) {
                fs.copyFileSync(DATA_FILE, backupPath);
            }
        } catch (error) {
            console.error('Error creating backup:', error);
        }
    }

    getData() {
        return this.data;
    }
}

module.exports = new DataManager();
