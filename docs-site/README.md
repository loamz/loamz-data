# Loamz Data Documentation Site

User documentation for Loamz Data, hosted at `https://data.loamz.com/docs`.

## Building the Documentation

```bash
# 1. Install dependencies
pip install -r requirements.txt

# 2. Generate and verify API reference pages from openapi.yaml
python3 gen_api.py
python3 check_api.py

# 3. Build static site (outputs to docs-site/site/)
mkdocs build --strict
```

To run a live preview development server:

```bash
mkdocs serve
```
