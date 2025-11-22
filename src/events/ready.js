const { Events, REST, Routes } = require('discord.js');
const cron = require('node-cron');
const fs = require('fs');
const path = require('path');
const dataManager = require('../data/DataManager');
const fishingManager = require('../game/FishingManager');

module.exports = {
    name: Events.ClientReady,
    once: true,
    async execute(client) {
        console.log(`✅ Bot is ready! Logged in as ${client.user.tag}`);

        dataManager.loadData();

        // Register Slash Commands
        const commands = [];
        const commandsPath = path.join(__dirname, '../commands');
        const commandFiles = fs.readdirSync(commandsPath).filter(file => file.endsWith('.js'));

        for (const file of commandFiles) {
            const command = require(path.join(commandsPath, file));
            commands.push(command.data.toJSON());
            client.commands.set(command.data.name, command);
        }

        const rest = new REST({ version: '10' }).setToken(process.env.DISCORD_BOT_TOKEN);

        try {
            console.log('Registering slash commands...');
            await rest.put(
                Routes.applicationCommands(client.user.id),
                { body: commands }
            );
            console.log('✅ Slash commands registered successfully!');
        } catch (error) {
            console.error('Error registering slash commands:', error);
        }

        // Schedule Daily Reset
        cron.schedule('30 14 * * *', () => {
            fishingManager.resetDailyData(client);
            console.log('🔄 Scheduled reset at 14:30 GMT (20:00/8 PM in GMT+5:30)');
        }, {
            timezone: 'Etc/GMT'
        });
    },
};
