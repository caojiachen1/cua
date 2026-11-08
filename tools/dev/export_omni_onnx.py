"""Offline exporter for the OmniParser v2 assets used by cua's pure-Rust runtime.

This is a development/verification tool, never part of the shipped runtime. It
loads the pinned TorchScript detector and the official Florence-2 caption
weights with PyTorch, then writes ONNX graphs whose inputs/outputs are fixed
and whose forward semantics match the official V2 service defaults.

Outputs (under <model-dir>/onnx):
  yolo_raw_{640,1280}.onnx   input  images[1,3,S,S] -> raw[1,42000] flat
  fl_encoder.onnx            pixel[1,3,64,64] + ids[1,8] -> hidden[1,13,768]
  fl_decoder.onnx            ids[1,21] + hidden[1,13,768] -> logits[1,21,51289]
  fl_ids.json                the expanded "<CAPTION>" token ids
  golden_*.bin               deterministic tensors for the Rust conformance test
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import torch
from PIL import Image, ImageDraw
from transformers import AutoModelForCausalLM, AutoProcessor

ROOT = Path(__file__).resolve().parents[2]
MODEL = ROOT / ".models" / "omni-v2" / "f55d0750"
OUT = MODEL / "onnx"
OUT.mkdir(parents=True, exist_ok=True)


class YoloRaw(torch.nn.Module):
    def __init__(self, model: torch.jit.ScriptModule):
        super().__init__()
        self.model = model

    def forward(self, images: torch.Tensor) -> torch.Tensor:
        return torch.cat([y.reshape(1, -1) for y in self.model(images)], dim=1)


def export_yolo() -> None:
    import onnx
    from onnx import TensorProto, helper

    model = torch.jit.load(str(MODEL / "icon_detect_v3-model.pt"), map_location="cpu").eval()
    for size in (640, 1280):
        final = OUT / f"yolo_raw_{size}.onnx"
        if final.exists():
            continue
        stage = OUT / f"yolo6_{size}.onnx"
        dummy = torch.linspace(-1.0, 1.0, 3 * size * size).reshape(1, 3, size, size)
        with torch.no_grad():
            outputs = model(dummy)
        torch.onnx.export(
            model,
            dummy,
            str(stage),
            opset_version=17,
            input_names=["images"],
            output_names=[f"o{i}" for i in range(len(outputs))],
            dynamo=False,
        )
        model_onnx = onnx.load(str(stage))
        graph = model_onnx.graph
        merged_inputs = []
        for i in range(len(outputs)):
            graph.node.append(
                helper.make_node("Flatten", [f"o{i}"], [f"flat{i}"], axis=1)
            )
            merged_inputs.append(f"flat{i}")
        graph.node.append(helper.make_node("Concat", merged_inputs, ["raw"], axis=1))
        del graph.output[:]
        graph.output.append(
            helper.make_tensor_value_info("raw", TensorProto.FLOAT, [1, -1])
        )
        onnx.save(model_onnx, str(final))
        expected = torch.cat([y.reshape(1, -1) for y in outputs], dim=1)
        expected.numpy().astype("<f4").tofile(OUT / f"golden_yolo_{size}.bin")
        dummy.numpy().astype("<f4").tofile(OUT / f"golden_yolo_in_{size}.bin")
        print("yolo", size, tuple(expected.shape), final.stat().st_size)


class FlorenceEncoder(torch.nn.Module):
    def __init__(self, model, input_ids: torch.Tensor):
        super().__init__()
        self.model = model
        self.register_buffer("input_ids", input_ids)

    def forward(self, pixel_values: torch.Tensor) -> torch.Tensor:
        feats = self.model._encode_image(pixel_values)
        emb = self.model.get_input_embeddings()(self.input_ids)
        merged, mask = self.model._merge_input_ids_with_image_features(feats, emb)
        out = self.model.language_model.model.encoder(inputs_embeds=merged, attention_mask=mask)
        return out.last_hidden_state


class FlorenceDecoder(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.lm = model.language_model

    def forward(self, input_ids: torch.Tensor, encoder_hidden_states: torch.Tensor) -> torch.Tensor:
        return self.lm(
            decoder_input_ids=input_ids,
            encoder_outputs=(encoder_hidden_states,),
            use_cache=False,
            return_dict=True,
        ).logits


def export_florence() -> None:
    processor = AutoProcessor.from_pretrained(
        str(ROOT / ".models" / "florence2-caption"), trust_remote_code=True, local_files_only=True
    )
    model = AutoModelForCausalLM.from_pretrained(
        str(ROOT / ".models" / "florence2-caption"),
        trust_remote_code=True,
        local_files_only=True,
        torch_dtype=torch.float32,
        low_cpu_mem_usage=True,
    ).eval()

    image = Image.new("RGB", (64, 64), (244, 244, 244))
    draw = ImageDraw.Draw(image)
    draw.rectangle([14, 14, 50, 50], fill=(25, 90, 210))
    inputs = processor(images=[image], text=["<CAPTION>"], return_tensors="pt", do_resize=False)
    ids = inputs["input_ids"]
    pixel = inputs["pixel_values"]
    (OUT / "fl_ids.json").write_text(json.dumps(ids[0].tolist()), encoding="utf-8")

    enc = FlorenceEncoder(model, ids).eval()
    with torch.no_grad():
        hidden = enc(pixel)
    torch.onnx.export(
        enc,
        pixel,
        str(OUT / "fl_encoder.onnx"),
        opset_version=17,
        input_names=["pixel_values"],
        output_names=["hidden"],
        dynamo=False,
    )
    hidden.numpy().astype("<f4").tofile(OUT / "golden_fl_encoder.bin")
    pixel.numpy().astype("<f4").tofile(OUT / "golden_fl_pixel.bin")

    dec = FlorenceDecoder(model).eval()
    dec_ids = torch.full((1, 21), 1, dtype=torch.long)
    dec_ids[0, 0] = 2
    # Use a real generated prefix so the golden includes non-trivial tokens.
    with torch.no_grad():
        out = model.generate(
            input_ids=ids,
            pixel_values=pixel,
            max_new_tokens=20,
            num_beams=1,
            do_sample=False,
        )
    print("official tokens", out[0].tolist())
    prefix = out[0].tolist()[1:5]
    dec_ids[0, 1 : 1 + len(prefix)] = torch.tensor(prefix)
    with torch.no_grad():
        logits = dec(dec_ids, hidden)
    torch.onnx.export(
        dec,
        (dec_ids, hidden),
        str(OUT / "fl_decoder.onnx"),
        opset_version=17,
        input_names=["input_ids", "encoder_hidden_states"],
        output_names=["logits"],
        dynamo=False,
    )
    logits.numpy().astype("<f4").tofile(OUT / "golden_fl_logits.bin")
    dec_ids.numpy().astype("<i8").tofile(OUT / "golden_fl_ids.bin")

    import onnxruntime as ort

    for graph, feeds in (
        ("fl_encoder.onnx", {"pixel_values": pixel.numpy().astype(np.float32)}),
        (
            "fl_decoder.onnx",
            {
                "input_ids": dec_ids.numpy().astype(np.int64),
                "encoder_hidden_states": hidden.numpy().astype(np.float32),
            },
        ),
    ):
        sess = ort.InferenceSession(str(OUT / graph), providers=["CPUExecutionProvider"])
        got = sess.run(None, feeds)[0]
        ref = logits.numpy() if graph.startswith("fl_decoder") else hidden.numpy()
        print(graph, "max_abs", float(np.max(np.abs(got - ref))), "shape", got.shape)




class FlorenceEncoderBatch(torch.nn.Module):
    def __init__(self, model, input_ids: torch.Tensor):
        super().__init__()
        self.model = model
        self.register_buffer("input_ids", input_ids)

    def forward(self, pixel_values: torch.Tensor) -> torch.Tensor:
        feats = self.model._encode_image(pixel_values)
        emb = self.model.get_input_embeddings()(self.input_ids)
        merged, mask = self.model._merge_input_ids_with_image_features(feats, emb)
        out = self.model.language_model.model.encoder(inputs_embeds=merged, attention_mask=mask)
        return out.last_hidden_state


class FlorenceDecoderBatch(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.lm = model.language_model

    def forward(self, input_ids: torch.Tensor, encoder_hidden_states: torch.Tensor) -> torch.Tensor:
        return self.lm(
            decoder_input_ids=input_ids,
            encoder_outputs=(encoder_hidden_states,),
            use_cache=False,
            return_dict=True,
        ).logits


def export_florence_batch(batch_size: int = 8) -> None:
    import onnxruntime as ort

    processor = AutoProcessor.from_pretrained(
        str(ROOT / ".models" / "florence2-caption"), trust_remote_code=True, local_files_only=True
    )
    model = AutoModelForCausalLM.from_pretrained(
        str(ROOT / ".models" / "florence2-caption"),
        trust_remote_code=True,
        local_files_only=True,
        torch_dtype=torch.float32,
        low_cpu_mem_usage=True,
    ).eval()

    from PIL import Image, ImageDraw

    image = Image.new("RGB", (64, 64), (244, 244, 244))
    draw = ImageDraw.Draw(image)
    draw.rectangle([14, 14, 50, 50], fill=(25, 90, 210))
    inputs = processor(
        images=[image] * batch_size,
        text=["<CAPTION>"] * batch_size,
        return_tensors="pt",
        do_resize=False,
    )
    pixel = inputs["pixel_values"]
    ids = inputs["input_ids"]
    assert pixel.shape[0] == batch_size and ids.shape[0] == batch_size

    enc = FlorenceEncoderBatch(model, ids).eval()
    with torch.no_grad():
        hidden = enc(pixel)
    enc_path = OUT / f"fl_encoder_b{batch_size}.onnx"
    torch.onnx.export(
        enc,
        pixel,
        str(enc_path),
        opset_version=17,
        input_names=["pixel_values"],
        output_names=["hidden"],
        dynamo=False,
    )

    dec = FlorenceDecoderBatch(model).eval()
    dec_ids = torch.full((batch_size, 21), 1, dtype=torch.long)
    dec_ids[:, 0] = 2
    with torch.no_grad():
        logits = dec(dec_ids, hidden)
    dec_path = OUT / f"fl_decoder_b{batch_size}.onnx"
    torch.onnx.export(
        dec,
        (dec_ids, hidden),
        str(dec_path),
        opset_version=17,
        input_names=["input_ids", "encoder_hidden_states"],
        output_names=["logits"],
        dynamo=False,
    )

    hidden.numpy().astype("<f4").tofile(OUT / f"golden_fl_encoder_b{batch_size}.bin")
    logits.numpy().astype("<f4").tofile(OUT / f"golden_fl_logits_b{batch_size}.bin")
    pixel.numpy().astype("<f4").tofile(OUT / f"golden_fl_pixel_b{batch_size}.bin")
    dec_ids.numpy().astype("<i8").tofile(OUT / f"golden_fl_ids_b{batch_size}.bin")

    def ort_out(graph, feeds):
        sess = ort.InferenceSession(str(OUT / graph), providers=["CPUExecutionProvider"])
        return sess.run(None, feeds)[0]

    b_hidden = ort_out(f"fl_encoder_b{batch_size}.onnx", {"pixel_values": pixel.numpy().astype(np.float32)})
    b_logits = ort_out(
        f"fl_decoder_b{batch_size}.onnx",
        {
            "input_ids": dec_ids.numpy().astype(np.int64),
            "encoder_hidden_states": hidden.numpy().astype(np.float32),
        },
    )
    print(
        "batch", batch_size,
        "encoder max_abs", float(np.max(np.abs(b_hidden - hidden.numpy()))),
        "decoder max_abs", float(np.max(np.abs(b_logits - logits.numpy()))),
        "shapes", b_hidden.shape, b_logits.shape,
    )
    if (OUT / "fl_encoder.onnx").is_file() and (OUT / "fl_decoder.onnx").is_file():
        single_pixel = pixel[:1].numpy().astype(np.float32)
        single_hidden = ort_out("fl_encoder.onnx", {"pixel_values": single_pixel})
        single_logits = ort_out(
            "fl_decoder.onnx",
            {
                "input_ids": dec_ids[:1].numpy().astype(np.int64),
                "encoder_hidden_states": single_hidden.astype(np.float32),
            },
        )
        print(
            "batch row0 vs single: encoder max_abs",
            float(np.max(np.abs(b_hidden[0] - single_hidden[0]))),
            "decoder max_abs",
            float(np.max(np.abs(b_logits[0] - single_logits[0]))),
        )

if __name__ == "__main__":
    import sys

    which = sys.argv[1:] or ["yolo", "florence"]
    if "yolo" in which:
        export_yolo()
    if "florence" in which:
        export_florence()
    if "batch" in which:
        export_florence_batch()
