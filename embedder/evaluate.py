"""Real-model HTTP acceptance and warm latency probe; fails without the actual service."""
import argparse
import json
import math
import statistics
import time
import urllib.error
import urllib.parse
import urllib.request

from model import REVISION

DOCUMENTS = [
    "Rick is allergic to peanuts and avoids food containing them.",
    "Rick's Lisbon trip is booked for June 12. He prefers a quiet hotel near the station.",
    "Anita's birthday is September 29.",
    "The backend uses Rust and PostgreSQL for durable application state.",
    "Rick likes concise answers with practical steps.",
]
CASES = [
    ("Which foods should I avoid because of my allergy?", 0),
    ("Back to my Portugal holiday: what kind of accommodation did I want?", 1),
    ("When is Anita's birthday?", 2),
    ("What database stores the app's information?", 3),
    ("How do I prefer you to respond?", 4),
    # Nearby conversation resolves the intentionally vague follow-up.
    ("We were arranging my Lisbon holiday. What were those dates again?", 1),
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://127.0.0.1:8788")
    args = parser.parse_args()
    url = urllib.parse.urlsplit(args.url)
    if url.scheme != "http" or url.hostname not in {"127.0.0.1", "::1"}:
        parser.error("literal loopback HTTP required")

    def request(texts):
        data = json.dumps({"texts": texts}).encode()
        req = urllib.request.Request(args.url.rstrip("/") + "/embed", data, {"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=2) as response:
            body = response.read(131073)
        assert len(body) <= 131072
        result = json.loads(body)
        assert result["model_revision"] == REVISION
        vectors = result["embeddings"]
        assert len(vectors) == len(texts)
        for vector in vectors:
            assert len(vector) == 384 and all(math.isfinite(x) for x in vector)
            assert abs(sum(x*x for x in vector) - 1) < 0.01
        return vectors

    documents = request(DOCUMENTS)
    timings = []
    outcomes = []
    for query, expected in CASES:
        start = time.monotonic()
        vector = request([query])[0]
        timings.append((time.monotonic() - start) * 1000)
        scores = [sum(a*b for a, b in zip(vector, doc)) for doc in documents]
        found = max(range(len(scores)), key=scores.__getitem__)
        outcomes.append(found == expected)
    # Maximum supported batch and malformed input rejection are public protocol contracts.
    request(["A short memory."] * 16)
    for texts in ([], ["x"] * 17, ["x" * 4001], [""]):
        try:
            request(texts)
        except urllib.error.HTTPError as error:
            assert error.code == 400
        else:
            raise AssertionError("invalid input accepted")
    print(json.dumps({"model_revision": REVISION, "recall_at_1": sum(outcomes) / len(outcomes),
                      "cases": len(outcomes), "warm_median_ms": statistics.median(timings),
                      "warm_max_ms": max(timings), "protocol": "passed"}, indent=2))
    assert all(outcomes), "retrieval acceptance failed; inspect actual model and context design"


if __name__ == "__main__":
    main()
