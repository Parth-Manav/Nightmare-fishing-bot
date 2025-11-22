const { SlashCommandBuilder, PermissionFlagsBits } = require('discord.js');
const dataManager = require('../data/DataManager');

module.exports = {
    data: new SlashCommandBuilder()
        .setName('setrole')
        .setDescription('Set the role to track for fishing (e.g., Nightmare verified member)')
        .addRoleOption(option =>
            option.setName('role')
                .setDescription('The role to track')
                .setRequired(true))
        .setDefaultMemberPermissions(PermissionFlagsBits.Administrator),
    async execute(interaction) {
        const role = interaction.options.getRole('role');
        const data = dataManager.getData();
        data.trackedRoleId = role.id;
        data.guildId = interaction.guildId;
        dataManager.saveData();

        await interaction.reply({
            content: `✅ Now tracking the **${role.name}** role for fishing statistics!`,
            flags: 64
        });
    },
};
