locals {
  buckets = {
    warehouse = "${var.project_id}-${var.name}-warehouse"
    staging   = "${var.project_id}-${var.name}-staging"
    volumes   = "${var.project_id}-${var.name}-volumes"
    agents    = "${var.project_id}-${var.name}-agent-files"
  }
}

resource "google_storage_bucket" "this" {
  for_each                    = local.buckets
  name                        = each.value
  project                     = var.project_id
  location                    = var.region
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = var.bucket_force_destroy
  labels                      = merge(local.labels, { loamz-role = each.key })

  dynamic "encryption" {
    for_each = var.kms_key_name != "" ? [1] : []
    content {
      default_kms_key_name = var.kms_key_name
    }
  }

  depends_on = [google_kms_crypto_key_iam_member.storage]

  dynamic "versioning" {
    for_each = each.key == "volumes" ? [1] : []
    content {
      enabled = true
    }
  }

  dynamic "lifecycle_rule" {
    for_each = each.key == "staging" ? [1] : []
    content {
      condition {
        age = 7
      }
      action {
        type = "Delete"
      }
    }
  }

  dynamic "cors" {
    for_each = each.key == "volumes" ? [1] : []
    content {
      origin          = var.browser_origins
      method          = ["GET", "PUT", "POST", "HEAD"]
      response_header = ["ETag", "Content-Type"]
      max_age_seconds = 3600
    }
  }
}

# With a customer-managed key, Cloud Storage and Cloud SQL use their own service agents to reach it.
data "google_storage_project_service_account" "this" {
  project = var.project_id
}

resource "google_kms_crypto_key_iam_member" "storage" {
  count         = var.kms_key_name != "" ? 1 : 0
  crypto_key_id = var.kms_key_name
  role          = "roles/cloudkms.cryptoKeyEncrypterDecrypter"
  member        = "serviceAccount:${data.google_storage_project_service_account.this.email_address}"
}

resource "google_project_service_identity" "sql" {
  count    = var.kms_key_name != "" ? 1 : 0
  provider = google-beta
  project  = var.project_id
  service  = "sqladmin.googleapis.com"
}

resource "google_kms_crypto_key_iam_member" "sql" {
  count         = var.kms_key_name != "" ? 1 : 0
  crypto_key_id = var.kms_key_name
  role          = "roles/cloudkms.cryptoKeyEncrypterDecrypter"
  member        = "serviceAccount:${google_project_service_identity.sql[0].email}"
}
