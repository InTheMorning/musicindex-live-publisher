// MusicIndex V4V Connector. This mapping sends the deck state to
// mixxx-now-playing with the protocol in musicindex-live-publisher ADR 0006.
// It sends only control change messages on MIDI channel 16.

var V4VConnector = {};

V4VConnector.STATUS = 0xBF;
V4VConnector.PROTOCOL_VERSION = 1;
V4VConnector.CC_HEARTBEAT = 1;
V4VConnector.CC_LOUDEST = 2;
V4VConnector.CC_PLAY = 10;
V4VConnector.CC_TRACK_LOADED = 20;
V4VConnector.CC_DURATION_HIGH = 30;
V4VConnector.CC_DURATION_LOW = 40;
V4VConnector.DECKS = 4;
V4VConnector.MAX_DURATION = 16383;
V4VConnector.HEARTBEAT_MS = 1000;
V4VConnector.LOUDEST_MS = 250;
// The "play" callback also computes the loudest deck.
V4VConnector.LOUDEST_CONTROLS = ["pregain", "volume", "orientation"];

V4VConnector.timers = [];
V4VConnector.connections = [];
V4VConnector.lastLoudest = -1;

V4VConnector.group = function(deck) {
    return "[Channel" + deck + "]";
};

V4VConnector.send = function(control, value) {
    midi.sendShortMsg(V4VConnector.STATUS, control, value);
};

// Gives the loudest deck that plays, 1 to 4, or 0 for none. This is the rule
// of PlayerInfo::updateCurrentPlayingDeck in Mixxx 2.5.6 (ADR 0006 §The
// Loudest Deck). Keep it the same as that rule.
V4VConnector.loudestDeck = function(decks, crossfader) {
    var left = crossfader > 0 ? 1 - crossfader : 1;
    var right = crossfader < 0 ? 1 + crossfader : 1;
    left = Math.max(left, 0);
    right = Math.max(right, 0);

    var best = 0;
    var bestValue = 0;
    for (var i = 0; i < decks.length; i++) {
        var deck = decks[i];
        if (deck.play === 0 || deck.pregain <= 0.25 || deck.volume === 0) {
            continue;
        }
        var gain = 1;
        if (deck.orientation === 0) {
            gain = left;
        } else if (deck.orientation === 2) {
            gain = right;
        }
        var value = deck.volume * gain;
        // Only a strictly higher value wins, so a tie gives the lower deck.
        if (value > bestValue) {
            best = i + 1;
            bestValue = value;
        }
    }
    return best;
};

// Gives [high, low]: whole seconds, rounded down and limited to 14 bits.
V4VConnector.durationParts = function(seconds) {
    var whole = Math.floor(seconds);
    if (!(whole > 0)) {
        whole = 0;
    }
    whole = Math.min(whole, V4VConnector.MAX_DURATION);
    return [(whole >> 7) & 0x7F, whole & 0x7F];
};

V4VConnector.readDecks = function() {
    var decks = [];
    for (var deck = 1; deck <= V4VConnector.DECKS; deck++) {
        var group = V4VConnector.group(deck);
        decks.push({
            play: engine.getValue(group, "play"),
            pregain: engine.getValue(group, "pregain"),
            volume: engine.getValue(group, "volume"),
            orientation: engine.getValue(group, "orientation"),
        });
    }
    return decks;
};

V4VConnector.computeLoudest = function() {
    return V4VConnector.loudestDeck(
        V4VConnector.readDecks(),
        engine.getValue("[Master]", "crossfader"));
};

V4VConnector.sendLoudest = function(loudest) {
    V4VConnector.lastLoudest = loudest;
    V4VConnector.send(V4VConnector.CC_LOUDEST, loudest);
};

V4VConnector.updateLoudest = function() {
    var loudest = V4VConnector.computeLoudest();
    if (loudest !== V4VConnector.lastLoudest) {
        V4VConnector.sendLoudest(loudest);
    }
};

V4VConnector.sendPlay = function(deck, value) {
    V4VConnector.send(V4VConnector.CC_PLAY + deck, value !== 0 ? 127 : 0);
};

V4VConnector.sendTrackLoaded = function(deck, value) {
    V4VConnector.send(V4VConnector.CC_TRACK_LOADED + deck, value !== 0 ? 127 : 0);
};

// The high part goes first. The consumer applies the value when the low part
// arrives.
V4VConnector.sendDuration = function(deck, seconds) {
    var parts = V4VConnector.durationParts(seconds);
    V4VConnector.send(V4VConnector.CC_DURATION_HIGH + deck, parts[0]);
    V4VConnector.send(V4VConnector.CC_DURATION_LOW + deck, parts[1]);
};

V4VConnector.sendCompleteState = function() {
    for (var deck = 1; deck <= V4VConnector.DECKS; deck++) {
        var group = V4VConnector.group(deck);
        V4VConnector.sendPlay(deck, engine.getValue(group, "play"));
        V4VConnector.sendTrackLoaded(deck, engine.getValue(group, "track_loaded"));
        V4VConnector.sendDuration(deck, engine.getValue(group, "duration"));
    }
    V4VConnector.sendLoudest(V4VConnector.computeLoudest());
};

V4VConnector.connect = function(group, key, callback) {
    V4VConnector.connections.push(engine.makeConnection(group, key, callback));
};

V4VConnector.connectDeck = function(deck) {
    var group = V4VConnector.group(deck);
    // Mixxx calls a connection callback as callback(value, group, key).
    V4VConnector.connect(group, "play", function(value) {
        V4VConnector.sendPlay(deck, value);
        V4VConnector.updateLoudest();
    });
    V4VConnector.connect(group, "track_loaded", function(value) {
        V4VConnector.sendTrackLoaded(deck, value);
    });
    V4VConnector.connect(group, "duration", function(value) {
        V4VConnector.sendDuration(deck, value);
    });
    for (var i = 0; i < V4VConnector.LOUDEST_CONTROLS.length; i++) {
        V4VConnector.connect(group, V4VConnector.LOUDEST_CONTROLS[i], V4VConnector.updateLoudest);
    }
};

V4VConnector.init = function() {
    for (var deck = 1; deck <= V4VConnector.DECKS; deck++) {
        V4VConnector.connectDeck(deck);
    }
    V4VConnector.connect("[Master]", "crossfader", V4VConnector.updateLoudest);
    V4VConnector.sendCompleteState();
    V4VConnector.timers.push(engine.beginTimer(V4VConnector.HEARTBEAT_MS, function() {
        V4VConnector.send(V4VConnector.CC_HEARTBEAT, V4VConnector.PROTOCOL_VERSION);
    }));
    V4VConnector.timers.push(engine.beginTimer(V4VConnector.LOUDEST_MS, V4VConnector.updateLoudest));
};

V4VConnector.shutdown = function() {
    for (var i = 0; i < V4VConnector.timers.length; i++) {
        engine.stopTimer(V4VConnector.timers[i]);
    }
    V4VConnector.timers = [];
    for (var j = 0; j < V4VConnector.connections.length; j++) {
        V4VConnector.connections[j].disconnect();
    }
    V4VConnector.connections = [];
};

// A consumer sends CC 1 with the value 1 to ask for the complete state.
V4VConnector.request = function(channel, control, value) {
    if (value === 1) {
        V4VConnector.sendCompleteState();
    }
};
