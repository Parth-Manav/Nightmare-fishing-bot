const { SlashCommandBuilder, PermissionFlagsBits } = require('discord.js');
const dataManager = require('../data/DataManager');

module.exports = {
    data: new SlashCommandBuilder()
        .setName('fishsummary')
        .setDescription('Get a summary of who has not fished today (for the tracked role).')
        .setDefaultMemberPermissions(PermissionFlagsBits.Administrator),
    async execute(interaction) {
        const data = dataManager.getData();

        if (!data.trackedRoleId) {
            return interaction.reply({
                content: '❌ No role is being tracked. Use `/setrole` to set one.',
                flags: 64
            });
        }

        const guild = await interaction.client.guilds.fetch(interaction.guildId);
        await guild.members.fetch();
        const role = await guild.roles.fetch(data.trackedRoleId);

        if (!role) {
            return interaction.reply({
                content: '❌ The tracked role could not be found. Please set it again with `/setrole`.',
                flags: 64
            });
        }

        const fishedTodayIds = Object.keys(data.users);
        const roleMembers = role.members;

        const nonFishers = roleMembers.filter(member => !fishedTodayIds.includes(member.id));

        if (nonFishers.size === 0) {
            return interaction.reply({
                content: `🎉 All members of the **${role.name}** role have fished today!`,
                flags: 64
            });
        }

        const nonFisherMentions = nonFishers.map(member => `<@${member.id}>`).join('\n');

        await interaction.reply({
            content: `**Members of the ${role.name} role who have not fished today:**\n${nonFisherMentions}`,
            flags: 64
        });
    },
};
