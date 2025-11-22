const { SlashCommandBuilder, PermissionFlagsBits } = require('discord.js');
const dataManager = require('../data/DataManager');

module.exports = {
    data: new SlashCommandBuilder()
        .setName('setsummarychannel')
        .setDescription('Set the channel for daily summaries')
        .setDefaultMemberPermissions(PermissionFlagsBits.Administrator),
    async execute(interaction) {
        const data = dataManager.getData();
        data.summaryChannelId = interaction.channelId;
        data.guildId = interaction.guildId;
        dataManager.saveData();

        await interaction.reply({
            content: `✅ Daily summaries will be posted in this channel!`,
            flags: 64
        });
    },
};
