const { SlashCommandBuilder, PermissionFlagsBits } = require('discord.js');
const dataManager = require('../data/DataManager');

module.exports = {
    data: new SlashCommandBuilder()
        .setName('togglereminder')
        .setDescription('Enable or disable pinging members in the daily fishing reminder.')
        .addBooleanOption(option =>
            option.setName('enabled')
                .setDescription('Set to true to enable pings, false to disable (shows nicknames instead).')
                .setRequired(true))
        .setDefaultMemberPermissions(PermissionFlagsBits.Administrator),
    async execute(interaction) {
        const enabled = interaction.options.getBoolean('enabled');
        const data = dataManager.getData();
        data.pingReminderEnabled = enabled;
        dataManager.saveData();

        await interaction.reply({
            content: `✅ Daily reminder pings have been ${enabled ? 'ENABLED' : 'DISABLED'}.`,
            flags: 64
        });
    },
};
