// SPDX-License-Identifier: MIT OR Apache-2.0
const {Gio, GLib} = imports.gi;
const destination = 'org.gnome.Mutter.ScreenCast';
const call = (path, iface, method, signature, values) => Gio.DBus.session.call_sync(
    destination, path, iface, method, new GLib.Variant(signature, values), null,
    Gio.DBusCallFlags.NONE, 5000, null,
).deep_unpack();

function cropWindowPixels(pixels, width, height, stride) {
    const rawAlpha = {transparent: 0, partial: 0, opaque: 0};
    let left = width, top = height, right = -1, bottom = -1;
    for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
            const i = y * stride + x * 4, alpha = pixels[i + 3];
            rawAlpha[alpha === 0 ? 'transparent' : alpha === 255 ? 'opaque' : 'partial']++;
            if (alpha) {
                left = Math.min(left, x);
                top = Math.min(top, y);
                right = Math.max(right, x);
                bottom = Math.max(bottom, y);
            }
        }
    }
    if (!rawAlpha.transparent || !rawAlpha.partial || !rawAlpha.opaque)
        throw Error('Window capture requires transparent corners, native shadow and opaque content');
    if (left < 2 || top < 2 || right >= width - 2 || bottom >= height - 2)
        throw Error('Window capture clips its native shadow');
    const crop = {x: left - 2, y: top - 2, width: right - left + 5, height: bottom - top + 5};
    const cropped = new Uint8Array(crop.width * crop.height * 4);
    const alpha = {transparent: 0, partial: 0, opaque: 0};
    for (let y = 0; y < crop.height; y++) {
        for (let x = 0; x < crop.width; x++) {
            const source = (y + crop.y) * stride + (x + crop.x) * 4;
            const target = (y * crop.width + x) * 4, a = pixels[source + 3];
            alpha[a === 0 ? 'transparent' : a === 255 ? 'opaque' : 'partial']++;
            cropped[target + 3] = a;
            for (let c = 0; c < 3; c++)
                cropped[target + c] = a ? Math.min(255, Math.round(pixels[source + c] * 255 / a)) : 0;
        }
    }
    const corners = [0, crop.width - 1, (crop.height - 1) * crop.width, crop.height * crop.width - 1]
        .map(pixel => cropped[pixel * 4 + 3]);
    if (corners.some(a => a !== 0)) throw Error('Window capture corners are not transparent');
    return {pixels: cropped, metadata: {raw_size: [width, height], crop,
        dimensions: [crop.width, crop.height], raw_alpha: rawAlpha, alpha: {...alpha, corners}}};
}

var WindowCapture = class {
    constructor(directory) {
        this.directory = directory;
        this.error = null;
        this.active = null;
    }

    capture(name) {
        if (!this.directory || typeof name !== 'string' || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name))
            throw Error('Invalid native window capture request');
        try {
            if (!this.active) this.begin(name);
            if (this.active.name !== name) throw Error('Another window capture is pending');
            if (this.error) throw this.error;
            if (GLib.get_monotonic_time() >= this.active.deadline)
                throw Error(`Compositor window capture timed out: ${name}`);
            const sample = this.active.sink?.try_pull_sample(0);
            if (!sample) return false;
            this.save(sample, name);
            this.close();
            if (this.error) throw this.error;
            return true;
        } catch (error) {
            this.error = error;
            this.close();
            throw error;
        }
    }

    begin(name) {
        const {Gst, GstApp} = imports.gi;
        Gst.init(null);
        this.active = {name, deadline: GLib.get_monotonic_time() + 15000000};
        this.active.session = call('/org/gnome/Mutter/ScreenCast', destination,
            'CreateSession', '(a{sv})', [{}])[0];
        const stream = call(this.active.session, `${destination}.Session`, 'RecordWindow', '(a{sv})',
            [{'cursor-mode': new GLib.Variant('u', 0)}])[0];
        this.active.signal = Gio.DBus.session.signal_subscribe(destination, `${destination}.Stream`,
            'PipeWireStreamAdded', stream, null, Gio.DBusSignalFlags.NONE,
            (_connection, _sender, _path, _iface, _signal, parameters) => {
                try {
                    this.active.pipeline = Gst.parse_launch(
                        `pipewiresrc path=${parameters.deep_unpack()[0]} ! video/x-raw,format=BGRA ! videoconvert ! video/x-raw,format=RGBA ! appsink name=frames max-buffers=1 drop=true sync=false`);
                    this.active.sink = this.active.pipeline.get_by_name('frames');
                    this.active.bus = this.active.pipeline.get_bus();
                    this.active.bus.add_signal_watch();
                    this.active.busSignal = this.active.bus.connect('message::error', (_bus, message) => {
                        this.error = Error(message.parse_error().join(': '));
                    });
                    if (this.active.pipeline.set_state(Gst.State.PLAYING) === Gst.StateChangeReturn.FAILURE)
                        throw Error('Cannot start compositor window capture');
                } catch (error) {
                    this.error = error;
                }
            });
        call(this.active.session, `${destination}.Session`, 'Start', '()', []);
    }

    save(sample, name) {
        const {Gst, GstVideo, GdkPixbuf} = imports.gi;
        const info = GstVideo.VideoInfo.new_from_caps(sample.get_caps());
        if (sample.get_caps().get_structure(0).get_string('format') !== 'RGBA')
            throw Error('Window capture did not negotiate RGBA');
        const buffer = sample.get_buffer();
        const [ok, map] = buffer.map(Gst.MapFlags.READ);
        if (!ok) throw Error('Cannot map compositor window capture');
        let result;
        try {
            result = cropWindowPixels(map.data, info.width, info.height, info.stride[0]);
        } finally {
            buffer.unmap(map);
        }
        const [width, height] = result.metadata.dimensions;
        GLib.mkdir_with_parents(this.directory, 0o700);
        GdkPixbuf.Pixbuf.new_from_bytes(new GLib.Bytes(result.pixels), GdkPixbuf.Colorspace.RGB,
            true, 8, width, height, width * 4).savev(`${this.directory}/${name}.png`, 'png', [], []);
        GLib.file_set_contents(`${this.directory}/${name}.json`, `${JSON.stringify(result.metadata, null, 2)}\n`);
    }

    close() {
        const active = this.active;
        this.active = null;
        if (!active) return;
        if (active.signal) Gio.DBus.session.signal_unsubscribe(active.signal);
        if (active.busSignal) active.bus.disconnect(active.busSignal);
        if (active.bus) active.bus.remove_signal_watch();
        if (active.pipeline) active.pipeline.set_state(imports.gi.Gst.State.NULL);
        if (active.session) {
            try {
                call(active.session, `${destination}.Session`, 'Stop', '()', []);
            } catch (error) {
                this.error ??= error;
            }
        }
    }
};
