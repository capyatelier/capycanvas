import json, sys, os
import numpy as np
B = 16
def load(d, i, fr):
    f = fr[i]
    w, h, s = f['width'], f['height'], f['stride']
    b = np.frombuffer(open(f"{d}/{i:04d}.rgb", "rb").read(), np.uint8)[:h * s].reshape(h, s)[:, :w * 3].reshape(h, w, 3)
    return b


def seg_mask(shape, paths, radius, origin=(0, 0)):
    h, w = shape
    oy, ox = origin[1], origin[0]
    yy, xx = np.mgrid[oy:oy+h, ox:ox+w]
    m = np.zeros((h, w), bool)
    for path in paths:
        for (x0, y0), (x1, y1) in zip(path[:-1], path[1:]):
            dx, dy = x1 - x0, y1 - y0
            l2 = dx * dx + dy * dy or 1e-9
            lo_x, hi_x = int(max(ox, min(x0, x1) - radius - 2)) - ox, int(min(ox+w, max(x0, x1) + radius + 2)) - ox
            lo_y, hi_y = int(max(oy, min(y0, y1) - radius - 2)) - oy, int(min(oy+h, max(y0, y1) + radius + 2)) - oy
            if lo_x >= hi_x or lo_y >= hi_y:
                continue
            sx, sy = xx[lo_y:hi_y, lo_x:hi_x], yy[lo_y:hi_y, lo_x:hi_x]
            t = np.clip(((sx - x0) * dx + (sy - y0) * dy) / l2, 0, 1)
            d2 = (sx - x0 - t * dx) ** 2 + (sy - y0 - t * dy) ** 2
            m[lo_y:hi_y, lo_x:hi_x] |= d2 <= radius * radius
    return m



LAT_MS = 60
PIX_T = 48


def main(cap, rundir, only=None):
    summary = {}
    for name in sorted(os.listdir(cap)):
        d = f"{cap}/{name}"
        if not os.path.exists(f"{d}/meta.json") or (only and name != only):
            continue
        meta = json.load(open(f"{d}/meta.json"))
        fr = json.load(open(f"{d}/frames.json"))
        trace = json.load(open(f"{rundir}/trace-{meta['input_step']}.json"))
        strokes, cur = [], None
        for e in trace:
            p = e['event'].get('pen')
            if p == 'down':
                cur = [(e['ns'], e['event']['point'])]
            elif p == 'move' and cur is not None:
                cur.append((e['ns'], e['event']['point']))
            elif p == 'up' and cur is not None:
                strokes.append((cur, e['ns']))
                cur = None
        assert meta["committed_after"] - meta["committed_before"] == 4
        assert len(strokes) == 4
        r = meta['stroke_rect']
        x0, y0, x1, y1 = int(np.ceil(r[0])) + 2, int(np.ceil(r[1])) + 2, int(r[2]) - 2, int(r[3]) - 2
        shape = (y1 - y0, x1 - x0)
        rad = meta['brush_size'] * meta['zoom'] / meta['scale_factor'] / 2
        n = len(fr)
        crop = lambda i: load(d, i, fr)[y0:y1, x0:x1].astype(np.int16)
        final = crop(n - 1)
        pre_i = max([i for i, f in enumerate(fr) if f['pulled_ns'] <= meta['changed_ns']] or [0])
        if meta['name'].endswith('quickzoom'):
            pre_i = min(i for i, f in enumerate(fr) if f['pulled_ns'] >= strokes[0][0][0][0]) - 1
        pre = crop(pre_i)
        inked = np.abs(final - pre).max(axis=2) > PIX_T
        print(f"== {name}: frames {n} pre#{pre_i} zoom {meta['zoom']:.3f} brush radius {rad:.1f}px")
        worst = []
        full_shape, full_final, full_pre, full_inked = shape, final, pre, inked
        for k, (pts, up) in enumerate(strokes):
            assert all(a[0] <= b[0] for a, b in zip(pts, pts[1:])), "ordered delivered pen samples"
            local = [(x-x0, y-y0) for _, (x, y) in pts]
            padding = rad*0.6+2
            h, w = full_shape
            left = max(0, int(np.floor((min(x for x,y in local)-padding)/B))*B)
            top = max(0, int(np.floor((min(y for x,y in local)-padding)/B))*B)
            right = min(w//B*B, int(np.ceil((max(x for x,y in local)+padding)/B))*B)
            bottom = min(h//B*B, int(np.ceil((max(y for x,y in local)+padding)/B))*B)
            assert left < right and top < bottom, "stroke intersects the recorded block grid"
            shape = (bottom-top, right-left)
            final, pre, inked = (image[top:bottom,left:right] for image in (full_final,full_pre,full_inked))
            crop = lambda i: load(d,i,fr)[y0+top:y0+bottom,x0+left:x0+right].astype(np.int16)
            masks = {}
            def prefix(count):
                if count not in masks:
                    core = seg_mask(shape,[local[:count]],rad*0.6,(left,top)) & inked
                    h,w = shape
                    blocks = core.reshape(h//B,B,w//B,B).sum(axis=(1,3))
                    masks[count] = (core,blocks)
                return masks[count]
            first_seen = {}
            due_at = {}
            frames_bad = []
            for i, f in enumerate(fr):
                t = f['pulled_ns']
                if t < pts[0][0] or t > up + 400e6:
                    continue
                due = [(x - x0, y - y0) for ns, (x, y) in pts if ns <= t - LAT_MS * 1e6]
                if len(due) < 2:
                    continue
                core, cb = prefix(len(due))
                img = crop(i)
                is_pre = np.abs(img - pre).max(axis=2) <= PIX_T // 2
                is_fin = np.abs(img - final).max(axis=2) <= PIX_T
                stale = core & is_pre & ~is_fin
                h, w = shape
                sb = stale[:h // B * B, :w // B * B].reshape(h // B, B, w // B, B).sum(axis=(1, 3))
                bad = (cb >= 24) & (sb > cb * 0.5)
                ok = (cb >= 24) & (sb <= cb * 0.1)
                for yx in zip(*np.nonzero(cb >= 24)):
                    due_at.setdefault(yx, t)
                for yx in zip(*np.nonzero(ok)):
                    first_seen.setdefault(yx, t)
                if bad.any():
                    ys, xs = np.nonzero(bad)
                    frames_bad.append((i, round((t - up) / 1e6), int(bad.sum()), int((cb >= 24).sum()), [int(xs.min()) * B + x0 + left, int(ys.min()) * B + y0 + top, int(xs.max() + 1) * B + x0 + left, int(ys.max() + 1) * B + y0 + top]))
            fullcore, fb = prefix(len(pts))
            h, w = shape
            state = {}
            reverts = []
            for i, f in enumerate(fr):
                t = f['pulled_ns']
                if t < pts[0][0] or t > up + 400e6:
                    continue
                img = crop(i)
                fin = (np.abs(img - final).max(axis=2) <= PIX_T) & fullcore
                pr = (np.abs(img - pre).max(axis=2) <= PIX_T // 2) & ~(np.abs(img - final).max(axis=2) <= PIX_T) & fullcore
                fs = fin[:h // B * B, :w // B * B].reshape(h // B, B, w // B, B).sum(axis=(1, 3))
                ps = pr[:h // B * B, :w // B * B].reshape(h // B, B, w // B, B).sum(axis=(1, 3))
                for yx in zip(*np.nonzero(fb >= 24)):
                    st = 'F' if fs[yx] >= fb[yx] * 0.9 else 'P' if ps[yx] >= fb[yx] * 0.5 else None
                    if st == 'P' and state.get(yx) == 'F':
                        reverts.append((i, round((t - up) / 1e6), int(yx[1]) * B + x0 + left, int(yx[0]) * B + y0 + top))
                    if st:
                        state[yx] = st
            print(f"  stroke {k}: F->P reverts {len(reverts)} {reverts[:4]}")
            ages = sorted(((first_seen.get(b, None) or float('inf')) - due_at[b]) / 1e6 for b in due_at)
            assert ages, f"{name}/{k}: no due ink blocks were observed"
            print(f"  stroke {k}: frames with stale due blocks {len(frames_bad)}; due-block reveal delay ms p50 {np.percentile(ages,50):.0f} p90 {np.percentile(ages,90):.0f} max {max(ages):.0f} (n={len(ages)})")
            for e in frames_bad[:3] + (frames_bad[-2:] if len(frames_bad) > 3 else []):
                print("     ", e)
            worst.append((len(frames_bad), max(ages)))
        summary[name] = worst
    return summary


if __name__ == '__main__':
    result=main(sys.argv[1],sys.argv[2])
    assert len(result)==20, f"expected pattern/photo, light/dark, five scenarios: {len(result)}"
    failures={name:rows for name,rows in result.items()if any(frames for frames,age in rows)}
    with open(os.path.join(sys.argv[1],"wrong-tiles-oracle.json"),"w")as output:
        json.dump({"latency_ms":LAT_MS,"pixel_threshold":PIX_T,"block_size":B,"results":result,"failures":failures},output,indent=2)
    assert not failures, f"stale stroke blocks beyond {LAT_MS}ms: {failures}"
