const { SlashCommandBuilder, PermissionFlagsBits } = require('discord.js');
const dataManager = require('../data/DataManager');

module.exports = {
    data: new SlashCommandBuilder()
        .setName('setbestanglerstreak')
        .setDescription('Set the minimum streak for the Best Anglers list.')
        .addIntegerOption(option =>
            option.setName('streak')
                .setDescription('The minimum streak required (e.g., 5).')
                .setRequired(true)
                .setMinValue(1))
        .setDefaultMemberPermissions(PermissionFlagsBits.Administrator),
    async execute(interaction) {
        const streak = interaction.options.getInteger('streak');
        const data = dataManager.getData();
        data.bestAnglerStreak = streak;
        dataManager.saveData();

        await interaction.reply({
            content: `✅ Best Angler minimum streak set to **${streak}** days.`,
            flags: 64
        });
    },
};
