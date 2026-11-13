# Loamz Data documentation

For the people building Loamz Data.

## Start here

| you are | read |
|---|---|
| new to Loamz Data | [product overview](product/overview.md), then the [architecture overview](architecture/overview.md) |
| building a feature | its line in [requirements](product/requirements.md), the [design](design/) of its part, its task in a [proposal](proposals/), then [contributing](../CONTRIBUTING.md) |
| reviewing security | the [rules](architecture/overview.md#6-rules-every-change-keeps), [governance](design/governance.md), [storage](design/storage.md), then [platform](architecture/platform.md) (secrets, isolation, revocation) |
| planning | the [roadmap](product/roadmap.md), then the [proposals](proposals/) |

## Everything

| folder | answers | read |
|---|---|---|
| [product/](product/) | what it is and why | [overview](product/overview.md) · [requirements](product/requirements.md) · [roadmap](product/roadmap.md) |
| [architecture/](architecture/) | how the parts fit | [overview](architecture/overview.md) · [platform](architecture/platform.md) · [decisions](architecture/decisions.md) |
| [design/](design/) | how each part works | [governance](design/governance.md) · [data](design/data.md) · [compute](design/compute.md) · [storage](design/storage.md) · [life of a query](design/life-of-a-query.md) |
| [proposals/](proposals/) | what we agreed to build next | [0001 fast data plane](proposals/0001-fast-data-plane.md) · [0002 foundation](proposals/0002-foundation.md) |

Also: [contributing](../CONTRIBUTING.md) (set up, test, code style) and the [API](../specs/data/v1/openapi.yaml) (OpenAPI).

| word | means |
|---|---|
| requirements | what the product must do, each item built or planned |
| architecture | how the parts fit (high-level design) |
| design | how one part works inside (low-level design) |
| proposal | a design agreed before it is built, with its tasks |
| decision | one line: what we chose and why |

Pictures and diagrams show the target design. A part that is not built yet says *planned*; the [requirements](product/requirements.md)
say what is built.
