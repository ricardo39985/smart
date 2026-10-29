"""Immutable model identity and offline, integrity-checked local inference."""
import hashlib
import json
from pathlib import Path

REPOSITORY = "hotchpotch/bekko-embedding-v1-a8m"
REVISION = "c721113d59a1d91b447450324f51c4b3332c924a"
FILES = ("tokenizer.json", "openvino/openvino_model.xml", "openvino/openvino_model.bin")
KNOWN_SHA256 = {
    "tokenizer.json": "8bd47075711f75a143d1b78e01a41cc65c1c591b00d3cfeffc23db07adce1392",
    "openvino/openvino_model.bin": "03330e53ab0dd085ff7e24e0dceb9669177f06414657b76276c130eb9e40d53a",
}


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


class Encoder:
    def __init__(self, directory, threads=2):
        # Imports are intentionally local: downloader needs only Python's standard library.
        import numpy as np
        import openvino as ov
        from tokenizers import Tokenizer

        self.np = np
        directory = Path(directory)
        manifest = json.loads((directory / "manifest.json").read_text())
        if manifest["revision"] != REVISION or set(manifest["sha256"]) != set(FILES):
            raise ValueError("wrong model manifest")
        for name in FILES:
            expected = KNOWN_SHA256.get(name, manifest["sha256"][name])
            if digest(directory / name) != expected or manifest["sha256"][name] != expected:
                raise ValueError("model integrity check failed")
        self.tokenizer = Tokenizer.from_file(str(directory / "tokenizer.json"))
        self.tokenizer.no_truncation()
        pad_id = self.tokenizer.token_to_id("<pad>")
        if pad_id is None:
            raise ValueError("missing padding token")
        self.tokenizer.enable_padding(pad_id=pad_id, pad_token="<pad>", direction="right")
        self.compiled = ov.Core().compile_model(
            str(directory / "openvino/openvino_model.xml"),
            "CPU",
            {"INFERENCE_NUM_THREADS": threads, "NUM_STREAMS": 1, "INFERENCE_PRECISION_HINT": "f32"},
        )
        if {port.any_name for port in self.compiled.inputs} != {"input_ids", "attention_mask"}:
            raise ValueError("unexpected model inputs")

    def encode(self, texts):
        if (not isinstance(texts, list) or not 1 <= len(texts) <= 16
                or any(not isinstance(s, str) or not s.strip() or len(s.encode()) > 4000 for s in texts)):
            raise ValueError("embedding input limit")
        encoded = self.tokenizer.encode_batch(texts, add_special_tokens=True)
        width = len(encoded[0].ids)
        # Reject rather than silently truncate memory. The caller can use keyword retrieval.
        if width > 1024 or width * len(texts) > 8192:
            raise ValueError("embedding token limit")
        np = self.np
        mask = np.asarray([x.attention_mask for x in encoded], dtype=np.int64)
        ids = np.asarray([x.ids for x in encoded], dtype=np.int64)
        result = self.compiled.create_infer_request().infer({"input_ids": ids, "attention_mask": mask})
        hidden = result[self.compiled.output("last_hidden_state")]
        if hidden.shape != (len(texts), width, 384):
            raise ValueError("unexpected model output")
        weights = mask[:, :, None].astype(np.float32)
        pooled = (hidden * weights).sum(axis=1) / weights.sum(axis=1).clip(min=1)
        norms = np.linalg.norm(pooled, axis=1, keepdims=True)
        if not np.isfinite(pooled).all() or (norms < 1e-8).any():
            raise ValueError("invalid model output")
        # Bound JSON size below the Rust client's 128-KiB ceiling even for 16 vectors.
        return [[round(float(x), 8) for x in row] for row in pooled / norms]
