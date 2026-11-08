"""Dev-only exporter for a KV-cache ("incremental") Florence-2 caption decoder.

The shipped graph from ``export_omni_onnx.py`` is ``fl_decoder.onnx``: it runs the
decoder with ``use_cache=False`` and a frozen ``input_ids[1,21]`` shape, so the
runtime re-feeds all 21 prefix tokens on every decoding step.  This tool asks the
same weights (and the same remote ``modeling_florence2.py``) for the standard
``past_key_values``/``use_cache=True`` interface and tries to freeze that path
into ONNX.

Stages (all optional, ``all`` runs them in order)::

    python tools/dev/export_florence_kv.py check    # is the cache path usable in torch?
    python tools/dev/export_florence_kv.py export   # write onnx + golden tensors
    python tools/dev/export_florence_kv.py verify   # numeric equivalence vs fl_decoder.onnx
    python tools/dev/export_florence_kv.py bench    # per-step CPU latency

Outputs (``.models/omni-v2/f55d0750/onnx``)::

    fl_decoder_kv.onnx            one-token decoder with stacked K/V cache
    fl_decoder_kv.onnx.data       its weights (the torch.export exporter externalises them)
    golden_fl_kv_prefix.bin       the 21-token decoder prefix (int64 [1,21])
    golden_fl_kv_cache0.bin       K/V cache of its first 20 tokens ([layers,1,12,S,64] x4)
    golden_fl_kv_ref.bin          reference last-position logits for the 5 steps

Two variants are attempted and the first that survives an onnxruntime smoke
test is kept:

``native``  the stock ``language_model(...)`` call, exported with the
            ``torch.export``/dynamo exporter (needs ``pip install onnxscript``).
            This is the variant currently on disk.
``manual``  the same math with the decoder step spelled out and ``position_ids``
            as an input, so the legacy TorchScript exporter can still keep the
            cache length dynamic (no extra dependency).

The legacy exporter alone cannot express the native path: ``Florence2Decoder``
derives the position offset from ``past_key_values[0][0].shape[2]`` and the
causal mask from the same integer, so the traced graph bakes ``past == 21`` into
the positional embeddings; the dynamo exporter keeps it symbolic.

Environment note: the Florence-2 remote code was written for transformers 4.x,
while this machine runs transformers 5.14.  Loading it therefore needs four
in-memory shims (no file on disk is modified):

* ``timm`` is not installed but ``modeling_florence2`` imports ``timm.layers``
  (only the DaViT vision tower uses it and this tool never runs that tower),
* ``Florence2LanguageConfig`` reads the attribute ``forced_bos_token_id`` which
  transformers 5 removed from ``GenerationConfig``,
* transformers 5 expects dict-shaped ``_tied_weights_keys`` and a ``post_init``
  that re-initialises/re-ties weights; we load ``model.safetensors`` ourselves and
  re-create the 4.x tying (``lm_head`` -> ``model.shared``), verified against
  ``golden_fl_logits.bin`` from the shipped export (1.1e-5),
* ``AutoProcessor``/``CLIPImageProcessor`` no longer inherit ``do_normalize`` /
  ``image_mean`` / ``image_std`` from ``preprocessor_config.json``, so the
  processor is assembled by hand and the missing kwargs are passed explicitly
  (ids and pixels then match the shipped golden tensors exactly).
"""

from __future__ import annotations

import argparse
import importlib.machinery as machinery
import importlib.util
import sys
import time
import types
from pathlib import Path

import numpy as np
import torch

ROOT = Path(__file__).resolve().parents[2]
CAPTION_DIR = ROOT / ".models" / "florence2-caption"
OMNI_DIR = ROOT / ".models" / "omni-v2" / "f55d0750"
ONNX_DIR = OMNI_DIR / "onnx"

PREFIX_LEN = 21
STEPS = 5
ENC_LEN = 13
LAYERS = 6
HEADS = 12
HEAD_DIM = 64

KV_ONNX = ONNX_DIR / "fl_decoder_kv.onnx"
GOLD_PREFIX = ONNX_DIR / "golden_fl_kv_prefix.bin"
GOLD_CACHE0 = ONNX_DIR / "golden_fl_kv_cache0.bin"
GOLD_REF = ONNX_DIR / "golden_fl_kv_ref.bin"


# --------------------------------------------------------------------------- #
# environment shims
# --------------------------------------------------------------------------- #
def shim_environment() -> dict[str, types.ModuleType]:
    """Make the transformers-4.x remote code importable under transformers 5.x."""

    try:  # pragma: no cover - exercised only on machines without timm
        import timm  # noqa: F401
    except ImportError:
        timm = types.ModuleType("timm")
        timm.__spec__ = machinery.ModuleSpec("timm", None, is_package=True)
        timm.__version__ = "0.0.0-stub"
        layers = types.ModuleType("timm.layers")
        layers.__spec__ = machinery.ModuleSpec("timm.layers", None)
        layers.__version__ = "0.0.0-stub"

        class DropPath(torch.nn.Module):
            def __init__(self, drop_prob: float = 0.0):
                super().__init__()
                self.drop_prob = drop_prob

            def forward(self, x):
                return x

        def trunc_normal_(tensor, mean=0.0, std=1.0, a=-2.0, b=2.0):
            return torch.nn.init.trunc_normal_(tensor, mean, std, a, b)

        layers.DropPath = DropPath
        layers.trunc_normal_ = trunc_normal_
        timm.layers = layers
        sys.modules["timm"] = timm
        sys.modules["timm.layers"] = layers

    from transformers.modeling_utils import PreTrainedModel

    # 5.x wants dict-shaped ties and runs init_weights/tie_weights during
    # post_init(); we load the safetensors file ourselves, so both are no-ops.
    PreTrainedModel.get_expanded_tied_weights_keys = lambda self, *a, **k: []
    PreTrainedModel.tie_weights = lambda self, *a, **k: None
    PreTrainedModel.init_weights = lambda self, *a, **k: None

    pkg = types.ModuleType("flremote")
    pkg.__path__ = [str(CAPTION_DIR)]
    pkg.__spec__ = machinery.ModuleSpec("flremote", None, is_package=True)
    sys.modules["flremote"] = pkg
    mods = {}
    for name in ("configuration_florence2", "modeling_florence2", "processing_florence2"):
        spec = importlib.util.spec_from_file_location(
            f"flremote.{name}", CAPTION_DIR / f"{name}.py"
        )
        module = importlib.util.module_from_spec(spec)
        sys.modules[f"flremote.{name}"] = module
        spec.loader.exec_module(module)
        mods[name] = module

    cfg_mod = mods["configuration_florence2"]
    if not hasattr(cfg_mod.Florence2LanguageConfig, "forced_bos_token_id"):
        cfg_mod.Florence2LanguageConfig.forced_bos_token_id = None

    for cls_name in ("Florence2PreTrainedModel", "Florence2LanguagePreTrainedModel"):
        cls = getattr(mods["modeling_florence2"], cls_name)
        cls._supports_sdpa = True
        cls._supports_flash_attn_2 = False
        cls._supports_flash_attn = False
    return mods


def load_processor(mods: dict[str, types.ModuleType]):
    from transformers import BartTokenizerFast, CLIPImageProcessor

    image_processor = CLIPImageProcessor.from_pretrained(str(CAPTION_DIR))
    tokenizer = BartTokenizerFast(tokenizer_file=str(CAPTION_DIR / "tokenizer.json"))
    # transformers 5 dropped this attribute, the remote processor still reads it.
    tokenizer.additional_special_tokens = []
    return mods["processing_florence2"].Florence2Processor(
        image_processor=image_processor, tokenizer=tokenizer
    )


def load_model(mods: dict[str, types.ModuleType]):
    from safetensors.torch import load_file

    cfg_mod, model_mod = mods["configuration_florence2"], mods["modeling_florence2"]
    config = cfg_mod.Florence2Config.from_pretrained(str(CAPTION_DIR))
    config._attn_implementation = "sdpa"
    config.text_config._attn_implementation = "sdpa"
    model = model_mod.Florence2ForConditionalGeneration(config).eval()

    state = load_file(str(CAPTION_DIR / "model.safetensors"))
    missing, unexpected = model.load_state_dict(state, strict=False)
    lm = model.language_model
    # transformers 4.x tied all three to `model.shared` (verified against the
    # shipped golden logits: 1.1e-5 with the tie, 8.7 without).
    shared = lm.model.shared.weight
    lm.model.encoder.embed_tokens.weight = shared
    lm.model.decoder.embed_tokens.weight = shared
    lm.lm_head.weight = shared
    untied = [k for k in missing if "embed_tokens" not in k]
    if untied or unexpected:
        raise RuntimeError(f"unexpected weight mismatch: {untied} {unexpected}")
    return model


def build_inputs(processor):
    """Same call the original exporter makes, plus the 5.x kwargs it now needs."""
    from PIL import Image, ImageDraw

    image = Image.new("RGB", (64, 64), (244, 244, 244))
    draw = ImageDraw.Draw(image)
    draw.rectangle([14, 14, 50, 50], fill=(25, 90, 210))
    inputs = processor(
        images=[image],
        text=["<CAPTION>"],
        return_tensors="pt",
        do_resize=False,
        do_normalize=True,
        image_mean=[0.485, 0.456, 0.406],
        image_std=[0.229, 0.224, 0.225],
    )
    return inputs["input_ids"], inputs["pixel_values"]


def encode_hidden(pixel: torch.Tensor) -> np.ndarray:
    """Reuse the already validated encoder graph (input is byte-identical)."""
    import onnxruntime as ort

    sess = ort.InferenceSession(
        str(ONNX_DIR / "fl_encoder.onnx"), providers=["CPUExecutionProvider"]
    )
    hidden = sess.run(None, {"pixel_values": pixel.numpy().astype(np.float32)})[0]
    golden = np.fromfile(ONNX_DIR / "golden_fl_encoder.bin", dtype="<f4").reshape(hidden.shape)
    diff = float(np.abs(hidden - golden).max())
    if diff > 1e-3:
        raise RuntimeError(f"fl_encoder.onnx disagrees with golden: {diff}")
    return hidden


# --------------------------------------------------------------------------- #
# decoder wrappers
# --------------------------------------------------------------------------- #
class StackedKVDecoder(torch.nn.Module):
    """One-token decoder whose cache is exposed as four stacked tensors.

    ``past_*``/``present_*`` are ``[L, B, H, S, D]`` (layer-major), which keeps the
    ONNX signature down to four cache tensors instead of 24.
    """

    def __init__(self, model):
        super().__init__()
        self.lm = model.language_model
        self.layers = len(self.lm.model.decoder.layers)

    def split(self, pk, pv, pck, pcv):
        return [(pk[i], pv[i], pck[i], pcv[i]) for i in range(self.layers)]

    def stack(self, cache):
        k = torch.stack([layer[0] for layer in cache])
        v = torch.stack([layer[1] for layer in cache])
        ck = torch.stack([layer[2] for layer in cache])
        cv = torch.stack([layer[3] for layer in cache])
        return k, v, ck, cv


class NativeKVDecoder(StackedKVDecoder):
    """Stock path: whatever ``language_model(...)`` does with past_key_values."""

    def forward(self, input_ids, encoder_hidden_states, past_k, past_v, past_cross_k, past_cross_v):
        out = self.lm(
            decoder_input_ids=input_ids,
            encoder_outputs=(encoder_hidden_states,),
            past_key_values=self.split(past_k, past_v, past_cross_k, past_cross_v),
            use_cache=True,
            return_dict=True,
        )
        return (out.logits, *self.stack(out.past_key_values))


class ManualKVDecoder(StackedKVDecoder):
    """Same math with the decoder step spelled out.

    Needed when the stock path only survives *tracing*: ``Florence2Decoder`` derives
    the position offset from ``past_key_values[0][0].shape[2]`` and the causal mask
    from the same integer, so a traced graph bakes the trace-time past length into
    the position embeddings.  Feeding ``position_ids`` explicitly keeps the graph
    valid for any past length.  ``attention_mask``/``encoder_attention_mask`` are
    left at ``None`` on purpose: with a single query token the causal mask is
    unused (``Florence2SdpaAttention`` sets ``is_causal=False`` when ``tgt_len``
    is 1) and the 13 encoder positions are unpadded, so both masks are all-ones.
    """

    def __init__(self, model):
        super().__init__(model)
        self.decoder = self.lm.model.decoder
        self.lm_head = self.lm.lm_head
        self.logits_bias = self.lm.final_logits_bias

    def forward(
        self,
        input_ids,
        encoder_hidden_states,
        position_ids,
        past_k,
        past_v,
        past_cross_k,
        past_cross_v,
    ):
        pos = self.decoder.embed_positions
        hidden = self.decoder.embed_tokens(input_ids)
        hidden = hidden + torch.nn.functional.embedding(position_ids + pos.offset, pos.weight)
        hidden = self.decoder.layernorm_embedding(hidden)

        present = []
        for idx, layer in enumerate(self.decoder.layers):
            outputs = layer(
                hidden,
                attention_mask=None,
                encoder_hidden_states=encoder_hidden_states,
                encoder_attention_mask=None,
                layer_head_mask=None,
                cross_attn_layer_head_mask=None,
                past_key_value=(past_k[idx], past_v[idx], past_cross_k[idx], past_cross_v[idx]),
                output_attentions=False,
                use_cache=True,
            )
            hidden = outputs[0]
            present.append(outputs[-1])

        logits = self.lm_head(hidden) + self.logits_bias
        return (logits, *self.stack(present))


# --------------------------------------------------------------------------- #
# helpers
# --------------------------------------------------------------------------- #
def ort_session(path: Path):
    import onnxruntime as ort

    return ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])


def as_np(t) -> np.ndarray:
    return t.numpy().astype(np.float32) if torch.is_tensor(t) else np.asarray(t, dtype=np.float32)


def graph_feeds(sess, hidden: np.ndarray, token: torch.Tensor, position: int, cache) -> dict:
    """Build feeds from what the graph actually declares.

    The legacy tracer drops inputs that reach no output: with the cross-attention
    K/V taken from the cache (``Florence2Attention`` reuse branch)
    ``encoder_hidden_states`` is pruned from the manual variant, while the dynamo
    variant keeps it.
    """
    table = {
        "input_ids": token.numpy().astype(np.int64),
        "encoder_hidden_states": np.asarray(hidden, dtype=np.float32),
        "position_ids": np.array([[position]], dtype=np.int64),
        "past_self_key": as_np(cache[0]),
        "past_self_value": as_np(cache[1]),
        "past_cross_key": as_np(cache[2]),
        "past_cross_value": as_np(cache[3]),
    }
    return {i.name: table[i.name] for i in sess.get_inputs()}


def stack_cache(cache):
    """``tuple[tuple[tensor]]`` -> four ``[L,B,H,S,D]`` tensors (torch)."""
    k = torch.stack([layer[0] for layer in cache]).detach()
    v = torch.stack([layer[1] for layer in cache]).detach()
    ck = torch.stack([layer[2] for layer in cache]).detach()
    cv = torch.stack([layer[3] for layer in cache]).detach()
    return (k, v, ck, cv)


def torch_reference(model, hidden, tokens):
    """Full-sequence (use_cache=False) logits, the ground truth for a prefix."""
    lm = model.language_model
    with torch.no_grad():
        out = lm(
            decoder_input_ids=tokens,
            encoder_outputs=(hidden,),
            use_cache=False,
            return_dict=True,
        )
    return out.logits


def describe(path: Path) -> str:
    import onnx

    model = onnx.load(str(path), load_external_data=False)
    opset = ",".join(f"{o.domain or 'ai.onnx'}:{o.version}" for o in model.opset_import)
    inputs = ", ".join(
        f"{i.name}{[d.dim_value or d.dim_param for d in i.type.tensor_type.shape.dim]}"
        for i in model.graph.input
    )
    outputs = ", ".join(
        f"{o.name}{[d.dim_value or d.dim_param for d in o.type.tensor_type.shape.dim]}"
        for o in model.graph.output
    )
    size = path.stat().st_size / 1e6
    external = path.with_suffix(path.suffix + ".data")
    extra = f" + {external.stat().st_size / 1e6:.0f}MB external data" if external.is_file() else ""
    return f"{path.name}  opset[{opset}]  {size:.0f}MB{extra}\n  in : {inputs}\n  out: {outputs}"


# --------------------------------------------------------------------------- #
# stage: check
# --------------------------------------------------------------------------- #
def stage_check() -> None:
    mods = shim_environment()
    model = load_model(mods)
    processor = load_processor(mods)
    ids, pixel = build_inputs(processor)
    print("processor ids", ids[0].tolist(), "pixel", tuple(pixel.shape),
          "pixel_vs_golden", float(np.abs(
              pixel.numpy() - np.fromfile(ONNX_DIR / "golden_fl_pixel.bin", dtype="<f4").reshape(1, 3, 64, 64)
          ).max()))

    hidden_np = encode_hidden(pixel)
    hidden = torch.from_numpy(hidden_np)
    prefix = torch.from_numpy(
        np.fromfile(ONNX_DIR / "golden_fl_ids.bin", dtype="<i8").reshape(1, PREFIX_LEN)
    )
    print("prefix", prefix.shape, "hidden", tuple(hidden.shape))

    lm = model.language_model
    full = torch_reference(model, hidden, prefix)
    golden = np.fromfile(ONNX_DIR / "golden_fl_logits.bin", dtype="<f4").reshape(full.shape)
    print("torch full-sequence vs shipped golden logits:", float(np.abs(full.numpy() - golden).max()))

    with torch.no_grad():
        out = lm(
            decoder_input_ids=prefix,
            encoder_outputs=(hidden,),
            use_cache=True,
            return_dict=True,
        )
    cache = out.past_key_values
    print("use_cache=True ->", type(cache).__name__, "layers", len(cache),
          "per-layer", [tuple(t.shape) for t in cache[0]])
    print("last-position logits with/without cache:",
          float((out.logits[0, -1] - full[0, -1]).abs().max()))

    # one cached step must match a full recompute of the longer prefix
    nxt = prefix[:, -1:]  # re-run the last token to exercise the cache path
    with torch.no_grad():
        step = lm(
            decoder_input_ids=nxt,
            encoder_outputs=(hidden,),
            past_key_values=tuple(
                (layer[0][:, :, :-1], layer[1][:, :, :-1], layer[2], layer[3]) for layer in cache
            ),
            use_cache=True,
            return_dict=True,
        )
    print("cached step vs full recompute:", float((step.logits[0, -1] - full[0, -1]).abs().max()))


# --------------------------------------------------------------------------- #
# stage: export
# --------------------------------------------------------------------------- #
def reference_logits(model, hidden, prefix):
    """Step 1 logits come from the shipped ONNX graph.

    The shipped ``fl_decoder.onnx`` only accepts exactly 21 ids, so the longer
    prefixes of steps 2..5 are scored by a full ``use_cache=False`` torch
    recompute; at 21 ids that recompute agrees with the ONNX graph to 8.6e-6
    (printed by the ``check`` stage), which is what makes it a valid stand-in.
    """
    sess = ort_session(ONNX_DIR / "fl_decoder.onnx")
    shipped = sess.run(
        None,
        {
            "input_ids": prefix.numpy().astype(np.int64),
            "encoder_hidden_states": hidden.numpy().astype(np.float32),
        },
    )[0][0, -1]
    refs = [shipped]
    tokens = prefix.clone()
    greedy = []
    for _ in range(STEPS):
        tok = int(np.argmax(refs[-1]))
        greedy.append(tok)
        tokens = torch.cat([tokens, torch.tensor([[tok]], dtype=torch.long)], dim=1)
        refs.append(torch_reference(model, hidden, tokens)[0, -1].numpy())
        if len(refs) == STEPS:
            break
    print("greedy tokens", greedy)
    return np.stack(refs[:STEPS]), tokens


def export_graph(model, hidden, prefix, kind: str, path: Path) -> bool:
    """Export one variant and smoke-test it at three prefix lengths."""
    if kind == "native":
        module = NativeKVDecoder(model).eval()
        export_kwargs = dict(dynamo=True)
    else:
        module = ManualKVDecoder(model).eval()
        export_kwargs = dict(dynamo=False)

    with torch.no_grad():
        cache = model.language_model(
            decoder_input_ids=prefix[:, : PREFIX_LEN - 1],
            encoder_outputs=(hidden,),
            use_cache=True,
            return_dict=True,
        ).past_key_values
    stacked = tuple(t.clone() for t in stack_cache(cache))
    extra = () if kind == "native" else (torch.tensor([[PREFIX_LEN - 1]], dtype=torch.long),)
    args = (prefix[:, -1:], hidden, *extra, *stacked)

    try:
        if kind == "native":
            from torch.export import Dim

            dynamic_shapes = {
                "input_ids": {},
                "encoder_hidden_states": {},
                "past_k": {3: Dim("past")},
                "past_v": {3: Dim("past")},
                "past_cross_k": {},
                "past_cross_v": {},
            }
            torch.onnx.export(
                module,
                args,
                str(path),
                input_names=[
                    "input_ids", "encoder_hidden_states", "past_self_key", "past_self_value",
                    "past_cross_key", "past_cross_value",
                ],
                output_names=[
                    "logits", "present_self_key", "present_self_value",
                    "present_cross_key", "present_cross_value",
                ],
                dynamic_shapes=dynamic_shapes,
                **export_kwargs,
            )
        else:
            # the manual step needs no shape-derived constant, so the legacy
            # exporter can keep the past length dynamic
            torch.onnx.export(
                module,
                args,
                str(path),
                input_names=[
                    "input_ids", "encoder_hidden_states", "position_ids", "past_self_key",
                    "past_self_value", "past_cross_key", "past_cross_value",
                ],
                output_names=[
                    "logits", "present_self_key", "present_self_value",
                    "present_cross_key", "present_cross_value",
                ],
                dynamic_axes={
                    "past_self_key": {3: "past"},
                    "past_self_value": {3: "past"},
                },
                opset_version=17,
                **export_kwargs,
            )
    except Exception as exc:  # noqa: BLE001 - we want the full reason in the report
        import traceback

        print(f"[{kind}] export FAILED: {type(exc).__name__}: {exc}")
        if isinstance(exc, ModuleNotFoundError) and "onnxscript" in str(exc):
            print("  -> the dynamo/torch.export ONNX exporter needs `pip install onnxscript`")
        traceback.print_exc()
        return False

    print(f"[{kind}] exported {path.name} ({path.stat().st_size / 1e6:.0f} MB)")
    try:
        sess = ort_session(path)
        errs = []
        for cut in (PREFIX_LEN - 2, PREFIX_LEN - 1, PREFIX_LEN):
            tokens = prefix[:, :cut]
            position = tokens.shape[1] - 1
            with torch.no_grad():
                cache = model.language_model(
                    decoder_input_ids=tokens[:, :-1],
                    encoder_outputs=(hidden,),
                    use_cache=True,
                    return_dict=True,
                ).past_key_values
            feeds = graph_feeds(
                sess, hidden.numpy(), tokens[:, -1:], position, stack_cache(cache)
            )
            got = sess.run(None, feeds)[0][0, -1]
            ref = torch_reference(model, hidden, tokens)[0, -1].numpy()
            errs.append(float(np.abs(got - ref).max()))
        print(f"[{kind}] ort smoke (past 18/19/20) max|dlogit| = {errs}")
        ok = all(e < 1e-3 for e in errs)
        print(f"[{kind}] -> {'OK' if ok else 'MISMATCH'}")
        return ok
    except Exception as exc:  # noqa: BLE001
        print(f"[{kind}] ort smoke FAILED: {type(exc).__name__}: {exc}")
        return False


def stage_export(force: bool = False) -> None:
    if KV_ONNX.is_file() and not force:
        print("fl_decoder_kv.onnx already exists, nothing written (use `export --force`):")
        print(describe(KV_ONNX))
        return
    mods = shim_environment()
    model = load_model(mods)
    processor = load_processor(mods)
    _, pixel = build_inputs(processor)
    hidden = torch.from_numpy(encode_hidden(pixel))
    prefix = torch.from_numpy(
        np.fromfile(ONNX_DIR / "golden_fl_ids.bin", dtype="<i8").reshape(1, PREFIX_LEN)
    )
    refs, _ = reference_logits(model, hidden, prefix)
    refs.astype("<f4").tofile(GOLD_REF)
    prefix.numpy().astype("<i8").tofile(GOLD_PREFIX)

    with torch.no_grad():
        cache = model.language_model(
            decoder_input_ids=prefix[:, : PREFIX_LEN - 1],
            encoder_outputs=(hidden,),
            use_cache=True,
            return_dict=True,
        ).past_key_values
    np.concatenate([t.numpy().astype("<f4").ravel() for t in stack_cache(cache)]).tofile(GOLD_CACHE0)

    for kind in ("native", "manual"):
        print(f"--- trying {kind} past_key_values export ---")
        if export_graph(model, hidden, prefix, kind, KV_ONNX):
            print(f"--- kept {kind} variant ---")
            break
    else:
        print("!!! no variant exported")

    if KV_ONNX.is_file():
        print(describe(KV_ONNX))
    print("reference logits", refs.shape, "->", GOLD_REF.name)


# --------------------------------------------------------------------------- #
# stage: verify
# --------------------------------------------------------------------------- #
def load_golden_inputs():
    prefix = torch.from_numpy(np.fromfile(GOLD_PREFIX, dtype="<i8").reshape(1, PREFIX_LEN))
    flat = np.fromfile(GOLD_CACHE0, dtype="<f4")
    self_shape = (LAYERS, 1, HEADS, PREFIX_LEN - 1, HEAD_DIM)
    cross_shape = (LAYERS, 1, HEADS, ENC_LEN, HEAD_DIM)
    sizes = [int(np.prod(s)) for s in (self_shape, self_shape, cross_shape, cross_shape)]
    parts, offset = [], 0
    for size, shape in zip(sizes, (self_shape, self_shape, cross_shape, cross_shape)):
        parts.append(torch.from_numpy(flat[offset : offset + size].reshape(shape)))
        offset += size
    if offset != flat.size:
        raise RuntimeError(f"cache golden has {flat.size} floats, expected {offset}")
    refs = np.fromfile(GOLD_REF, dtype="<f4").reshape(STEPS, -1)
    return prefix, parts, refs


def stage_verify() -> None:
    mods = shim_environment()
    model = load_model(mods)
    processor = load_processor(mods)
    _, pixel = build_inputs(processor)
    hidden = encode_hidden(pixel)

    prefix, cache, refs = load_golden_inputs()
    sess = ort_session(KV_ONNX)
    print("kv graph inputs:", [i.name for i in sess.get_inputs()])
    print("kv graph outputs:", [o.name for o in sess.get_outputs()])

    current = prefix.clone()
    past = tuple(t.numpy() for t in cache)
    worst = 0.0
    for step in range(STEPS):
        feeds = graph_feeds(sess, hidden, current[:, -1:], current.shape[1] - 1, past)
        got = sess.run(None, feeds)
        logits = got[0][0, -1]
        past = tuple(got[1 + i] for i in range(4))
        err = float(np.abs(logits - refs[step]).max())
        worst = max(worst, err)
        top = int(np.argmax(logits))
        ref_top = int(np.argmax(refs[step]))
        print(f"step {step + 1}: feed_pos={current.shape[1] - 1} past_len={past[0].shape[3]} "
              f"token(kv)={top} token(ref)={ref_top} max|dlogit|={err:.3e}")
        current = torch.cat([current, torch.tensor([[top]], dtype=torch.long)], dim=1)
    print(f"worst max|dlogit| over {STEPS} steps: {worst:.3e} "
          f"({'PASS' if worst < 1e-3 else 'FAIL'} vs 1e-3)")


# --------------------------------------------------------------------------- #
# stage: bench
# --------------------------------------------------------------------------- #
def stage_bench() -> None:
    mods = shim_environment()
    model = load_model(mods)
    processor = load_processor(mods)
    _, pixel = build_inputs(processor)
    hidden = encode_hidden(pixel)
    prefix, cache, refs = load_golden_inputs()

    full_sess = ort_session(ONNX_DIR / "fl_decoder.onnx")
    kv_sess = ort_session(KV_ONNX)
    print("kv inputs:", [i.name for i in kv_sess.get_inputs()])

    full_feeds = {"input_ids": prefix.numpy().astype(np.int64), "encoder_hidden_states": hidden}
    for _ in range(3):
        full_sess.run(None, full_feeds)
    kv_sess.run(None, graph_feeds(kv_sess, hidden, prefix[:, -1:], PREFIX_LEN - 1,
                                  tuple(t.numpy() for t in cache)))

    # REPEATS passes over the 5 steps; the box is noisy, so report medians.
    REPEATS = 5
    full_ms = [[] for _ in range(STEPS)]
    kv_ms = [[] for _ in range(STEPS)]
    for _ in range(REPEATS):
        for step in range(STEPS):
            t0 = time.perf_counter()
            full_sess.run(None, full_feeds)
            full_ms[step].append((time.perf_counter() - t0) * 1e3)
        past = tuple(t.numpy() for t in cache)
        current = prefix.clone()
        for step in range(STEPS):
            feeds = graph_feeds(kv_sess, hidden, current[:, -1:], current.shape[1] - 1, past)
            t0 = time.perf_counter()
            out = kv_sess.run(None, feeds)
            kv_ms[step].append((time.perf_counter() - t0) * 1e3)
            past = tuple(out[1 + i] for i in range(4))
            current = torch.cat(
                [current, torch.tensor([[int(np.argmax(out[0][0, -1]))]], dtype=torch.long)], dim=1
            )

    full_med = [float(np.median(v)) for v in full_ms]
    kv_med = [float(np.median(v)) for v in kv_ms]
    print(f"median of {REPEATS} runs per step")
    print("step  full[21 tok] ms  kv[1 tok + past] ms  past_len")
    for i, (a, b) in enumerate(zip(full_med, kv_med)):
        print(f"{i + 1:4d}  {a:15.1f}  {b:18.1f}  {PREFIX_LEN - 1 + i:8d}")
    print(f"mean  {np.mean(full_med):15.1f}  {np.mean(kv_med):18.1f}")
    print(f"speedup {np.mean(full_med) / np.mean(kv_med):.2f}x")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stages", nargs="*", default=["all"],
                        choices=["check", "export", "verify", "bench", "all"])
    parser.add_argument("--force", action="store_true",
                        help="re-export even when fl_decoder_kv.onnx already exists")
    args = parser.parse_args()
    stages = ["check", "export", "verify", "bench"] if "all" in args.stages else args.stages
    for stage in stages:
        print(f"===== {stage} =====")
        {"check": stage_check,
         "export": lambda: stage_export(force=args.force),
         "verify": stage_verify,
         "bench": stage_bench}[stage]()


if __name__ == "__main__":
    main()
