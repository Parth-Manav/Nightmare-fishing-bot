const { SlashCommandBuilder, PermissionFlagsBits } = require('discord.js');
const dataManager = require('../data/DataManager');

module.exports = {
    data: new SlashCommandBuilder()
        .setName('setreminderthreshold')
        .setDescription('Set the number of days of inactivity before pinging a member.')
        .addIntegerOption(option =>
            option.setName('days')
                .setDescription('Number of days (e.g., 1 for daily, 3 for every 3 days)')
                .setRequired(true)
                .setMinValue(1))
        .setDefaultMemberPermissions(PermissionFlagsBits.Administrator),
    async execute(interaction) {
        const days = interaction.options.getInteger('days');
        const data = dataManager.getData();
        data.reminderThreshold = days;
        dataManager.saveData();

        await interaction.reply({
            content: `✅ Inactivity threshold set to **${days} days**. Members will be pinged if they haven't fished for ${days} days or more.`,
            flags: 64
        });
    },
};
