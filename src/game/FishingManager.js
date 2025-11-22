const { ActionRowBuilder, ButtonBuilder, ButtonStyle, EmbedBuilder } = require('discord.js');
const dataManager = require('../data/DataManager');
const { getDateString, getYesterdayDateString, shouldReset, getDaysDifference } = require('../utils/timeUtils');

class FishingManager {
    constructor() {
        this.isResetting = false;
    }

    async postDailySummary(client) {
        const data = dataManager.getData();
        if (!data.summaryChannelId || !data.guildId) {
            console.log('⚠️ Summary channel or guild not configured, skipping summary');
            return;
        }

        try {
            const guild = await client.guilds.fetch(data.guildId);
            const channel = await guild.channels.fetch(data.summaryChannelId);

            if (!channel) {
                console.log('⚠️ Summary channel not found');
                return;
            }

            const totalCatches = data.dailyCount;
            const todayDate = getDateString(Date.now());

            let guildMembers = null;
            if (data.trackedRoleId) {
                guildMembers = await guild.members.fetch();
            }

            const fishedTodayIds = Object.keys(data.users);
            let nonFishers = [];
            let missedCount = 0;

            if (data.trackedRoleId) {
                const role = await guild.roles.fetch(data.trackedRoleId);
                if (role) {
                    // Filter members who haven't fished today AND meet the inactivity threshold
                    nonFishers = Array.from(role.members.filter(member => {
                        // If they fished today, they are safe
                        if (fishedTodayIds.includes(member.id)) return false;

                        // If they haven't fished today, check how long it's been
                        const userData = data.persistentUsers[member.id];

                        // If they have never fished, they are inactive (infinite days > threshold)
                        if (!userData) return true;

                        // Calculate days since last fish
                        const lastFishedDate = userData.lastFishedDate;
                        const daysDiff = getDaysDifference(lastFishedDate, todayDate);

                        // If days since last fish >= threshold, ping them
                        return daysDiff >= data.reminderThreshold;
                    }).values());

                    missedCount = nonFishers.length;
                }
            } else {
                // Fallback logic for no role (based on persistent users)
                const persistentUserIds = Object.keys(data.persistentUsers);
                const nonFishingPersistent = persistentUserIds.filter(id => {
                    if (fishedTodayIds.includes(id)) return false;

                    const userData = data.persistentUsers[id];
                    const lastFishedDate = userData.lastFishedDate;
                    const daysDiff = getDaysDifference(lastFishedDate, todayDate);

                    return daysDiff >= data.reminderThreshold;
                });
                missedCount = nonFishingPersistent.length;
            }

            let bestAnglers = [];
            for (const [userId, userData] of Object.entries(data.persistentUsers)) {
                if (userData.streak >= data.bestAnglerStreak) {
                    if (data.trackedRoleId && guildMembers) {
                        const member = guildMembers.get(userId);
                        if (member && member.roles.cache.has(data.trackedRoleId)) {
                            bestAnglers.push({
                                userId,
                                username: userData.username,
                                streak: userData.streak,
                                totalCatches: userData.totalCatches || 0
                            });
                        }
                    } else if (!data.trackedRoleId) {
                        bestAnglers.push({
                            userId,
                            username: userData.username,
                            streak: userData.streak,
                            totalCatches: userData.totalCatches || 0
                        });
                    }
                }
            }

            bestAnglers.sort((a, b) => b.streak - a.streak || b.totalCatches - a.totalCatches);

            // --- Build Summary Embed ---
            const summaryEmbed = new EmbedBuilder()
                .setColor(0xFFD700) // Gold color
                .setTitle('🐠 Daily Guild Aquarium Contributions')
                .setDescription('Here is how the pond is doing today!')
                .addFields(
                    { name: '🎣 Total Catches Today', value: `**${totalCatches}**`, inline: true },
                    { name: '😴 Members Missed', value: `**${missedCount}**`, inline: true }
                )
                .setTimestamp()
                .setFooter({ text: 'Stardust Pond Daily Summary' });

            if (bestAnglers.length > 0) {
                let anglersText = '';
                for (const angler of bestAnglers) {
                    anglersText += `🏆 **${angler.username}**: ${angler.totalCatches} 🐟 (${angler.streak} day streak)\n`;
                }
                // Discord field limit is 1024 chars, truncate if needed in future
                summaryEmbed.addFields({ name: `🔥 Best Anglers (${data.bestAnglerStreak}+ Day Streak)`, value: anglersText || 'None today!' });
            }

            let pingMessage = '';
            if (nonFishers.length > 0) {
                if (data.pingReminderEnabled) {
                    pingMessage = '**Wake up! You haven\'t fished in a while!** 🎣\n' + nonFishers.map(member => `<@${member.id}>`).join(' ');
                } else {
                    const nicknames = nonFishers.map(member => member.displayName).join(', ');
                    summaryEmbed.addFields({ name: '🎣 Needs to Fish', value: nicknames });
                }
            }

            summaryEmbed.addFields({ name: 'Message', value: 'We miss you ❤️ \nPlease remember to fish daily 🙏🏻 Many lovely cats, cosmic dolphins and diamond rewards await us all 💎✨' });

            await channel.send({ embeds: [summaryEmbed] });

            if (pingMessage) {
                await channel.send(pingMessage);
            }

            console.log('✅ Daily summary posted successfully');
        } catch (error) {
            console.error('Error posting daily summary:', error);
        }
    }

    async resetDailyData(client) {
        if (this.isResetting) {
            console.log('⚠️ Reset already in progress, skipping duplicate call');
            return;
        }

        this.isResetting = true;

        try {
            await this.postDailySummary(client);

            const data = dataManager.getData();
            for (const [userId, userData] of Object.entries(data.persistentUsers)) {
                if (!data.users[userId]) {
                    data.persistentUsers[userId].streak = 0;
                }
            }

            data.dailyCount = 0;
            data.lastResetTimestamp = Date.now();
            data.users = {};
            dataManager.saveData();
            dataManager.backupData(); // Backup after reset

            const localTime = new Date(Date.now() + (5.5 * 60 * 60 * 1000));
            console.log(`Daily data reset at ${localTime.toLocaleString('en-US', { timeZone: 'UTC' })}`);
        } finally {
            this.isResetting = false;
        }
    }

    async handleFishClick(interaction) {
        const userId = interaction.user.id;
        const username = interaction.member.displayName;
        const data = dataManager.getData();

        if (shouldReset(data.lastResetTimestamp)) {
            await this.resetDailyData(interaction.client);
        }

        if (data.users[userId]) {
            await interaction.reply({
                content: `❌ You've already fished today! Come back tomorrow.`,
                flags: 64
            });
            return;
        }

        const todayDate = getDateString(Date.now());
        const yesterdayDate = getYesterdayDateString();

        if (!data.persistentUsers[userId]) {
            data.persistentUsers[userId] = {
                username: username,
                streak: 1,
                lastFishedDate: todayDate,
                totalCatches: 1
            };
        } else {
            const lastFished = data.persistentUsers[userId].lastFishedDate;

            if (lastFished === yesterdayDate) {
                data.persistentUsers[userId].streak++;
            } else if (lastFished !== todayDate) {
                data.persistentUsers[userId].streak = 1;
            }

            data.persistentUsers[userId].lastFishedDate = todayDate;
            data.persistentUsers[userId].username = username;
            data.persistentUsers[userId].totalCatches = (data.persistentUsers[userId].totalCatches || 0) + 1;
        }

        data.users[userId] = {
            username: username,
            fishedAt: new Date().toISOString()
        };
        data.dailyCount++;

        const currentStreak = data.persistentUsers[userId].streak;
        const totalCatches = data.persistentUsers[userId].totalCatches;
        const oldButtonMessageId = data.buttonMessageId;
        const oldButtonChannelId = data.buttonChannelId;

        // --- Build Fish Embed ---
        const fishEmbed = new EmbedBuilder()
            .setColor(0x0099FF) // Blue
            .setTitle('🎣 Catch of the Day!')
            .setDescription(`**${username}** cast their line and caught a fish! 🐟`)
            .setThumbnail(interaction.user.displayAvatarURL({ dynamic: true }))
            .addFields(
                { name: '🔥 Streak', value: `${currentStreak} Days`, inline: true },
                { name: '✨ Total Catches', value: `${totalCatches}`, inline: true }
            )
            .setTimestamp()
            .setFooter({ text: 'Stardust Pond' });

        await interaction.reply({
            embeds: [fishEmbed]
        });

        const row = new ActionRowBuilder()
            .addComponents(
                new ButtonBuilder()
                    .setCustomId('fish_button')
                    .setLabel('🎣 Fish!')
                    .setStyle(ButtonStyle.Primary)
            );

        const newButtonMessage = await interaction.channel.send({
            content: '🎣 Welcome to Stardust Pond — click to fish!',
            components: [row]
        });

        data.buttonMessageId = newButtonMessage.id;
        data.buttonChannelId = interaction.channelId;
        dataManager.saveData();

        try {
            if (oldButtonMessageId && oldButtonChannelId) {
                const channel = await interaction.client.channels.fetch(oldButtonChannelId);
                const oldMessage = await channel.messages.fetch(oldButtonMessageId);
                await oldMessage.delete();
                console.log('✅ Old button message deleted successfully');
            }
        } catch (error) {
            console.log('⚠️ Could not delete old button message:', error.message);
        }
    }
}

module.exports = new FishingManager();
