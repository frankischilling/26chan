#!/usr/bin/env python3
"""Extract frozen, independently measured source counters, never cost estimates.

Default verifies checked-in TSVs. --record deliberately replaces them. No Rust
module or ledger estimator is imported or executed, and no bound is calculated.
All input hashes are checked before JSON decoding, even with python -O.
"""

import hashlib
import json
from pathlib import Path
import re
import sys

HERE = Path(__file__).resolve().parent
PINS = {
    "shapes": "dbc102099b5e6315e87939739997a9657b5be66057bda656db076d485705d123",
    "segments": "e3ac631fb0700c962683a6eeca5e7e102ba7945fea7674fdb4da3e9aac772035",
    "state": "f34b673385bb9b5e4d7c30ff17c7a749af836d9c297dfe109d7db747c69f3fee",
    "state-cost": "3dd282cf2afaf0961389ffa8fdc211d55713330ef0c6c8315b6aa3758a9b814f",
    "ledger": "5fc240231dd799de7302117afbd521ee907231115403499841a2135b9f1a54b7",
}
SOURCE_SHA = "daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744"
TOOLS = {"pencil": 1, "pen": 2, "airbrush": 3, "tone": 5, "blur": 7, "eraser": 8}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def load(name):
    raw = (HERE / "source" / (name + ".json")).read_bytes()
    require(hashlib.sha256(raw).hexdigest() == PINS[name], "input pin mismatch: " + name)
    result = json.loads(raw)
    require(result["pin"] == {"bytes": 110619, "sha256": SOURCE_SHA, "version": "0.9.4"}, "source pin")
    return result


def lines(rows):
    return "".join("\t".join(str(value) for value in row) + "\n" for row in rows).encode()


def source_events(code):
    if code == "for(let i=0;i<50;i++)new TegakiEventHistoryDummy(0).dispatch();":
        return ["HistoryDummy"] * 50
    pattern = r"new TegakiEvent(\w+)\(([^()]*)\)\.dispatch\(\);"
    require(not re.sub(pattern, "", code).strip(), "unrecognized source event code: " + code)
    result = []
    for name, arguments in re.findall(pattern, code):
        args = arguments.split(",")
        require(args.pop(0) == "0", "expected synthetic event time")
        result.append(",".join([name] + args))
    return result


def source_totals(rows, flatten=False):
    """Sum raw counters/API dimensions. No estimator or envelope formulas."""
    result = {key: 0 for key in [
        "typed_backing_bytes", "typed_copy_bytes", "typed_clear_bytes",
        "shape_loop_units", "shape_flood_scalar_pushes", "brush_leaf_units", "brush_loop_units",
        "tone_map_cells", "tone_loop_units", "canvas_surface_pixels", "preview_surface_pixels",
        "canvas_get_calls", "canvas_get_pixels", "canvas_fill_calls", "canvas_fill_pixels",
        "canvas_put_calls", "canvas_put_input_pixels", "canvas_put_dirty_pixels",
        "preview_draw_calls", "preview_source_pixels", "preview_destination_pixels",
        "preview_clear_calls", "preview_clear_pixels", "flatten_draw_calls",
        "flatten_source_pixels", "flatten_destination_pixels",
    ]}
    for row in rows:
        result["typed_backing_bytes"] += row["typedBytes"]
        c = row["counts"]
        result["typed_copy_bytes"] += c.get("typed.copyBytes", 0)
        result["typed_clear_bytes"] += c.get("typed.fillBytes", 0)
        result["shape_loop_units"] += sum(v for k, v in c.items() if k.startswith("shape."))
        result["shape_flood_scalar_pushes"] += c.get("shape.stack.scalarPushes", 0)
        result["brush_leaf_units"] += sum(c.get(k, 0) for k in [
            "segment.points", "brush.cells", "blur.neighbor.cells", "blur.copy.cells"
        ])
        result["brush_loop_units"] += sum(v for k, v in c.items() if k == "segment.points" or re.fullmatch(
            r"(brush\.(rows|cells)|blur\.(neighbor|copy)\.(columns|cells))(\.tests)?", k))
        result["tone_map_cells"] += c.get("tone.cells", 0)
        result["tone_loop_units"] += sum(v for k, v in c.items() if k.startswith("tone."))
        for canvas in row.get("createdCanvases", []):
            name = canvas["id"]
            if name == "tegaki-cursor-layer":
                continue  # Explicitly outside replay-area accounting.
            key = "preview_surface_pixels" if name and name.startswith("tegaki-layers-p-canvas-") else "canvas_surface_pixels"
            result[key] += canvas["requestedPixels"]
        for call in row["canvas"]:
            name, args = call["method"], call["args"]
            if name == "getImageData":
                result["canvas_get_calls"] += 1
                result["canvas_get_pixels"] += args[2] * args[3]
            elif name == "fillRect":
                result["canvas_fill_calls"] += 1
                result["canvas_fill_pixels"] += args[2] * args[3]
            elif name == "putImageData":
                result["canvas_put_calls"] += 1
                result["canvas_put_input_pixels"] += args[0] * args[1]
                result["canvas_put_dirty_pixels"] += args[6] * args[7] if len(args) == 8 else args[0] * args[1]
            elif name == "drawImage":
                prefix = "flatten" if flatten else "preview"
                result[prefix + "_draw_calls"] += 1
                result[prefix + "_source_pixels"] += args[0] * args[1]
                result[prefix + "_destination_pixels"] += args[4] * args[5] if len(args) == 6 else args[0] * args[1]
            elif name == "clearRect":
                if call["canvasWidth"] == 128 and call["canvasHeight"] == 128:
                    continue  # Fixture cursor, not a layer preview.
                result["preview_clear_calls"] += 1
                result["preview_clear_pixels"] += args[2] * args[3]
            else:
                raise ValueError("unrecognized source canvas call: " + name)
    return result


def extract():
    source = {name: load(name) for name in PINS}
    output = {}
    output["shapes.tsv"] = lines([
        [TOOLS[c["tool"]], c["tip"] or 0, c["size"], c["B"], c["typedBytes"], c["work"], c["stackScalarPushes"]]
        for c in source["shapes"]["cases"]
    ])
    output["segments.tsv"] = lines([
        [TOOLS[c["tool"]], c["tip"] or 0, c["size"], *c["from"], *c["to"], c["B"], c["I"], c["leafWork"], c["allLoopWork"]]
        for c in source["segments"]["cases"]
    ])
    tools = source["state-cost"]["scenarios"][0]["toolMap"]
    output["tools.tsv"] = lines([
        [key, value["size"], value["alpha"], value["flow"], value["step"], value["tipId"], int(value["usePreserveAlpha"])]
        for key, value in tools.items()
    ])
    ledger = source["ledger"]
    events = []
    names = {"setTool": "SetTool", "setAlpha": "SetToolAlpha", "setSize": "SetToolSize", "setTip": "SetToolTip", "commit": "DrawCommit"}
    for event in ledger["plan"]["events"]:
        kind = event["type"]
        if kind in ["start", "draw"]:
            name = "DrawStart" if kind == "start" else "Draw"
            args = [event["x"], event["y"]]
            if "pressure" in event:
                args.append(event["pressure"])
            else:
                name += "NoP"
        else:
            name = names[kind]
            args = [event["value"]] if "value" in event else []
        events.append(",".join(str(v) for v in [name] + args))
    output["ledger-events.txt"] = (";".join(events) + "\n").encode()
    output["ledger-counters.tsv"] = lines(ledger["measuredSourceCounters"].items())
    scenario_rows, counter_rows = [], []
    scenarios = source["state-cost"]["scenarios"]
    require(sum(len(s["rows"]) for s in scenarios) == 47, "state-cost row count")
    for scenario in scenarios:
        name = scenario["name"]
        flatten = name == "optional-final-flatten-request-volume"
        if flatten:
            # These unmeasured setup events are explicit in pinned state-cost.cjs.
            events = ["AddLayer", "AddLayer", "ToggleLayerVisibility,2"]
            rows = scenario["rows"][1:]
        else:
            events = [event for row in scenario["rows"][1:] for event in source_events(row["code"])]
            rows = scenario["rows"]
        scenario_rows.append([name, int(flatten), ";".join(events)])
        counter_rows.extend([name, key, value] for key, value in source_totals(rows, flatten).items())
    output["state-scenarios.tsv"] = lines(scenario_rows)
    output["state-counters.tsv"] = lines(counter_rows)
    # Preserve exact source tone-cell and selected-loop observations independently.
    output["tone.tsv"] = lines([
        [row["label"], row["counts"].get("tone.cells", 0), sum(v for k, v in row["counts"].items() if k.startswith("tone.")), row["typedBytes"]]
        for row in source["state"]["cases"]
    ])
    return output


if __name__ == "__main__":
    require(sys.argv[1:] in [[], ["--record"]], "usage: generate.py [--record]")
    output = extract()
    for name, content in output.items():
        path = HERE / name
        if sys.argv[1:]:
            path.write_bytes(content)
        else:
            require(path.read_bytes() == content, "frozen output differs: " + name)
    print("verified " + str(len(output)) + " frozen source-counter files")
