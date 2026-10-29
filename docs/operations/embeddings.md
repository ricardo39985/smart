# Private local embeddings

The Rust API calls a separate local OpenVINO process. It contains no database access,
model API keys or external embedding calls. PostgreSQL remains authoritative. The process
uses Python for the thin native-runtime boundary, with direct OpenVINO and Hugging Face
Tokenizers bindings; no PyTorch, Transformers, hosted inference or remote model code is
used. This does not introduce a second application backend.

## Pinned identity

- Model: `hotchpotch/bekko-embedding-v1-a8m`, MIT.
- Immutable revision: `c721113d59a1d91b447450324f51c4b3332c924a`.
- Recommended CPU artifacts: `openvino/openvino_model.xml` and `.bin`. Only the static
  embedding table is int8; the transformer uses fp32. The experimental transformer-weight
  qint8 files are deliberately not selected.
- Tokenizer: `tokenizer.json` at the same revision, no query/document prefixes.
- Output: masked mean pooling, L2-normalized 384-dimensional vectors.
- Runtime pins: [requirements](../../embedder/requirements.txt). These are direct dependency
  pins, not a platform-specific transitive wheel lock; create/review that lock and demonstrate
  target Debian compatibility before production installation.

The explicit downloader checks the repository revision, each ordinary file's Git blob ID
or large file's upstream SHA256 and the independently recorded tokenizer/weights SHA256.
It records all resulting SHA256 values in the local model manifest. Startup checks every
file again, offline. XML's content hash is resolved from the immutable revision metadata
at installation, not invented in this repository. No download happens during inference.

## Install and verify on a network-enabled development host

Run from the repository root with Python 3.10+ (3.11/3.12 preferred until target verification):

```sh
python3 -m venv .work/embedder-venv
.work/embedder-venv/bin/python -m pip install -r embedder/requirements.txt
.work/embedder-venv/bin/python embedder/download.py .work/bekko-a8m
.work/embedder-venv/bin/python embedder/serve.py .work/bekko-a8m --threads 2
```

In another terminal:

```sh
.work/embedder-venv/bin/python embedder/evaluate.py
```

Configure the API's `EMBEDDING_URL=http://127.0.0.1:8788` only after the probe succeeds.
The checked-in benchmark uses real HTTP and real model inference: paraphrases, names,
returning to a prior topic, short follow-ups with nearby context, dimensions, norm,
batch limit and malformed request rejection. It reports warm latency and recall on six
small synthetic cases; this is a smoke gate, not a production capacity guarantee or broad
accuracy benchmark. Bare ambiguous pronouns cannot uniquely identify a topic; retrieval
must include nearby conversation, and the model should ask when evidence remains unclear.

## Resource and privacy boundaries

Listen address is fixed to literal loopback. Default port is 8788. Run as a dedicated
unprivileged user, do not proxy this endpoint through Caddy, and block external networking
for the runtime service after model installation. No request text, vectors or raw exceptions
are logged. Health reports only the model revision. Models contain public weights, not
user state; embeddings and memory text persist only in PostgreSQL through the main API.

The service admits at most two connections and uses an OS backlog of four. OpenVINO has
one execution stream and 1–4 configured CPU threads (default two). It accepts at most 16
texts, each 4,000 UTF-8 bytes, 1,024 tokens/text and 8,192 padded tokens per batch. Oversize
inputs are rejected, never silently truncated. Connections have a three-second socket
timeout. The Rust client has a two-second total request timeout, two in-flight requests,
128-KiB response ceiling and validates the revision, count, dimensions, finite numbers and
norm. Its dedicated HTTP client disables proxies and redirects so private text cannot be
forwarded off-host by environment proxy settings. Saturation/unavailability fails promptly for the context layer's keyword fallback;
it never silently submits to a paid embedding endpoint.

Cold model compilation occurs before readiness. The Rust timeout drops the request but
cannot interrupt already running native inference; the service's admission and token bounds
limit that work. Run chat and the embedder with measured CPU/memory quotas before launch;
do not assume the model card's benchmark hardware represents the intended Lightsail host.

## Verification in this implementation session

Passed: Python syntax compilation; pinned artifact identity, pooling, input/output names,
padding token and selected runtime releases inspected against primary sources.

Blocked: actual inference, quality/latency evaluation and dependency installation. This
execution environment has no DNS resolution for Hugging Face and no installed OpenVINO,
NumPy or Tokenizers; an isolated `/tmp` venv install returned no available distribution.
No fallback model or fabricated embedding evidence is used. Run the commands above to
complete the real-model acceptance gate before enabling semantic retrieval in deployment.

Sources: [pinned model revision](https://huggingface.co/hotchpotch/bekko-embedding-v1-a8m/commit/c721113d59a1d91b447450324f51c4b3332c924a),
[model card](https://huggingface.co/hotchpotch/bekko-embedding-v1-a8m),
[OpenVINO IR](https://huggingface.co/hotchpotch/bekko-embedding-v1-a8m/blob/main/openvino/openvino_model.xml),
[weights checksum](https://huggingface.co/hotchpotch/bekko-embedding-v1-a8m/blob/main/openvino/openvino_model.bin),
[tokenizer checksum](https://huggingface.co/hotchpotch/bekko-embedding-v1-a8m/blob/main/tokenizer.json),
[OpenVINO](https://pypi.org/project/openvino/2026.4.1/),
[Tokenizers](https://pypi.org/project/tokenizers/0.22.2/),
[NumPy](https://pypi.org/project/numpy/2.2.6/).
