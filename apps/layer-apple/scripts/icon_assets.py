"""Keep fixed SVG paints and palette-tinted paints in their original draw order.

Single-mode SVGs remain one vector asset, including group opacity. Mixed icons
split at root children and ordinary fill-before-stroke paints. Reject complex
group mixing instead of silently changing overlap/compositing semantics.
"""
import copy
import xml.etree.ElementTree as ET

ET.register_namespace("", "http://www.w3.org/2000/svg")


def paint_modes(element, inherited=None):
    tag = element.tag.rsplit("}", 1)[-1]
    if tag not in {"svg", "g", "path", "circle", "rect", "ellipse", "line", "polyline", "polygon"}:
        raise ValueError(f"Unsupported SVG element {tag}; preserve its paint semantics before adding it")
    if any(key in element.attrib for key in ("style", "color", "filter", "mask", "clip-path", "paint-order")):
        raise ValueError("SVG styles/effects require explicit paint-order support")
    paint = dict(inherited or {"fill": "#000", "stroke": "none"})
    paint.update({key: value for key, value in element.attrib.items() if key in paint})
    if tag in {"svg", "g"}:
        return set().union(*(paint_modes(child, paint) for child in element))
    return {"template" if value == "currentColor" else "original" for value in paint.values() if value != "none"}


def icon_layers(svg):
    root = ET.fromstring(svg)
    modes = paint_modes(root)
    if len(modes) <= 1:
        mode = next(iter(modes), "template")
        return [(mode, svg.replace("currentColor", "#ffffff"))]
    if root.get("opacity", "1") != "1":
        raise ValueError("Mixed SVG root opacity requires group compositing")
    inherited = {"fill": root.get("fill", "#000"), "stroke": root.get("stroke", "none")}
    runs = []
    for child in root:
        modes = paint_modes(child, inherited)
        if len(modes) > 1:
            if child.tag.rsplit("}", 1)[-1] == "g" or child.get("opacity", "1") != "1":
                raise ValueError("Mixed SVG subtree opacity/group requires group compositing")
            fill, stroke = copy.deepcopy(child), copy.deepcopy(child)
            fill.set("stroke", "none")
            stroke.set("fill", "none")
            children = [fill, stroke]
        else:
            children = [child]
        for paint in children:
            modes = paint_modes(paint, inherited)
            if not modes:
                continue
            mode = next(iter(modes))
            if not runs or runs[-1][0] != mode:
                layer = copy.deepcopy(root)
                layer[:] = []
                runs.append((mode, layer))
            runs[-1][1].append(copy.deepcopy(paint))
    return [(mode, ET.tostring(layer, encoding="unicode").replace("currentColor", "#ffffff"))
            for mode, layer in runs]
