// Tests for the MusicIndex V4V Connector mapping script (ADR 0006). The script
// runs in a node:vm context with a stub engine and a stub midi object.

import { test } from "node:test";
import { strict as assert } from "node:assert";
import { readFileSync } from "node:fs";
import { createContext, runInContext } from "node:vm";

const SCRIPT = readFileSync(
    new URL("../MusicIndex-V4V-Connector.js", import.meta.url), "utf-8");

// A stub of the Mixxx script engine. A connection callback gets
// (value, group, key), the same as Mixxx 2.5.6 scriptconnection.cpp.
function load() {
    const values = new Map();
    const connections = [];
    const timers = new Map();
    let nextTimer = 1;
    const sent = [];

    const engine = {
        getValue: (group, key) => values.get(`${group},${key}`) ?? 0,
        makeConnection: (group, key, callback) => {
            const connection = {
                group, key, callback, connected: true,
                disconnect() { this.connected = false; return true; },
            };
            connections.push(connection);
            return connection;
        },
        beginTimer: (ms, callback) => {
            const id = nextTimer++;
            timers.set(id, { ms, callback });
            return id;
        },
        stopTimer: (id) => { timers.delete(id); },
    };
    const midi = { sendShortMsg: (status, control, value) => sent.push([status, control, value]) };
    const context = createContext({ engine, midi, print: () => {} });
    runInContext(SCRIPT, context);

    return {
        V4V: context.V4VConnector,
        sent,
        timers,
        connections,
        // Sets a control and calls each connected callback, as Mixxx does.
        set(group, key, value) {
            values.set(`${group},${key}`, value);
            for (const c of connections) {
                if (c.connected && c.group === group && c.key === key) {
                    c.callback(value, group, key);
                }
            }
        },
        clear() { sent.length = 0; },
    };
}

const deck = (play, volume, orientation = 1, pregain = 1) =>
    ({ play, pregain, volume, orientation });
const silent = () => deck(0, 1);

test("loudestDeck: crossfader -1 silences the right side", (t) => {
    const { V4V } = load();
    // Deck 1 left, deck 2 right, both play at full volume.
    assert.equal(V4V.loudestDeck([deck(1, 1, 0), deck(1, 1, 2), silent(), silent()], -1), 1);
});

test("loudestDeck: crossfader 1 silences the left side", () => {
    const { V4V } = load();
    assert.equal(V4V.loudestDeck([deck(1, 1, 0), deck(1, 1, 2), silent(), silent()], 1), 2);
});

test("loudestDeck: crossfader 0 gives both sides the gain 1", () => {
    const { V4V } = load();
    // Deck 2 on the right is louder by volume.
    assert.equal(V4V.loudestDeck([deck(1, 0.5, 0), deck(1, 0.8, 2), silent(), silent()], 0), 2);
    // Deck 1 on the left is louder by volume.
    assert.equal(V4V.loudestDeck([deck(1, 0.8, 0), deck(1, 0.5, 2), silent(), silent()], 0), 1);
});

test("loudestDeck: a partial crossfader scales one side only", () => {
    const { V4V } = load();
    // x = 0.5: left gain 0.5, right gain 1. Left 1.0 * 0.5 < right 0.6 * 1.
    assert.equal(V4V.loudestDeck([deck(1, 1, 0), deck(1, 0.6, 2), silent(), silent()], 0.5), 2);
    // x = -0.5: right gain 0.5. Right 1.0 * 0.5 < left 0.6.
    assert.equal(V4V.loudestDeck([deck(1, 0.6, 0), deck(1, 1, 2), silent(), silent()], -0.5), 1);
});

test("loudestDeck: a center deck ignores the crossfader", () => {
    const { V4V } = load();
    // Crossfader full left. A center deck at 0.3 beats a right deck.
    assert.equal(V4V.loudestDeck([silent(), deck(1, 1, 2), deck(1, 0.3, 1), silent()], -1), 3);
    // Crossfader full right. A center deck at 0.3 beats a left deck.
    assert.equal(V4V.loudestDeck([deck(1, 1, 0), silent(), deck(1, 0.3, 1), silent()], 1), 3);
});

test("loudestDeck: a deck at gain 0 alone gives 0", () => {
    const { V4V } = load();
    assert.equal(V4V.loudestDeck([deck(1, 1, 0), silent(), silent(), silent()], 1), 0);
});

test("loudestDeck: pregain 0.25 does not count, 0.26 counts", () => {
    const { V4V } = load();
    assert.equal(V4V.loudestDeck([deck(1, 1, 1, 0.25), silent(), silent(), silent()], 0), 0);
    assert.equal(V4V.loudestDeck([deck(1, 1, 1, 0.26), silent(), silent(), silent()], 0), 1);
});

test("loudestDeck: volume 0 does not count", () => {
    const { V4V } = load();
    assert.equal(V4V.loudestDeck([deck(1, 0), deck(1, 0.1), silent(), silent()], 0), 2);
});

test("loudestDeck: equal values give the lower deck", () => {
    const { V4V } = load();
    assert.equal(V4V.loudestDeck([silent(), deck(1, 0.7), deck(1, 0.7), silent()], 0), 2);
});

test("loudestDeck: no deck that plays gives 0", () => {
    const { V4V } = load();
    assert.equal(V4V.loudestDeck([silent(), silent(), silent(), silent()], 0), 0);
});

test("durationParts", () => {
    const { V4V } = load();
    const cases = [[0, [0, 0]], [127, [0, 127]], [128, [1, 0]], [16383, [127, 127]],
        [20000, [127, 127]], [200.9, [1, 72]], [-1, [0, 0]]];
    for (const [seconds, parts] of cases) {
        assert.deepEqual(Array.from(V4V.durationParts(seconds)), parts, `seconds ${seconds}`);
    }
});

function expectedCompleteState(loudest) {
    const messages = [];
    for (let n = 1; n <= 4; n++) {
        messages.push([0xBF, 10 + n, 0], [0xBF, 20 + n, 0], [0xBF, 30 + n, 0], [0xBF, 40 + n, 0]);
    }
    messages.push([0xBF, 2, loudest]);
    return messages;
}

test("init sends the complete state and starts two timers", () => {
    const m = load();
    m.V4V.init();
    assert.deepEqual(m.sent, expectedCompleteState(0));
    assert.deepEqual([...m.timers.values()].map((t) => t.ms).sort((a, b) => a - b), [250, 1000]);
});

test("the complete state carries the deck values", () => {
    const m = load();
    m.set("[Channel3]", "play", 1);
    m.set("[Channel3]", "track_loaded", 1);
    m.set("[Channel3]", "duration", 200.02);
    m.set("[Channel3]", "volume", 1);
    m.set("[Channel3]", "pregain", 1);
    m.V4V.init();
    const deck3 = m.sent.filter(([, cc]) => [13, 23, 33, 43].includes(cc));
    assert.deepEqual(deck3, [[0xBF, 13, 127], [0xBF, 23, 127], [0xBF, 33, 1], [0xBF, 43, 72]]);
    assert.deepEqual(m.sent.at(-1), [0xBF, 2, 3]);
});

test("the heartbeat timer sends CC 1 with version 1", () => {
    const m = load();
    m.V4V.init();
    m.clear();
    [...m.timers.values()].find((t) => t.ms === 1000).callback();
    assert.deepEqual(m.sent, [[0xBF, 1, 1]]);
});

test("request 1 sends the complete state, request 0 sends nothing", () => {
    const m = load();
    m.V4V.init();
    m.clear();
    m.V4V.request(15, 1, 0, 0xBF, "[Master]");
    assert.deepEqual(m.sent, []);
    m.V4V.request(15, 1, 1, 0xBF, "[Master]");
    assert.deepEqual(m.sent, expectedCompleteState(0));
});

test("control changes send the deck messages", () => {
    const m = load();
    m.V4V.init();
    m.clear();
    m.set("[Channel2]", "track_loaded", 1);
    m.set("[Channel2]", "duration", 300.5);
    m.set("[Channel2]", "play", 1);
    m.set("[Channel2]", "play", 0);
    // Volume and pregain are 0, so the loudest deck stays 0 and is not sent.
    assert.deepEqual(m.sent, [[0xBF, 22, 127], [0xBF, 32, 2], [0xBF, 42, 44],
        [0xBF, 12, 127], [0xBF, 12, 0]]);
});

test("the loudest deck is sent on a change only", () => {
    const m = load();
    m.V4V.init();
    m.set("[Channel1]", "volume", 1);
    m.set("[Channel1]", "pregain", 1);
    m.clear();
    const loudestTimer = [...m.timers.values()].find((t) => t.ms === 250);

    loudestTimer.callback();
    assert.deepEqual(m.sent, [], "no change sends nothing");

    m.set("[Channel1]", "play", 1);
    assert.deepEqual(m.sent, [[0xBF, 11, 127], [0xBF, 2, 1]]);
    m.clear();

    loudestTimer.callback();
    assert.deepEqual(m.sent, [], "the same deck again sends nothing");

    m.set("[Channel1]", "orientation", 0);
    m.set("[Master]", "crossfader", 1);
    assert.deepEqual(m.sent, [[0xBF, 2, 0]], "the crossfader change sends deck 0");
});

test("shutdown stops the timers and disconnects each connection", () => {
    const m = load();
    m.V4V.init();
    assert.equal(m.connections.length, 4 * 6 + 1);
    m.V4V.shutdown();
    assert.equal(m.timers.size, 0);
    assert.ok(m.connections.every((c) => !c.connected));
});

test("every message has the status 0xBF", () => {
    const m = load();
    m.V4V.init();
    m.set("[Channel1]", "play", 1);
    [...m.timers.values()].forEach((t) => t.callback());
    m.V4V.request(15, 1, 1, 0xBF, "[Master]");
    assert.ok(m.sent.length > 0);
    assert.ok(m.sent.every(([status, , value]) => status === 0xBF && value >= 0 && value <= 127));
});
