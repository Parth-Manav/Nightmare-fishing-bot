const { SlashCommandBuilder, ActionRowBuilder, ButtonBuilder, ButtonStyle } = require('discord.js');
const dataManager = require('../data/DataManager');

module.exports = {
    data: new SlashCommandBuilder()
        .setName('fishsetup')
        .setDescription('Set up the fishing pond'),
    async execute(interaction) {
        const row = new ActionRowBuilder()
            .addComponents(
                new ButtonBuilder()
                    .setCustomId('fish_button')
                    .setLabel('🎣 Fish!')
                    .setStyle(ButtonStyle.Primary)
            );

        const reply = await interaction.reply({
            content: '🎣 Welcome to Stardust Pond — click to fish!',
            components: [row],
            fetchReply: true
        });

        const data = dataManager.getData();
        data.buttonMessageId = reply.id;
        data.buttonChannelId = interaction.channelId;
        data.guildId = interaction.guildId;
        dataManager.saveData();
    },
};
