// SPDX-License-Identifier: MIT OR Apache-2.0
// Test-only virtual pointer through Mutter -> Wayland -> GTK/Chromium. Run ONLY inside
// an isolated dbus-run-session + headless Mutter, never on a user's desktop.
const {Gio, GLib} = imports.gi;
// The private monitor is configured by workspace-motion.sh. RemoteDesktop's
// touch positions are stream pixels; Mutter divides them by monitor scale.
// Relative pointer motion and the test fixtures use logical desktop units.
const touchScale = Number(GLib.getenv('LAYER_MOTION_SCALE') || 1);
if (!Number.isFinite(touchScale) || touchScale < 1 || touchScale > 4)
    throw new Error('Expected private monitor scale 1..4');
if (!GLib.getenv('WAYLAND_DISPLAY')?.startsWith('layer-bench-'))
    throw new Error('Requires an isolated layer-bench-* Wayland display');
const output = GLib.getenv('LAYER_NATIVE_INPUT_DIR');
if (!output) throw new Error('Set LAYER_NATIVE_INPUT_DIR to an empty temporary directory');
// Optional lossless captures of the actual composited monitor, including GSK's
// incremental window damage. WidgetPaintable.render_texture cannot test that.
const captureDir = GLib.getenv('LAYER_NATIVE_CAPTURE_DIR');
let capturePipeline, captureSink;
const captureFrame = name => {
    if (!captureDir || !/^[a-z0-9-]+$/.test(name)) throw Error('Invalid native capture request');
    const sample = captureSink?.try_pull_sample(0);
    if (!sample) return false;
    const {Gst, GstVideo, GdkPixbuf} = imports.gi;
    const info = GstVideo.VideoInfo.new_from_caps(sample.get_caps());
    const buffer = sample.get_buffer();
    const [ok, map] = buffer.map(Gst.MapFlags.READ);
    if (!ok) throw Error('Cannot map compositor capture');
    const pixels = new GLib.Bytes(map.data);
    buffer.unmap(map);
    GdkPixbuf.Pixbuf.new_from_bytes(pixels, GdkPixbuf.Colorspace.RGB, false, 8,
        info.width, info.height, info.stride[0]).savev(`${captureDir}/${name}.png`, 'png', [], []);
    return true;
};
const ready = Gio.File.new_for_path(`${output}/ready`);
if (ready.query_exists(null)) throw new Error('Use a fresh output directory');
const loop = new GLib.MainLoop(null, false);
const nativeTest = ARGV.find(a => a.startsWith('--native-test='))?.slice('--native-test='.length);
if (nativeTest && !/^[a-z0-9_]+$/.test(nativeTest)) throw new Error('Use one exact native test name');
const workspaceDrag = ARGV.includes('--workspace-drag');
const workspaceClicks = ARGV.includes('--workspace-clicks');
const workspaceCursor = ARGV.includes('--workspace-cursor');
const workspaceDrawer = ARGV.includes('--workspace-drawer');
const columnStacks = ARGV.includes('--column-stacks');
const drawerStyle = ARGV.includes('--drawer-style');
const tooltips = ARGV.includes('--tooltips');
const workspaceWindow = ARGV.includes('--workspace-window');
const workspaceColumns = ARGV.includes('--workspace-columns');
const workspaceTabs = ARGV.includes('--workspace-tabs');
const dragPickup = ARGV.includes('--drag-pickup');
const columnDrops = ARGV.includes('--column-drops');
const layerHold = ARGV.includes('--layer-hold');
const workspaceHold = ARGV.includes('--workspace-hold') || layerHold || dragPickup || columnDrops;
const workspaceSwitcher = ARGV.includes('--workspace-switcher');
const iconAudit = ARGV.includes('--icons');
const workspaceTransitions = ARGV.includes('--workspace-transitions');
const workspaceManagerVisual = ARGV.includes('--workspace-manager-visual');
const workspaceMenus = ARGV.includes('--workspace-menus') || workspaceSwitcher || workspaceManagerVisual || workspaceTransitions;
const workspaceResize = ARGV.includes('--workspace-resize') || ARGV.includes('--web-workspace-resize');
const workspaceWeb = ARGV.includes('--web-workspace-motion') || ARGV.includes('--web-workspace-resize') || ARGV.includes('--web-color-panel');
const colorPanel = ARGV.includes('--color-panel') || ARGV.includes('--web-color-panel');
const workspaceDropSizes = ARGV.includes('--workspace-drop-sizes');
const workspaceEdges = ARGV.includes('--workspace-edges');
const workspaceMotion = workspaceDropSizes || workspaceEdges || colorPanel || columnStacks || ARGV.includes('--workspace-motion') || workspaceWeb || workspaceResize;
// A captured executable lets correctness runs identify their exact build and
// avoids an unrelated release rebuild. The ordinary cargo route remains usable.
const capturedExecutable = GLib.getenv('LAYER_NATIVE_TEST_EXECUTABLE');
const testExecutable = capturedExecutable && GLib.canonicalize_filename(capturedExecutable, null);
const launcher = new Gio.SubprocessLauncher({flags: Gio.SubprocessFlags.NONE});
// Column stacks include the real storage lifecycle: maintenance must preserve
// open projections and retained controls while ordinary motion stays incremental.
if ((nativeTest && !ARGV.includes('--native-storage')) || drawerStyle || tooltips || (workspaceMotion && !columnStacks) || workspaceHold) launcher.unsetenv('CAPY_WORKSPACE_DIR');
const launch = [
    ...(workspaceWeb ? ['node', 'apps/layer-web/test.mjs', colorPanel ? '--color-panel' : workspaceResize ? '--workspace-resize' : '--workspace-motion', '--native-input'] : [
        'cargo', 'test', '--release', '-p', 'layer-linux', workspaceDropSizes ? 'native_workspace_drop_sizes' : workspaceEdges ? 'native_workspace_drag_edges' : iconAudit ? 'native_icon_audit' : colorPanel ? 'native_color_panel_input' : workspaceTransitions ? 'native_workspace_transition_stability' : columnStacks ? 'native_column_stack_input' : tooltips ? 'native_tooltip_input' : columnDrops ? 'native_collapsed_divider_drop_input' : dragPickup ? 'native_drag_pickup_input' : workspaceManagerVisual ? 'native_workspace_manager_visual' : workspaceSwitcher ? 'native_workspace_switcher_input' : drawerStyle ? 'native_drawer_style_input' : workspaceResize ? 'native_workspace_resize_input' : layerHold ? 'native_layer_hold_input' : workspaceMenus ? 'native_workspace_menu_input' : workspaceMotion ? 'native_workspace_motion_input' : workspaceHold ? 'native_long_press_drag_input' : workspaceTabs ? 'native_tab_slide_input' : workspaceColumns ? 'native_collapsed_column_input' : workspaceWindow ? 'native_window_drag_input' : workspaceDrawer ? 'native_column_drawer_drag_input' : workspaceCursor ? 'native_divider_cursor_input' : workspaceClicks ? 'native_floating_click_input' : workspaceDrag ? 'native_toolbar_drag_input' : 'native_compositor_input',
        '--', '--ignored', '--test-threads=1', '--nocapture',
    ]),
];
if (nativeTest) {
    // Cargo's filter is a substring match: a short name can accidentally run
    // another case (and its storage/input protocol) in the same process.
    const listing = Gio.Subprocess.new(
        testExecutable ? [testExecutable, '--list'] : ['cargo', 'test', '--locked', '--release', '-p', 'layer-linux', '--', '--list'],
        Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE,
    );
    const [, stdout, stderr] = listing.communicate_utf8(null, null);
    if (!listing.get_successful()) throw Error(`Cannot list native tests: ${stderr}`);
    const matches = stdout.split('\n').filter(line => line.endsWith(': test'))
        .map(line => line.slice(0, -6)).filter(name => name.split('::').at(-1) === nativeTest);
    if (matches.length !== 1) throw Error(`Expected exactly one native test named ${nativeTest}, found ${matches.length}`);
    launch[5] = matches[0];
    launch.push('--exact');
}
if (testExecutable && !workspaceWeb) {
    launch.splice(0, 7, testExecutable, launch[5]);
    launcher.set_cwd('apps/layer-linux');
}
let tabletProxy;
if (ARGV.includes('--tablet')) {
    tabletProxy = Gio.Subprocess.new(['python3', 'apps/layer-linux/bench/tablet-proxy.py'], Gio.SubprocessFlags.NONE);
    const socket = Gio.File.new_for_path(`${GLib.getenv('XDG_RUNTIME_DIR')}/layer-bench-tablet`);
    for (let attempt = 0; attempt < 200 && !socket.query_exists(null); attempt++) GLib.usleep(10000);
    if (!socket.query_exists(null)) throw Error('Tablet proxy did not start');
    launcher.setenv('WAYLAND_DISPLAY', 'layer-bench-tablet', true);
}
const process = launcher.spawnv(launch);
let passed = false;
process.wait_async(null, (p, result) => {
    p.wait_finish(result);
    passed = p.get_successful();
    if (capturePipeline) capturePipeline.set_state(imports.gi.Gst.State.NULL);
    tabletProxy?.force_exit();
    loop.quit();
});
GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
    if (!ready.query_exists(null)) return GLib.SOURCE_CONTINUE;
    const dest = 'org.gnome.Mutter.RemoteDesktop', iface = `${dest}.Session`;
    const call = (path, name, method, signature, values) => Gio.DBus.session.call_sync(
        dest, path, name, method, new GLib.Variant(signature, values), null,
        Gio.DBusCallFlags.NONE, 3000, null,
    ).deep_unpack();
    const session = call('/org/gnome/Mutter/RemoteDesktop', dest, 'CreateSession', '()', [])[0];
    const send = (method, signature, values) => call(session, iface, method, signature, values);
    let touchStream;
    if (nativeTest || workspaceHold || workspaceMotion || workspaceSwitcher || workspaceTransitions) {
        const id = call(session, 'org.freedesktop.DBus.Properties', 'Get', '(ss)', [iface, 'SessionId'])[0].deep_unpack();
        const cast = 'org.gnome.Mutter.ScreenCast';
        const castCall = (path, name, method, signature, values) => Gio.DBus.session.call_sync(
            cast, path, name, method, new GLib.Variant(signature, values), null,
            Gio.DBusCallFlags.NONE, 3000, null,
        ).deep_unpack();
        const path = castCall('/org/gnome/Mutter/ScreenCast', cast, 'CreateSession', '(a{sv})',
            [{'remote-desktop-session-id': new GLib.Variant('s', id)}])[0];
        touchStream = castCall(path, `${cast}.Session`, 'RecordMonitor', '(sa{sv})',
            ['', captureDir ? {'cursor-mode': new GLib.Variant('u', 0)} : {}])[0];
        if (captureDir) {
            Gio.DBus.session.signal_subscribe(cast, `${cast}.Stream`, 'PipeWireStreamAdded', touchStream,
                null, Gio.DBusSignalFlags.NONE, (_connection, _sender, _path, _iface, _signal, params) => {
                    print(`Compositor capture stream: ${params.deep_unpack()[0]}`);
                    const {Gst, GstApp, GstVideo, GdkPixbuf} = imports.gi;
                    Gst.init(null);
                    capturePipeline = Gst.parse_launch(`pipewiresrc path=${params.deep_unpack()[0]} ! videoconvert ! video/x-raw,format=RGB ! appsink name=frames max-buffers=1 drop=true sync=false`);
                    captureSink = capturePipeline.get_by_name('frames');
                    const bus = capturePipeline.get_bus();
                    bus.add_signal_watch();
                    bus.connect('message::error', (_bus, message) => { throw Error(message.parse_error().join(': ')); });
                    capturePipeline.set_state(Gst.State.PLAYING);
                });
        }
    }
    send('Start', '()', []);
    send('NotifyPointerMotionRelative', '(dd)', [-10000, -10000]);
    if (nativeTest || workspaceHold || workspaceMotion || workspaceSwitcher || workspaceTransitions) {
        // Creating Mutter's virtual touchscreen announces a new seat capability.
        // Let GTK bind wl_touch before the first test contact is delivered.
        send('NotifyTouchDown', '(sudd)', [touchStream, 0, 0, 0]);
        send('NotifyTouchUp', '(u)', [0]);
    }
    if (nativeTest || columnStacks || workspaceSwitcher || columnDrops || workspaceTransitions || colorPanel) {
        // Announce the virtual keyboard before testing activation. Otherwise
        // the first key can arrive before GTK binds the new wl_keyboard.
        send('NotifyKeyboardKeysym', '(ub)', [0xffe1, true]);
        send('NotifyKeyboardKeysym', '(ub)', [0xffe1, false]);
    }
    if (nativeTest || workspaceClicks || workspaceCursor || workspaceDrawer || drawerStyle || tooltips || workspaceWindow || workspaceColumns || workspaceTabs || workspaceHold || workspaceMotion || workspaceMenus) {
        let step = 0, events = null, index = 0, previous = [0, 0], trace = [], resumeAt = 0, pen = 0;
        const interval = Number(GLib.getenv('LAYER_NATIVE_EVENT_MS') || (workspaceMotion ? 4 : 60));
        if (!Number.isInteger(interval) || interval < 1 || interval > 1000)
            throw Error('LAYER_NATIVE_EVENT_MS must be an integer from 1 to 1000');
        const tracing = GLib.getenv('LAYER_NATIVE_INPUT_TRACE') === '1';
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, interval, () => {
            if (GLib.get_monotonic_time() < resumeAt) return GLib.SOURCE_CONTINUE;
            if (Gio.File.new_for_path(`${output}/finished`).query_exists(null)) {
                if (capturePipeline) {
                    capturePipeline.set_state(imports.gi.Gst.State.NULL);
                    capturePipeline = null;
                }
                send('Stop', '()', []);
                return GLib.SOURCE_REMOVE;
            }
            if (!events) {
                const file = Gio.File.new_for_path(`${output}/step-${step}.json`);
                if (!file.query_exists(null)) return GLib.SOURCE_CONTINUE;
                const [, bytes] = file.load_contents(null);
                events = JSON.parse(new TextDecoder().decode(bytes));
                index = 0;
                trace = [];
            }
            if (index === events.length) {
                if (tracing) GLib.file_set_contents(`${output}/trace-${step}.json`, JSON.stringify(trace));
                GLib.file_set_contents(`${output}/done-${step++}`, 'done');
                events = null;
            } else {
                const event = events[index++];
                if (tracing) trace.push({ns: GLib.get_monotonic_time() * 1000, event});
                if (event.capture) {
                    if (!captureFrame(event.capture)) index--;
                    return GLib.SOURCE_CONTINUE;
                }
                if ('wait_ms' in event) {
                    if (!Number.isFinite(event.wait_ms) || event.wait_ms < 0 || event.wait_ms > 10000)
                        throw Error('Native event wait must be 0..10000 ms');
                    resumeAt = GLib.get_monotonic_time() + event.wait_ms * 1000;
                    return GLib.SOURCE_CONTINUE;
                }
                if (event.key) {
                    send('NotifyKeyboardKeysym', '(ub)', [event.key, event.down]);
                    return GLib.SOURCE_CONTINUE;
                }
                if (event.touch) {
                    if (event.touch === 'up') send('NotifyTouchUp', '(u)', [event.slot ?? 0]);
                    else send(event.touch === 'down' ? 'NotifyTouchDown' : 'NotifyTouchMotion',
                        '(sudd)', [touchStream, event.slot ?? 0, ...event.point.map(v => v * touchScale)]);
                    return GLib.SOURCE_CONTINUE;
                }
                if (event.pen) {
                    if (!tabletProxy) throw Error('Pen contacts require --tablet');
                    GLib.file_set_contents(`${output}/pen-${pen++}.json`, JSON.stringify(event));
                    return GLib.SOURCE_CONTINUE;
                }
                if (event.point) {
                    // Touch, tablet input and native popups can reposition the
                    // pointer. Correctness fixtures need actual desktop points,
                    // not deltas from a stale pre-popup mouse position.
                    if (nativeTest) send('NotifyPointerMotionAbsolute', '(sdd)',
                        [touchStream, ...event.point.map(v => v * touchScale)]);
                    else send('NotifyPointerMotionRelative', '(dd)',
                        [event.point[0] - previous[0], event.point[1] - previous[1]]);
                    previous = event.point;
                }
                if (event.wheel) send('NotifyPointerAxisDiscrete', '(ui)', event.wheel);
                if ('down' in event) send('NotifyPointerButton', '(ib)', [event.button ?? 272, event.down]);
            }
            return GLib.SOURCE_CONTINUE;
        });
        return GLib.SOURCE_REMOVE;
    }
    if (workspaceDrag) {
        const [, bytes] = ready.load_contents(null);
        const {start, points} = JSON.parse(new TextDecoder().decode(bytes));
        let previous = [0, 0], index = -2;
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, 60, () => {
            if (index === -2) {
                send('NotifyPointerMotionRelative', '(dd)', start);
                previous = start;
            } else if (index === -1) {
                send('NotifyPointerButton', '(ib)', [272, true]);
            } else if (index < points.length) {
                const point = points[index];
                send('NotifyPointerMotionRelative', '(dd)', [point[0] - previous[0], point[1] - previous[1]]);
                previous = point;
            } else {
                send('NotifyPointerButton', '(ib)', [272, false]);
                send('Stop', '()', []);
                GLib.file_set_contents(`${output}/finished`, 'finished');
                return GLib.SOURCE_REMOVE;
            }
            index++;
            return GLib.SOURCE_CONTINUE;
        });
        return GLib.SOURCE_REMOVE;
    }
    // Separate positioning from focus/press so Mutter cannot coalesce the warp.
    GLib.timeout_add(GLib.PRIORITY_DEFAULT, 100, () => {
        send('NotifyPointerMotionRelative', '(dd)', [800, 500]);
        return GLib.SOURCE_REMOVE;
    });
    GLib.timeout_add(GLib.PRIORITY_DEFAULT, 300, () => {
        send('NotifyPointerButton', '(ib)', [272, true]);
        send('NotifyPointerButton', '(ib)', [272, false]);
        return GLib.SOURCE_REMOVE;
    });
    GLib.timeout_add(GLib.PRIORITY_DEFAULT, 600, () => {
        send('NotifyPointerButton', '(ib)', [274, true]);
        const start = GLib.get_monotonic_time(), sent = [];
        let previous = [0, 0];
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, 4, () => {
            const now = GLib.get_monotonic_time(), t = (now - start) / 1e6;
            if (t >= 6) {
                send('NotifyPointerButton', '(ib)', [274, false]);
                send('Stop', '()', []);
                GLib.file_set_contents(`${output}/sent.json`, JSON.stringify({start_us: start, end_us: now, sent_us: sent}));
                print(`Injected ${sent.length} native pointer motions in ${t.toFixed(3)} seconds`);
                return GLib.SOURCE_REMOVE;
            }
            const point = [180 * Math.sin(t * 3), 100 * Math.sin(t * 4)];
            send('NotifyPointerMotionRelative', '(dd)', [point[0] - previous[0], point[1] - previous[1]]);
            sent.push(now);
            previous = point;
            return GLib.SOURCE_CONTINUE;
        });
        return GLib.SOURCE_REMOVE;
    });
    return GLib.SOURCE_REMOVE;
});
GLib.timeout_add(GLib.PRIORITY_DEFAULT, nativeTest || workspaceHold || workspaceMotion || drawerStyle || workspaceSwitcher ? 120000 : 60000, () => {
    process.force_exit();
    loop.quit();
    return GLib.SOURCE_REMOVE;
});
loop.run();
if (!passed) throw new Error('Native input benchmark failed');
