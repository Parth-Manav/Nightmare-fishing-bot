const { ActionRowBuilder, ButtonBuilder, ButtonStyle } = require('discord.js');
const dataManager = require('../data/DataManager');
const { getDateString, getYesterdayDateString, shouldReset } = require('../utils/timeUtils');

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
                    nonFishers = Array.from(role.members.filter(member => !fishedTodayIds.includes(member.id)).values());
                    missedCount = nonFishers.length;
                }
            } else {
                const persistentUserIds = Object.keys(data.persistentUsers);
                const nonFishingPersistent = persistentUserIds.filter(id => !fishedTodayIds.includes(id));
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

            let message = '🐠 **Daily Guild Aquarium Contributions** 🐠\n\n';
            message += `Total Catches today: **${totalCatches}**\n`;
            if (data.trackedRoleId) {
                message += `Members who didn't fish: **${missedCount}**\n\n`;
            }

            if (bestAnglers.length > 0) {
                message += `**🏆 Best Anglers (${data.bestAnglerStreak}+ Day streak)**\n`;
                for (const angler of bestAnglers) {
                    message += `・${angler.username} caught ${angler.totalCatches} 🐟, they are on a ${angler.streak} day streak!\n`;
                }
                message += '\n';
            }

            if (nonFishers.length > 0) {
                message += '**Members Who Haven\'t Fished Today 🎣**\n';
                if (data.pingReminderEnabled) {
                    message += nonFishers.map(member => `<@${member.id}>`).join(' ');
                } else {
                    const nicknames = nonFishers.map(member => member.displayName).join(', ');
                    message += nicknames;
                }
                message += '\n\n';
            }

            message += 'We miss you ❤️ \nPlease remember to fish daily 🙏🏻 Many lovely cats, cosmic dolphins and diamond rewards await us all 💎✨';

            await channel.send(message);
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
        const oldButtonMessageId = data.buttonMessageId;
        const oldButtonChannelId = data.buttonChannelId;

        let replyMessage = `${username} has fished! 🐟🎣 Total catches today: ${data.dailyCount}`;
        if (currentStreak > 1) {
            replyMessage += `\n🔥 ${currentStreak} day streak!`;
        }

        await interaction.reply({
            content: replyMessage
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
