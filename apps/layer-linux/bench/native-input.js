// SPDX-License-Identifier: MIT OR Apache-2.0
// Test-only virtual pointer through Mutter -> Wayland -> GTK/Chromium. Run ONLY inside
// an isolated dbus-run-session + headless Mutter, never on a user's desktop.
const {Gio, GLib} = imports.gi;
if (!GLib.getenv('WAYLAND_DISPLAY')?.startsWith('layer-bench-'))
    throw new Error('Requires an isolated layer-bench-* Wayland display');
const output = GLib.getenv('LAYER_NATIVE_INPUT_DIR');
if (!output) throw new Error('Set LAYER_NATIVE_INPUT_DIR to an empty temporary directory');
const ready = Gio.File.new_for_path(`${output}/ready`);
if (ready.query_exists(null)) throw new Error('Use a fresh output directory');
const loop = new GLib.MainLoop(null, false);
const workspaceDrag = ARGV.includes('--workspace-drag');
const workspaceClicks = ARGV.includes('--workspace-clicks');
const workspaceCursor = ARGV.includes('--workspace-cursor');
const workspaceDrawer = ARGV.includes('--workspace-drawer');
const workspaceWindow = ARGV.includes('--workspace-window');
const workspaceColumns = ARGV.includes('--workspace-columns');
const workspaceTabs = ARGV.includes('--workspace-tabs');
const workspaceHold = ARGV.includes('--workspace-hold');
const workspaceMenus = ARGV.includes('--workspace-menus');
const workspaceWeb = ARGV.includes('--web-workspace-motion');
const workspaceMotion = ARGV.includes('--workspace-motion') || workspaceWeb;
const process = Gio.Subprocess.new([
    ...(workspaceWeb ? ['node', 'apps/layer-web/test.mjs', '--workspace-motion', '--native-input'] : [
        'cargo', 'test', '--release', '-p', 'layer-linux', workspaceMenus ? 'native_workspace_menu_input' : workspaceMotion ? 'native_workspace_motion_input' : workspaceHold ? 'native_long_press_drag_input' : workspaceTabs ? 'native_tab_slide_input' : workspaceColumns ? 'native_collapsed_column_input' : workspaceWindow ? 'native_window_drag_input' : workspaceDrawer ? 'native_column_drawer_drag_input' : workspaceCursor ? 'native_divider_cursor_input' : workspaceClicks ? 'native_floating_click_input' : workspaceDrag ? 'native_toolbar_drag_input' : 'native_compositor_input',
        '--', '--ignored', '--test-threads=1', '--nocapture',
    ]),
], Gio.SubprocessFlags.NONE);
let passed = false;
process.wait_async(null, (p, result) => {
    p.wait_finish(result);
    passed = p.get_successful();
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
    if (workspaceHold || workspaceMotion) {
        const id = call(session, 'org.freedesktop.DBus.Properties', 'Get', '(ss)', [iface, 'SessionId'])[0].deep_unpack();
        const cast = 'org.gnome.Mutter.ScreenCast';
        const castCall = (path, name, method, signature, values) => Gio.DBus.session.call_sync(
            cast, path, name, method, new GLib.Variant(signature, values), null,
            Gio.DBusCallFlags.NONE, 3000, null,
        ).deep_unpack();
        const path = castCall('/org/gnome/Mutter/ScreenCast', cast, 'CreateSession', '(a{sv})',
            [{'remote-desktop-session-id': new GLib.Variant('s', id)}])[0];
        touchStream = castCall(path, `${cast}.Session`, 'RecordMonitor', '(sa{sv})', ['', {}])[0];
    }
    send('Start', '()', []);
    send('NotifyPointerMotionRelative', '(dd)', [-10000, -10000]);
    if (workspaceHold || workspaceMotion) {
        // Creating Mutter's virtual touchscreen announces a new seat capability.
        // Let GTK bind wl_touch before the first test contact is delivered.
        send('NotifyTouchDown', '(sudd)', [touchStream, 0, 0, 0]);
        send('NotifyTouchUp', '(u)', [0]);
    }
    if (workspaceClicks || workspaceCursor || workspaceDrawer || workspaceWindow || workspaceColumns || workspaceTabs || workspaceHold || workspaceMotion || workspaceMenus) {
        let step = 0, events = null, index = 0, previous = [0, 0];
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, workspaceMotion ? 4 : 60, () => {
            if (Gio.File.new_for_path(`${output}/finished`).query_exists(null)) {
                send('Stop', '()', []);
                return GLib.SOURCE_REMOVE;
            }
            if (!events) {
                const file = Gio.File.new_for_path(`${output}/step-${step}.json`);
                if (!file.query_exists(null)) return GLib.SOURCE_CONTINUE;
                const [, bytes] = file.load_contents(null);
                events = JSON.parse(new TextDecoder().decode(bytes));
                index = 0;
            }
            if (index === events.length) {
                GLib.file_set_contents(`${output}/done-${step++}`, 'done');
                events = null;
            } else {
                const event = events[index++];
                if (event.touch) {
                    if (event.touch === 'up') send('NotifyTouchUp', '(u)', [0]);
                    else send(event.touch === 'down' ? 'NotifyTouchDown' : 'NotifyTouchMotion',
                        '(sudd)', [touchStream, 0, ...event.point]);
                    return GLib.SOURCE_CONTINUE;
                }
                if (event.point) {
                    send('NotifyPointerMotionRelative', '(dd)',
                        [event.point[0] - previous[0], event.point[1] - previous[1]]);
                    previous = event.point;
                }
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
GLib.timeout_add(GLib.PRIORITY_DEFAULT, workspaceHold || workspaceMotion ? 120000 : 60000, () => {
    process.force_exit();
    loop.quit();
    return GLib.SOURCE_REMOVE;
});
loop.run();
if (!passed) throw new Error('Native input benchmark failed');
