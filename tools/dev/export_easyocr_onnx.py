"""Offline export of EasyOCR's English reference models for cua's Rust/ORT
backend. Development tool only; the runtime uses ONNX Runtime, not PyTorch.

Models: craft_mlt_25k.pth (CRAFT detector) and english_g2.pth (VGG+BiLSTM CTC).
"""
from __future__ import annotations

import json
import os
from pathlib import Path

import numpy as np
import torch
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
MODEL = ROOT / ".models" / "easyocr-en"
OUT = MODEL
OUT.mkdir(parents=True, exist_ok=True)
EASY = Path(os.path.expanduser("~/.EasyOCR/model"))


def strip_module(sd):
    return {k[7:] if k.startswith("module.") else k: v for k, v in sd.items()}


class CraftWrap(torch.nn.Module):
    def __init__(self, net):
        super().__init__()
        self.net = net

    def forward(self, images):
        y, _ = self.net(images)
        return y.permute(0, 3, 1, 2)


class RecWrap(torch.nn.Module):
    def __init__(self, net):
        super().__init__()
        self.net = net

    def forward(self, images):
        return self.net(images, None)


def export() -> None:
    from easyocr.craft import CRAFT
    from easyocr.model.vgg_model import Model

    craft = CRAFT()
    craft.load_state_dict(strip_module(torch.load(EASY / "craft_mlt_25k.pth", map_location="cpu", weights_only=False)))
    craft.eval()
    craft_wrap = CraftWrap(craft).eval()
    dummy = torch.rand(1, 3, 640, 640)
    torch.onnx.export(
        craft_wrap,
        dummy,
        str(OUT / "craft_mlt_25k.onnx"),
        opset_version=17,
        input_names=["images"],
        output_names=["scores"],
        dynamic_axes={"images": {2: "height", 3: "width"}, "scores": {2: "height", 3: "width"}},
        dynamo=False,
    )

    import easyocr

    reader = easyocr.Reader(["en"], gpu=False, verbose=False)
    chars = list(reader.converter.character)
    (OUT / "characters.json").write_text(json.dumps(chars, ensure_ascii=False), encoding="utf-8")
    num_class = len(chars)
    rec = Model(input_channel=1, output_channel=256, hidden_size=256, num_class=num_class)
    rec.load_state_dict(strip_module(torch.load(EASY / "english_g2.pth", map_location="cpu", weights_only=False)))
    rec.eval()
    rec_wrap = RecWrap(rec).eval()
    dummy_rec = torch.zeros(1, 1, 64, 400)
    torch.onnx.export(
        rec_wrap,
        dummy_rec,
        str(OUT / "english_g2.onnx"),
        opset_version=17,
        input_names=["images"],
        output_names=["logits"],
        dynamo=False,
    )

    # A deterministic UI-like crop for the Rust conformance test.
    image = Image.new("RGB", (800, 400), (250, 250, 250))
    draw = ImageDraw.Draw(image)
    from PIL import ImageFont

    font = ImageFont.truetype(r"C:\Windows\Fonts\arial.ttf", 28)
    draw.text((40, 40), "OCR TEST 123", font=font, fill=(0, 0, 0))
    draw.text((40, 100), "Settings", font=font, fill=(0, 0, 0))
    image.save(OUT / "easyocr-test.png")

    # PyTorch/ONNX Runtime consistency on the CRAFT input.
    import onnxruntime as ort

    arr = np.asarray(image.resize((800, 400)), dtype=np.float32)
    mean = np.array([0.485, 0.456, 0.406], dtype=np.float32) * 255.0
    std = np.array([0.229, 0.224, 0.225], dtype=np.float32) * 255.0
    arr = (arr - mean) / std
    x = arr.transpose(2, 0, 1)[None].astype(np.float32)
    with torch.no_grad():
        ref = craft_wrap(torch.from_numpy(x)).numpy()
    sess = ort.InferenceSession(str(OUT / "craft_mlt_25k.onnx"), providers=["CPUExecutionProvider"])
    got = sess.run(None, {"images": x})[0]
    print("craft max_abs", float(np.max(np.abs(got - ref))), "shape", got.shape)

    # Recognition conformance on one crop.
    crop = image.crop((40, 40, 260, 80)).convert("L").resize((220, 64), Image.BICUBIC)
    rec_arr = (np.asarray(crop, dtype=np.float32) / 255.0 - 0.5) / 0.5
    rec_in = np.zeros((1, 1, 64, 400), dtype=np.float32)
    rec_in[0, 0, :, :220] = rec_arr
    with torch.no_grad():
        ref_rec = rec_wrap(torch.from_numpy(rec_in)).numpy()
    sess = ort.InferenceSession(str(OUT / "english_g2.onnx"), providers=["CPUExecutionProvider"])
    got_rec = sess.run(None, {"images": rec_in})[0]
    print("rec max_abs", float(np.max(np.abs(got_rec - ref_rec))), "shape", got_rec.shape)
    print("chars", len(chars), chars[:5], chars[-5:])
    print("exported", list(OUT.glob("*.onnx")), list(OUT.glob("*.json")))


if __name__ == "__main__":
    export()
