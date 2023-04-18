const IRC = require('irc-framework');
const axios = require('axios');

var bot = new IRC.Client();
var apiBase = 'http://streamer:9090';
var radioBase = 'http://icecast2:8000';

bot.connect({
	host: process.env.IRC_SERVER,
	port: process.env.IRC_PORT,
	secure: true,
	rejectUnauthorized: false,
	nick: process.env.IRC_NICK
});

bot.on('registered', function(event) {
    var chans = process.env.IRC_CHANNELS.split(",");
    chans.forEach(chan => {
        bot.join(chan);
    });
});

bot.on('message', function(event) {
    var args = event.message.split(' ');
    var text = args.slice(1).join(' ');
    var cmd = args[0].substring(1);

    if (event.message.match(/^!(queue|play) /)) {
        spotifyApi('/search/track/1?q='+text, event, data => {
            spotifyApi('/'+cmd+'/'+data[0].id, event, data => {
                return _nowPlaying(data, event);
            });
        });
    }

   if (event.message.match(/^!skip/)) {
        spotifyApi('/skip', event, data => {
            return _nowPlaying(data, event);
        });
    }


    if (event.message.match(/^!np/)) {
        return _nowPlaying(false, event);
    }

    if (event.message.match(/^!(url|link)/)) {
        spotifyApi("/np", event, data => {
            event.reply("https://open.spotify.com/track/"+data.id);
        });
    }

    if (event.message.match(/^!shuffle/)) {
        spotifyApi("/shuffle", event, data => {
            event.reply("shuffled");
        });
    }

    if (event.message.match(/^!search /)) {
        var output = [];

        spotifyApi('/search/track/5?q='+text, event, data => {
            data.forEach(item => {
                var track = {};
                track.track = item.name;
                track.artists = item.artists.map(a => a.name);
                output.push(track);
            });
            output.forEach(item => {
                console.log(item);
                event.reply(makeSpotifyMessage(item));
            });
        });
    }

});

function _nowPlaying(data, event){
    getListenerNumber(event, listeners => {
        if(data)
            return event.reply(makeSpotifyMessage(data, listeners));
        spotifyApi("/np", event, data => {
            event.reply(makeSpotifyMessage(data, listeners));
        });
    });
}

function getListenerNumber(event, after){
    axios.get(radioBase+'/status-json.xsl').then(res => {
        if(!res||!res.data||!res.data.icestats||!res.data.icestats.source)
            return event.reply('couldnt get radio stats, is it down?');
        after(res.data.icestats.source.listeners);
    }).catch(err => {
        event.reply('radio is down');
    });
}

function spotifyApi(path, event, after){
    axios.get(apiBase+path).then(res => {
        handleUnknownError(res, event, data => {
            after(data);
        });
    }).catch(err => {
        handleHttpError(err, event);
    });
}

function handleUnknownError(res, event, otherwise){
    if(!res||!res.data) return event.reply('unknown error');
    if(res.data.error) return event.reply(res.data.error);
    otherwise(res.data);
}

function handleHttpError(err, event){
    if(!err||!err.response||!err.response.data||!err.response.data.error)
        return event.reply('unknown error');
    event.reply('error: '+err.response.data.error);
}

function makeSpotifyMessage(item, listeners){
    if(!item||!item.track||!item.artists)
        return 'search error';

    var msg = "\x03\x31,9 \u266A \x03 \x03\x31,15 " + item.track + " \x03 by";
        msg += " \x03\x31,15 " + item.artists.join(', ') + " \x03";

    if(listeners)
        msg += " \x03\x31,9 "+listeners+" \x03";

    return msg;
}

