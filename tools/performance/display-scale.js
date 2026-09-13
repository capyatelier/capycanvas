// Configure only workspace-motion.sh's private Mutter display. GDK_SCALE alone
// does not establish the monitor scale on a Wayland compositor.
const {Gio, GLib} = imports.gi;
const scale = Number(ARGV[0]);
if (!Number.isFinite(scale) || scale < 1 || scale > 4) throw Error('Expected display scale 1..4');
const dest = 'org.gnome.Mutter.DisplayConfig';
const path = '/org/gnome/Mutter/DisplayConfig';
const call = (method, signature, values) => Gio.DBus.session.call_sync(
    dest, path, dest, method, signature ? new GLib.Variant(signature, values) : null,
    null, Gio.DBusCallFlags.NONE, 5000, null,
).deep_unpack();
const [serial, monitors] = call('GetCurrentState');
if (monitors.length !== 1) throw Error('Expected one private virtual monitor');
const [spec, modes] = monitors[0];
const mode = modes.find(m => m[6]['is-current']?.deep_unpack()) || modes[0];
if (!mode[5].some(s => Math.abs(s - scale) < .001)) throw Error(`Unsupported monitor scale ${scale}`);
call('ApplyMonitorsConfig', '(uua(iiduba(ssa{sv}))a{sv})', [
    serial, 1, [[0, 0, scale, 0, true, [[spec[0], mode[0], {}]]]], {},
]);
const logical = call('GetCurrentState')[2];
if (logical.length !== 1 || Math.abs(logical[0][2] - scale) > .001) throw Error('Monitor scale did not apply');
print(`Private Wayland monitor scale: ${scale}`);
