// SPDX-License-Identifier: MIT OR Apache-2.0
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('./window-capture.js', import.meta.url), 'utf8');
const plain = value => JSON.parse(JSON.stringify(value));

function framePixels() {
    const width = 12, height = 10, stride = 56;
    const pixels = new Uint8Array(stride * height).fill(81);
    for (let y = 0; y < height; y++)
        for (let x = 0; x < width; x++) pixels[y * stride + x * 4 + 3] = 0;
    for (let y = 3; y <= 6; y++)
        for (let x = 4; x <= 7; x++) pixels.set([32, 64, 96, 128], y * stride + x * 4);
    pixels.set([20, 30, 40, 255], 4 * stride + 5 * 4);
    return {pixels, width, height, stride};
}

function harness() {
    const frame = framePixels();
    const state = {now: 0, sample: null, format: 'RGBA', signals: new Map(), calls: [], writes: [],
        pipelines: [], sessions: 0, mapped: 0, unmapped: 0};
    const Gst = {MapFlags: {READ: 1}, State: {PLAYING: 'playing', NULL: 'null'},
        StateChangeReturn: {FAILURE: 'failure', SUCCESS: 'success'}, init() {},
        parse_launch(command) {
            const pipeline = {command, actions: [], handler: null,
                get_by_name: () => ({try_pull_sample: () => state.sample}),
                get_bus() { return this; },
                add_signal_watch() { this.actions.push('watch'); },
                connect(_signal, handler) { this.handler = handler; this.actions.push('connect'); return 1; },
                disconnect() { this.handler = null; this.actions.push('disconnect'); },
                remove_signal_watch() { this.actions.push('unwatch'); },
                set_state(value) { this.actions.push(value); return state.startFailure && value === 'playing' ? 'failure' : 'success'; }};
            state.pipelines.push(pipeline);
            return pipeline;
        }};
    const session = {
        call_sync(_destination, path, _iface, method) {
            state.calls.push({path, method});
            if (method === 'RecordWindow' && state.recordFailure) throw Error('Window not found');
            if (method === 'Stop' && state.stopFailure) throw Error('Session stop failed');
            if (method === 'Start') {
                for (const callback of state.signals.values())
                    callback(null, null, null, null, null, {deep_unpack: () => [42]});
            }
            return {deep_unpack: () => method === 'CreateSession' ? [`/session/${++state.sessions}`] : ['/stream']};
        },
        signal_subscribe(_destination, _iface, _signal, _stream, _arg, _flags, callback) {
            const id = state.signals.size + 1;
            state.signals.set(id, callback);
            return id;
        },
        signal_unsubscribe(id) { assert.ok(state.signals.delete(id)); },
    };
    const GLib = {get_monotonic_time: () => state.now, Variant: class {}, Bytes: class {},
        mkdir_with_parents() {}, file_set_contents(path, value) { state.writes.push({path, value}); }};
    const context = vm.createContext({imports: {gi: {GLib, Gst, GstApp: {},
        Gio: {DBus: {session}, DBusCallFlags: {NONE: 0}, DBusSignalFlags: {NONE: 0}},
        GstVideo: {VideoInfo: {new_from_caps: () => ({...frame, stride: [frame.stride]})}},
        GdkPixbuf: {Colorspace: {RGB: 0}, Pixbuf: {new_from_bytes: () => ({savev(path) {
            if (state.saveFailure) throw Error('PNG write failed');
            state.writes.push({path});
        }})}},
    }}});
    vm.runInContext(source, context);
    state.frame = {get_caps: () => ({get_structure: () => ({get_string: () => state.format})}),
        get_buffer: () => ({map() { state.mapped++; return [true, {data: frame.pixels}]; },
            unmap() { state.unmapped++; }})};
    return {context, state, frame, capture: new context.WindowCapture('/captures')};
}

function assertClosed(state, capture) {
    assert.equal(capture.active, null);
    assert.equal(state.signals.size, 0);
    for (const pipeline of state.pipelines)
        assert.deepEqual(pipeline.actions.slice(-3), ['disconnect', 'unwatch', 'null']);
    assert.equal(state.calls.at(-1).method, 'Stop');
}

test('crop keeps native alpha extents and padding, respects stride and converts premultiplied pixels', () => {
    const {context, frame} = harness();
    const result = context.cropWindowPixels(frame.pixels, frame.width, frame.height, frame.stride);
    assert.deepEqual(plain(result.metadata.crop), {x: 2, y: 1, width: 8, height: 8});
    assert.deepEqual(Array.from(result.pixels.slice((2 * 8 + 2) * 4, (2 * 8 + 2) * 4 + 4)), [64, 128, 191, 128]);
    assert.deepEqual(Array.from(result.pixels.slice((3 * 8 + 3) * 4, (3 * 8 + 3) * 4 + 4)), [20, 30, 40, 255]);
    assert.deepEqual(Array.from(result.pixels.slice(0, 4)), [0, 0, 0, 0]);
    assert.deepEqual(plain(result.metadata.alpha), {transparent: 48, partial: 15, opaque: 1, corners: [0, 0, 0, 0]});
});

test('crop rejects empty, opaque and clipped-shadow captures at every edge', () => {
    const {context, frame} = harness();
    const crop = pixels => context.cropWindowPixels(pixels, frame.width, frame.height, frame.stride);
    assert.throws(() => crop(new Uint8Array(frame.pixels.length)), /requires transparent corners/);
    assert.throws(() => crop(new Uint8Array(frame.pixels.length).fill(255)), /requires transparent corners/);
    for (const [x, y] of [[1, 4], [10, 4], [5, 1], [5, 8]]) {
        const clipped = frame.pixels.slice();
        clipped.set([0, 0, 0, 128], y * frame.stride + x * 4);
        assert.throws(() => crop(clipped), /clips its native shadow/);
    }
});

test('unsafe stems fail before creating any compositor resources', () => {
    const {state, capture} = harness();
    for (const stem of [null, 123, '', '../escape', 'upperCase', '-leading', 'trailing-', 'two--dashes'])
        assert.throws(() => capture.capture(stem), /Invalid native window capture/);
    assert.equal(state.calls.length, 0);
});

test('success writes matching PNG and metadata and cleans each independent session', () => {
    const {state, capture} = harness();
    state.sample = state.frame;
    for (const name of ['first-window', 'second-window']) {
        assert.equal(capture.capture(name), true);
        assertClosed(state, capture);
    }
    assert.equal(state.sessions, 2);
    assert.equal(state.mapped, 2);
    assert.equal(state.unmapped, 2);
    assert.equal(capture.error, null);
    assert.deepEqual(state.writes.map(write => write.path), ['/captures/first-window.png',
        '/captures/first-window.json', '/captures/second-window.png', '/captures/second-window.json']);
    const metadata = JSON.parse(state.writes[1].value);
    assert.deepEqual(metadata.dimensions, [8, 8]);
    assert.deepEqual(metadata.raw_size, [12, 10]);
    assert.match(state.pipelines[0].command, /pipewiresrc path=42 ! video\/x-raw,format=BGRA ! videoconvert ! video\/x-raw,format=RGBA/);
});

test('pending capture times out and releases pipeline, bus watch, signal and session', () => {
    const {state, capture} = harness();
    assert.equal(capture.capture('timeout'), false);
    state.now = 15000000;
    assert.throws(() => capture.capture('timeout'), /timed out: timeout/);
    assertClosed(state, capture);
    assert.equal(state.writes.length, 0);
});

test('RecordWindow, pipeline startup and pipeline bus errors propagate with cleanup', () => {
    for (const failure of ['recordFailure', 'startFailure', 'busFailure']) {
        const {state, capture} = harness();
        if (failure === 'busFailure') {
            assert.equal(capture.capture('failure'), false);
            state.pipelines[0].handler(null, {parse_error: () => ['PipeWire failed', 'details']});
        } else state[failure] = true;
        assert.throws(() => capture.capture('failure'), /Window not found|Cannot start|PipeWire failed/);
        assertClosed(state, capture);
        assert.equal(state.writes.length, 0);
    }
});

test('format, empty-frame and PNG errors propagate, mapped buffers unmap and resources close', () => {
    for (const failure of ['format', 'empty', 'saveFailure']) {
        const {state, frame, capture} = harness();
        state.sample = state.frame;
        if (failure === 'format') state.format = 'RGB';
        else if (failure === 'empty') frame.pixels.fill(0);
        else state.saveFailure = true;
        assert.throws(() => capture.capture('failure'), /negotiate RGBA|requires transparent corners|PNG write failed/);
        assertClosed(state, capture);
        assert.equal(state.mapped, state.unmapped);
        assert.equal(state.writes.length, 0);
    }
});

test('process-finish cleanup is idempotent and session-stop errors remain failures', () => {
    const {state, capture} = harness();
    assert.equal(capture.capture('pending'), false);
    capture.close();
    assertClosed(state, capture);
    const calls = state.calls.length;
    capture.close();
    assert.equal(state.calls.length, calls);
    const second = harness();
    second.state.sample = second.state.frame;
    second.state.stopFailure = true;
    assert.throws(() => second.capture.capture('failure'), /Session stop failed/);
    assertClosed(second.state, second.capture);
});
