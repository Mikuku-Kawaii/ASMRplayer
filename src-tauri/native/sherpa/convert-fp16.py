"""Convert the pinned upstream SenseVoice ONNX; never fetch or execute model code."""
import json
import sys
from pathlib import Path

import onnx
import ort_float16 as float16


def convert(source: Path, target: Path):
    model = onnx.load(str(source), load_external_data=False)
    metadata = {entry.key: entry.value for entry in model.metadata_props}
    if metadata.get("model_type") != "sense_voice_ctc":
        raise ValueError("Unexpected SenseVoice model type")
    if any(t.external_data for t in model.graph.initializer):
        raise ValueError("Expected self-contained official model")
    original_inputs = [v.SerializeToString() for v in model.graph.input]
    original_outputs = [v.SerializeToString() for v in model.graph.output]
    # Keep the C++ recognizer's float32 feature/logit interface. Operators that
    # require full precision remain float32 through the converter's block list.
    model = float16.convert_float_to_float16(model, keep_io_types=True)
    # The official converter appends IO/block-list casts. Restore topological
    # order before validating and saving this fixed feed-forward model.
    known = {v.name for v in model.graph.input} | {v.name for v in model.graph.initializer} | {""}
    pending = list(model.graph.node)
    ordered = []
    while pending:
        remaining = []
        for node in pending:
            if set(node.input) <= known:
                ordered.append(node)
                known.update(node.output)
            else:
                remaining.append(node)
        if len(remaining) == len(pending):
            raise ValueError("Unresolved model dependencies")
        pending = remaining
    del model.graph.node[:]
    model.graph.node.extend(ordered)
    onnx.checker.check_model(model, full_check=True)
    if ([v.SerializeToString() for v in model.graph.input] != original_inputs
            or [v.SerializeToString() for v in model.graph.output] != original_outputs):
        raise ValueError("Recognition IO contract changed")
    count = sum(t.data_type == onnx.TensorProto.FLOAT16 for t in model.graph.initializer)
    if count == 0:
        raise ValueError("No FP16 weights produced")
    if {entry.key: entry.value for entry in model.metadata_props} != metadata:
        raise ValueError("Recognition metadata changed")
    onnx.save(model, str(target), save_as_external_data=False)
    print(json.dumps({"fp16Tensors": count, "bytes": target.stat().st_size}))


if __name__ == "__main__":
    convert(Path(sys.argv[1]), Path(sys.argv[2]))
