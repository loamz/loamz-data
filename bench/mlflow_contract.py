#!/usr/bin/env python3
"""Records the requests a pinned `mlflow` client makes to a tracking server, for proposal 0008's gateway allow-list.

It starts a stock MLflow server, puts a small recording proxy in front of it, and runs the operations the beta must support with the
standard client: experiments, runs, artifacts, the model registry, traces, assessments and prompts, and a GenAI evaluation. It writes
every distinct method and path the client used (and which operation caused it) to `docs/design/mlflow-contract.md`.

  ~/.cache/loamz/mlflow-venv/bin/python bench/mlflow_contract.py
"""
import http.client
import http.server
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
UPSTREAM, PROXY = 5101, 5102
SEEN: dict[tuple[str, str], dict] = {}
STEP = ["setup"]


def shape(path: str) -> str:
    """The path with ids made generic, so the same call counts once."""
    path = re.sub(r"[0-9a-f]{32}", "{id}", path)
    path = re.sub(r"/tr-[0-9a-f]+", "/{trace_id}", path)
    path = re.sub(r"/(\d+)(?=/|$)", "/{n}", path)
    return path


class Recorder(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def handle_any(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        url = urllib.parse.urlsplit(self.path)
        key = (self.command, shape(url.path))
        entry = SEEN.setdefault(key, {"steps": set(), "count": 0, "keys": set(), "query": set(), "auth": False})
        entry["steps"].add(STEP[0])
        entry["count"] += 1
        entry["auth"] |= "Bearer tok" in (self.headers.get("Authorization") or "")
        entry["query"] |= {k for k, _ in urllib.parse.parse_qsl(url.query)}
        if body and "json" in (self.headers.get("Content-Type") or ""):
            try:
                parsed = json.loads(body)
                if isinstance(parsed, dict):
                    entry["keys"] |= set(parsed)
            except ValueError:
                pass
        conn = http.client.HTTPConnection("127.0.0.1", UPSTREAM, timeout=120)
        headers = {k: v for k, v in self.headers.items() if k.lower() not in ("host", "connection")}
        conn.request(self.command, self.path, body=body or None, headers=headers)
        r = conn.getresponse()
        data = r.read()
        self.send_response(r.status)
        for k, v in r.getheaders():
            if k.lower() not in ("transfer-encoding", "connection", "content-length"):
                self.send_header(k, v)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = handle_any


def scenario(tmp: str) -> None:
    import mlflow
    from mlflow import MlflowClient

    os.environ["MLFLOW_TRACKING_TOKEN"] = "tok"
    mlflow.set_tracking_uri(f"http://127.0.0.1:{PROXY}")
    client = MlflowClient()
    open(f"{tmp}/note.txt", "w").write("hello")

    def step(name, fn):
        STEP[0] = name
        try:
            fn()
        except Exception as e:  # keep going: what the client asked for before failing still counts
            print(f"  [{name}] {type(e).__name__}: {str(e)[:120]}")

    def experiments():
        exp = mlflow.set_experiment("contract-exp")
        client.get_experiment(exp.experiment_id)
        client.get_experiment_by_name("contract-exp")
        client.search_experiments()
        client.set_experiment_tag(exp.experiment_id, "team", "x")
        client.delete_experiment_tag(exp.experiment_id, "team")
        client.rename_experiment(exp.experiment_id, "contract-exp-2")
        tmp_id = client.create_experiment("contract-tmp")
        client.delete_experiment(tmp_id)
        client.restore_experiment(tmp_id)

    def runs():
        with mlflow.start_run(run_name="r1") as run:
            mlflow.log_param("a", 1)
            mlflow.log_params({"b": 2})
            mlflow.log_metric("m", 0.5, step=1)
            mlflow.log_metrics({"m2": 1.0})
            mlflow.set_tag("k", "v")
            mlflow.log_artifact(f"{tmp}/note.txt")
            mlflow.log_text("text", "t/t.txt")
            mlflow.log_dict({"a": 1}, "d.json")
            import pandas as pd

            ds = mlflow.data.from_pandas(pd.DataFrame({"x": [1, 2]}), name="ds")
            mlflow.log_input(ds, context="training")
            rid = run.info.run_id
        client.get_run(rid)
        client.get_metric_history(rid, "m")
        client.list_artifacts(rid)
        client.download_artifacts(rid, "note.txt", tmp)
        mlflow.search_runs()
        client.set_terminated(rid, "FINISHED")
        client.delete_tag(rid, "k")
        client.delete_run(rid)
        client.restore_run(rid)

    def registry():
        import mlflow.pyfunc

        class M(mlflow.pyfunc.PythonModel):
            def predict(self, context, model_input, params=None):
                return model_input

        with mlflow.start_run():
            info = mlflow.pyfunc.log_model(name="model", python_model=M(), registered_model_name="contract-model")
        client.search_registered_models()
        client.get_registered_model("contract-model")
        client.update_registered_model("contract-model", description="d")
        client.set_registered_model_tag("contract-model", "t", "1")
        client.get_latest_versions("contract-model") if hasattr(client, "get_latest_versions") else None
        mv = client.get_model_version("contract-model", "1")
        client.search_model_versions("name='contract-model'")
        client.set_registered_model_alias("contract-model", "champion", "1")
        client.get_model_version_by_alias("contract-model", "champion")
        client.set_model_version_tag("contract-model", "1", "k", "v")
        client.get_model_version_download_uri("contract-model", "1")
        mlflow.pyfunc.load_model("models:/contract-model@champion")
        client.delete_registered_model_alias("contract-model", "champion")
        client.rename_registered_model("contract-model", "contract-model-2")
        client.delete_model_version("contract-model-2", "1")
        client.delete_registered_model("contract-model-2")
        _ = (info, mv)

    def traces():
        mlflow.set_experiment("contract-traces")

        @mlflow.trace
        def work(x):
            with mlflow.start_span("inner") as s:
                s.set_inputs({"x": x})
                s.set_outputs({"y": x})
            return x

        work(1)
        mlflow.flush_trace_async_logging() if hasattr(mlflow, "flush_trace_async_logging") else None
        found = mlflow.search_traces(return_type="list")
        if found:
            t = found[0]
            tid = t.info.trace_id
            client.get_trace(tid)
            client.set_trace_tag(tid, "k", "v")
            client.delete_trace_tag(tid, "k")
            mlflow.log_feedback(trace_id=tid, name="good", value=True)
            mlflow.log_expectation(trace_id=tid, name="answer", value="1")
            client.search_traces(locations=[mlflow.get_experiment_by_name("contract-traces").experiment_id])
            client.delete_traces(experiment_id=mlflow.get_experiment_by_name("contract-traces").experiment_id, trace_ids=[tid])

    def prompts():
        from mlflow import genai

        genai.register_prompt(name="contract-prompt", template="Hello {{name}}", commit_message="first")
        genai.register_prompt(name="contract-prompt", template="Hi {{name}}", commit_message="second")
        genai.load_prompt("prompts:/contract-prompt/1")
        genai.set_prompt_alias("contract-prompt", alias="prod", version=2)
        genai.load_prompt("prompts:/contract-prompt@prod")
        genai.search_prompts()
        genai.delete_prompt_alias("contract-prompt", "prod")
        genai.delete_prompt_version("contract-prompt", "1")
        genai.delete_prompt("contract-prompt")

    def evaluation():
        from mlflow.genai import scorer

        mlflow.set_experiment("contract-eval")

        @scorer
        def exact(outputs, expectations):
            return outputs == expectations["answer"]

        data = [{"inputs": {"q": "a"}, "expectations": {"answer": "a"}}]
        mlflow.genai.evaluate(data=data, predict_fn=lambda q: q, scorers=[exact])

    for name, fn in [("experiments", experiments), ("runs", runs), ("registry", registry), ("traces", traces), ("prompts", prompts), ("evaluation", evaluation)]:
        step(name, fn)


def main() -> int:
    tmp = tempfile.mkdtemp(prefix="loamz-mlflow-")
    py = sys.executable
    env = {**os.environ, "MLFLOW_DISABLE_AGENT_HINT": "1"}
    server = subprocess.Popen(
        [py, "-m", "mlflow", "server", "--host", "127.0.0.1", "--port", str(UPSTREAM), "--backend-store-uri", f"sqlite:///{tmp}/m.db",
         "--artifacts-destination", f"{tmp}/art", "--serve-artifacts", "--workers", "1"],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    proxy = http.server.ThreadingHTTPServer(("127.0.0.1", PROXY), Recorder)
    threading.Thread(target=proxy.serve_forever, daemon=True).start()
    try:
        for _ in range(120):
            try:
                http.client.HTTPConnection("127.0.0.1", UPSTREAM, timeout=2).request("GET", "/health")
                break
            except OSError:
                time.sleep(1)
        import mlflow

        print("mlflow", mlflow.__version__)
        os.environ["MLFLOW_DISABLE_AGENT_HINT"] = "1"
        scenario(tmp)
        time.sleep(2)
    finally:
        proxy.shutdown()
        server.terminate()
        server.wait()
    version = __import__("mlflow").__version__
    rows = sorted(SEEN.items(), key=lambda kv: (kv[0][1], kv[0][0]))
    out = [f"# The MLflow client's requests (pinned: mlflow {version})", "",
           "Recorded by `bench/mlflow_contract.py` from the standard client running the operations the beta supports, against a stock server. "
           "It is the starting point for the gateway's allow-list (proposal 0008): each row is one method and path the client used, the steps that "
           "caused it, and the JSON fields it sent. Re-run it when the pinned version changes and diff the result.", "",
           "| method | path | steps | json fields sent | bearer sent |", "|---|---|---|---|---|"]
    for (method, path), e in rows:
        out.append(f"| {method} | `{path}` | {', '.join(sorted(e['steps']))} | {', '.join(sorted(e['keys'])) or '-'} | {'yes' if e['auth'] else 'no'} |")
    out += ["", f"{len(rows)} distinct requests.", "", "## What this shows for the gateway", "",
            "- **A prompt is a registered model.** Registering, loading, aliasing and searching a prompt uses the `registered-models` and "
            "`model-versions` endpoints, the same ones a model uses. The gateway must tell the two apart (by the tag and the securable kind) and "
            "authorize a prompt call as a prompt.",
            "- **Saving a model also uses `logged-models` and `runs/outputs`,** not only the registry: four more endpoints that link a model to a "
            "run and an experiment, so each link is something to authorize.",
            "- **Artifact paths are made of experiment, run and model ids** (`/artifacts/<experiment>/<run>/artifacts/...`, "
            "`/artifacts/<experiment>/models/m-<id>/artifacts/...`, `/artifacts/<experiment>/traces/tr-<id>/artifacts/...`): the gateway "
            "resolves every segment to a mapped object before it touches a volume, and never lets a client choose the root.",
            "- **Traces use three doors:** the 2.0 endpoints (delete, tags, link to a run), the 3.0 endpoints (create, get, search, "
            "assessments) and OTLP at `/v1/traces`. All three need the same checks, and the OTLP one needs its own size and rate limits.",
            "- **`GET /version` and `/api/3.0/mlflow/server-info`** are asked by the client before anything else; the gateway answers them itself.",
            "- **The client sends the bearer token on every call,** so `MLFLOW_TRACKING_TOKEN` carrying a Loamz token works with no client change.",
            "- **Not in this list on purpose:** everything the client did not need (users, webhooks, gateway, deployment, jobs, UI routes). "
            "The gateway forwards only what is here once each row has its authorization rule and a test."]
    target = os.path.join(ROOT, "docs/design/mlflow-contract.md")
    open(target, "w").write("\n".join(out) + "\n")
    print(f"{len(rows)} distinct requests written to docs/design/mlflow-contract.md")
    return 0


if __name__ == "__main__":
    sys.exit(main())
