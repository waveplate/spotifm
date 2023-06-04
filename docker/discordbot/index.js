const { Client, GatewayIntentBits } = require('discord.js');
const { joinVoiceChannel, createAudioPlayer, createAudioResource, entersState, VoiceConnectionStatus } = require('@discordjs/voice');
const axios = require('axios');
const fs = require('fs');

const configData = fs.readFileSync('/etc/discordbot.json');
const discordOpts = JSON.parse(configData);

const apiBase = 'http://streamer:9090';

const client = new Client({
    intents: [
        GatewayIntentBits.Guilds,
        GatewayIntentBits.GuildMessages,
        GatewayIntentBits.MessageContent,
        GatewayIntentBits.GuildMembers,
        GatewayIntentBits.GuildVoiceStates,
    ],
});

client.on('ready', () => {
    console.log(`Bot is ready!`);

    channel = client.channels.cache.get(discordOpts.voiceChannelId);
            
    const connection = joinVoiceChannel({
        channelId: channel.id,
        guildId: channel.guild.id,
        adapterCreator: channel.guild.voiceAdapterCreator,
    });

    try {
        entersState(connection, VoiceConnectionStatus.Ready, 20e3).then(() => {
            const player = createAudioPlayer();
            const resource = createAudioResource('http://icecast2:8000/listen');
            
            player.play(resource);
  
            connection.subscribe(player);
        });
    } catch (error) {
        console.error(error);
    }
});

client.on('messageCreate', message => {
    let parts = message.content.split(' ');
    let text = parts.slice(1).join(' ');
    let cmd = parts[0].substring(1);

    if(['np', 'skip', 'prev', 'next'].indexOf(cmd) > -1){
        axios.get(`${apiBase}/${cmd}`).then(response => {
            message.channel.send(makeSpotifyText(response.data));
        });
    } else if(['play', 'queue'].indexOf(cmd) > -1 && text){
        axios.get(`${apiBase}/search/track/1?q=${text}`).then(search => {
            if(search && search.data && search.data.length > 0){
                axios.get(`${apiBase}/${parts[0].substring(1)}/${search.data[0].id}`).then(response => {
                    message.channel.send(makeSpotifyText(response.data));
                });
            }
        });
    } else if(['search'].indexOf(cmd) > -1 && text){
        axios.get(`${apiBase}/search/track/5?q=${text}`).then(search => {
            if(search && search.data && search.data.length > 0){
                search.data.forEach(track => {
                    let trackInfo = {
                        track: track.name,
                        artists: track.artists.map(artist => artist.name),
                    }
                    message.channel.send(makeSpotifyText(trackInfo));
                });
            }
        });
    }
});

client.login(discordOpts.token);

function makeSpotifyText(jsonData){
    if(jsonData && jsonData.track && jsonData.artists){
        let track = jsonData.track;
        let artists = jsonData.artists;
        return `\`${track}\` by __\`${artists.join('\`__ __\`')}\`__`;
    }
    return 'error generating track info';
}