# Loamz Data deployment

The Data cell consists of the Data API, controller and workers, Data workspace, PostgreSQL, SeaweedFS and Keycloak. The shared Keycloak realm remains in `keycloak/`.

Use `make cell` to build and start the local Compose cell after preparing its environment with `deploy/compose/gen-env.sh`. `make cell-dev` runs the workspace from the checkout. `deploy/PORTS.md` lists published and internal ports. The `ml` profile adds the optional llama-server workers.

Build `deploy/docker/loamz-data.Dockerfile`, `deploy/docker/loamz-data-workspace.Dockerfile`, or the CPU/GPU llama-server Dockerfiles from the repository root. Release workflows publish images for amd64 and arm64.

`helm/loamz-cell` contains the Data chart; `terraform/` contains cloud infrastructure templates. Review their storage, identity, networking, secrets and resource settings before use. See `keycloak/README.md` for the shared realm.
