# The MLflow client's requests (pinned: mlflow 3.18.0)

Recorded by `bench/mlflow_contract.py` from the standard client running the operations the beta supports, against a stock server. It is the starting point for the gateway's allow-list (proposal 0008): each row is one method and path the client used, the steps that caused it, and the JSON fields it sent. Re-run it when the pinned version changes and diff the result.

| method | path | steps | json fields sent | bearer sent |
|---|---|---|---|---|
| GET | `/api/2.0/mlflow-artifacts/artifacts` | registry, runs | - | yes |
| GET | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/MLmodel` | registry | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/MLmodel` | registry | - | yes |
| GET | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/conda.yaml` | registry | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/conda.yaml` | registry | - | yes |
| GET | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/python_env.yaml` | registry | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/python_env.yaml` | registry | - | yes |
| GET | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/python_model.pkl` | registry | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/python_model.pkl` | registry | - | yes |
| GET | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/requirements.txt` | registry | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/models/m-{id}/artifacts/requirements.txt` | registry | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/traces/tr-{id}/artifacts/traces.json` | traces | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/{id}/artifacts/d.json` | runs | - | yes |
| GET | `/api/2.0/mlflow-artifacts/artifacts/{n}/{id}/artifacts/note.txt` | runs | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/{id}/artifacts/note.txt` | runs | - | yes |
| PUT | `/api/2.0/mlflow-artifacts/artifacts/{n}/{id}/artifacts/t/t.txt` | runs | - | yes |
| POST | `/api/2.0/mlflow/experiments/create` | evaluation, experiments, traces | name | yes |
| POST | `/api/2.0/mlflow/experiments/delete` | experiments | experiment_id | yes |
| POST | `/api/2.0/mlflow/experiments/delete-experiment-tag` | experiments | experiment_id, key | yes |
| GET | `/api/2.0/mlflow/experiments/get` | evaluation, experiments, traces | - | yes |
| GET | `/api/2.0/mlflow/experiments/get-by-name` | evaluation, experiments, traces | - | yes |
| POST | `/api/2.0/mlflow/experiments/restore` | experiments | experiment_id | yes |
| POST | `/api/2.0/mlflow/experiments/search` | experiments | max_results, view_type | yes |
| POST | `/api/2.0/mlflow/experiments/set-experiment-tag` | experiments | experiment_id, key, value | yes |
| POST | `/api/2.0/mlflow/experiments/update` | experiments | experiment_id, new_name | yes |
| POST | `/api/2.0/mlflow/logged-models` | registry | experiment_id, name, source_run_id, tags | yes |
| GET | `/api/2.0/mlflow/logged-models/m-{id}` | registry | - | yes |
| PATCH | `/api/2.0/mlflow/logged-models/m-{id}` | registry | model_id, status | yes |
| PATCH | `/api/2.0/mlflow/logged-models/m-{id}/tags` | registry | tags | yes |
| POST | `/api/2.0/mlflow/logged-models/search` | runs | experiment_ids, filter | yes |
| GET | `/api/2.0/mlflow/metrics/get-history` | runs | - | yes |
| POST | `/api/2.0/mlflow/model-versions/create` | prompts, registry | description, model_id, name, source, tags | yes |
| DELETE | `/api/2.0/mlflow/model-versions/delete` | registry | name, version | yes |
| GET | `/api/2.0/mlflow/model-versions/get` | prompts, registry | - | yes |
| GET | `/api/2.0/mlflow/model-versions/get-download-uri` | registry | - | yes |
| GET | `/api/2.0/mlflow/model-versions/search` | registry | - | yes |
| POST | `/api/2.0/mlflow/model-versions/set-tag` | registry | key, name, value, version | yes |
| DELETE | `/api/2.0/mlflow/registered-models/alias` | prompts, registry | alias, name | yes |
| GET | `/api/2.0/mlflow/registered-models/alias` | prompts, registry | - | yes |
| POST | `/api/2.0/mlflow/registered-models/alias` | prompts, registry | alias, name, version | yes |
| POST | `/api/2.0/mlflow/registered-models/create` | prompts, registry | description, name, tags | yes |
| DELETE | `/api/2.0/mlflow/registered-models/delete` | registry | name | yes |
| GET | `/api/2.0/mlflow/registered-models/get` | prompts, registry | - | yes |
| POST | `/api/2.0/mlflow/registered-models/get-latest-versions` | registry | name | yes |
| POST | `/api/2.0/mlflow/registered-models/rename` | registry | name, new_name | yes |
| GET | `/api/2.0/mlflow/registered-models/search` | prompts, registry | - | yes |
| POST | `/api/2.0/mlflow/registered-models/set-tag` | prompts, registry | key, name, value | yes |
| PATCH | `/api/2.0/mlflow/registered-models/update` | registry | description, name | yes |
| POST | `/api/2.0/mlflow/runs/create` | evaluation, registry, runs | experiment_id, run_name, start_time, tags, user_id | yes |
| POST | `/api/2.0/mlflow/runs/delete` | runs | run_id | yes |
| POST | `/api/2.0/mlflow/runs/delete-tag` | runs | key, run_id | yes |
| GET | `/api/2.0/mlflow/runs/get` | evaluation, registry, runs | - | yes |
| POST | `/api/2.0/mlflow/runs/log-batch` | evaluation, runs | metrics, params, run_id, tags | yes |
| POST | `/api/2.0/mlflow/runs/log-inputs` | evaluation, runs | datasets, run_id | yes |
| POST | `/api/2.0/mlflow/runs/log-metric` | runs | key, run_id, run_uuid, step, timestamp, value | yes |
| POST | `/api/2.0/mlflow/runs/log-parameter` | runs | key, run_id, run_uuid, value | yes |
| POST | `/api/2.0/mlflow/runs/outputs` | registry | models, run_id | yes |
| POST | `/api/2.0/mlflow/runs/restore` | runs | run_id | yes |
| POST | `/api/2.0/mlflow/runs/search` | runs | experiment_ids, filter, max_results, run_view_type | yes |
| POST | `/api/2.0/mlflow/runs/set-tag` | evaluation, runs | key, run_id, run_uuid, value | yes |
| POST | `/api/2.0/mlflow/runs/update` | evaluation, registry, runs | end_time, run_id, run_uuid, status | yes |
| POST | `/api/2.0/mlflow/traces/delete-traces` | traces | experiment_id, request_ids | yes |
| POST | `/api/2.0/mlflow/traces/link-to-run` | evaluation | run_id, trace_ids | yes |
| DELETE | `/api/2.0/mlflow/traces/tr-{id}/tags` | traces | key | yes |
| PATCH | `/api/2.0/mlflow/traces/tr-{id}/tags` | traces | key, value | yes |
| GET | `/api/3.0/mlflow/server-info` | runs | - | yes |
| POST | `/api/3.0/mlflow/traces` | evaluation, traces | trace | yes |
| GET | `/api/3.0/mlflow/traces/batchGet` | traces | - | yes |
| GET | `/api/3.0/mlflow/traces/get` | evaluation, traces | - | yes |
| POST | `/api/3.0/mlflow/traces/search` | evaluation, traces | filter, locations, max_results | yes |
| GET | `/api/3.0/mlflow/traces/tr-{id}` | evaluation, traces | - | yes |
| POST | `/api/3.0/mlflow/traces/tr-{id}/assessments` | evaluation, traces | assessment | yes |
| POST | `/v1/traces` | evaluation, traces | - | yes |
| GET | `/version` | traces | - | yes |

74 distinct requests.

## What this shows for the gateway

- **A prompt is a registered model.** Registering, loading, aliasing and searching a prompt uses the `registered-models` and `model-versions` endpoints, the same ones a model uses. The gateway must tell the two apart (by the tag and the securable kind) and authorize a prompt call as a prompt.
- **Saving a model also uses `logged-models` and `runs/outputs`,** not only the registry: four more endpoints that link a model to a run and an experiment, so each link is something to authorize.
- **Artifact paths are made of experiment, run and model ids** (`/artifacts/<experiment>/<run>/artifacts/...`, `/artifacts/<experiment>/models/m-<id>/artifacts/...`, `/artifacts/<experiment>/traces/tr-<id>/artifacts/...`): the gateway resolves every segment to a mapped object before it touches a volume, and never lets a client choose the root.
- **Traces use three doors:** the 2.0 endpoints (delete, tags, link to a run), the 3.0 endpoints (create, get, search, assessments) and OTLP at `/v1/traces`. All three need the same checks, and the OTLP one needs its own size and rate limits.
- **`GET /version` and `/api/3.0/mlflow/server-info`** are asked by the client before anything else; the gateway answers them itself.
- **The client sends the bearer token on every call,** so `MLFLOW_TRACKING_TOKEN` carrying a Loamz token works with no client change.
- **Not in this list on purpose:** everything the client did not need (users, webhooks, gateway, deployment, jobs, UI routes). The gateway forwards only what is here once each row has its authorization rule and a test.
