# Skeleton of the realm in ../realm-loamz.json, for reconciling a running cell. NOT applied or validated.
# Things to check against the provider version you pin: `organizations_enabled` on keycloak_realm, and the
# organization claim mapper (the provider has no typed resource for oidc-organization-membership-mapper in every
# release; keycloak_generic_protocol_mapper is used so the mapper type is passed through verbatim).
# Standard scopes (basic, profile, email, roles, web-origins, acr) already exist in a realm created by Terraform; the JSON
# template defines them only because a realm file that lists any scope suppresses the built-ins.

resource "keycloak_realm" "loamz" {
  realm                    = var.realm
  enabled                  = true
  display_name             = "Loamz"
  organizations_enabled    = true
  ssl_required             = "external"
  registration_allowed     = false
  login_with_email_allowed = true
  reset_password_allowed   = true
  revoke_refresh_token     = true
  refresh_token_max_reuse  = 0
  access_token_lifespan    = "5m"
  sso_session_idle_timeout = "30m"
  sso_session_max_lifespan = "10h"

  security_defenses {
    brute_force_detection {
      max_login_failures = 10
    }
  }
}

# ---------------------------------------------------------------- roles
resource "keycloak_role" "platform_admin" {
  realm_id    = keycloak_realm.loamz.id
  name        = "platform_admin"
  description = "Operates the cell: all tenants, provisioning and erasure. Never granted to tenant users."
}

resource "keycloak_role" "tenant_admin" {
  realm_id    = keycloak_realm.loamz.id
  name        = "tenant_admin"
  description = "Administers one organization (tenant)."
}

resource "keycloak_role" "end_user" {
  realm_id    = keycloak_realm.loamz.id
  name        = "end_user"
  description = "Uses the agents and data of the organization they belong to."
}

resource "keycloak_group" "data_admins" {
  realm_id = keycloak_realm.loamz.id
  name     = "data-admins"
}

# ---------------------------------------------------------------- resource servers (audiences)
resource "keycloak_openid_client" "loamz_agents" {
  realm_id    = keycloak_realm.loamz.id
  client_id   = "loamz-agents"
  name        = "Agent runtime API"
  enabled     = true
  access_type = "BEARER-ONLY"
}

resource "keycloak_openid_client" "loamz_data" {
  realm_id    = keycloak_realm.loamz.id
  client_id   = "loamz-data"
  name        = "Loamz Data API"
  enabled     = true
  access_type = "BEARER-ONLY"
}

locals {
  resource_servers = {
    loamz_agents = keycloak_openid_client.loamz_agents.id
    loamz_data   = keycloak_openid_client.loamz_data.id
  }
  tenant_lifecycle_roles = ["provision-tenant", "erase-tenant"]
}

resource "keycloak_role" "tenant_lifecycle" {
  for_each = {
    for pair in setproduct(keys(local.resource_servers), local.tenant_lifecycle_roles) :
    "${pair[0]}.${pair[1]}" => { server = pair[0], role = pair[1] }
  }
  realm_id  = keycloak_realm.loamz.id
  client_id = local.resource_servers[each.value.server]
  name      = each.value.role
}

# ---------------------------------------------------------------- client scopes
resource "keycloak_openid_client_scope" "agents_api" {
  realm_id               = keycloak_realm.loamz.id
  name                   = "agents-api"
  include_in_token_scope = false
}

resource "keycloak_openid_audience_protocol_mapper" "agents_api" {
  realm_id                 = keycloak_realm.loamz.id
  client_scope_id          = keycloak_openid_client_scope.agents_api.id
  name                     = "loamz-agents audience"
  included_client_audience = keycloak_openid_client.loamz_agents.client_id
}

resource "keycloak_openid_client_scope" "data_api" {
  realm_id               = keycloak_realm.loamz.id
  name                   = "data-api"
  include_in_token_scope = false
}

resource "keycloak_openid_audience_protocol_mapper" "data_api" {
  realm_id                 = keycloak_realm.loamz.id
  client_scope_id          = keycloak_openid_client_scope.data_api.id
  name                     = "loamz-data audience"
  included_client_audience = keycloak_openid_client.loamz_data.client_id
}

# The organization claim: {"<alias>": {"id": "<organization id>"}}. Clients request `organization:<alias>`.
resource "keycloak_openid_client_scope" "organization" {
  realm_id               = keycloak_realm.loamz.id
  name                   = "organization"
  include_in_token_scope = true
}

resource "keycloak_generic_protocol_mapper" "organization" {
  realm_id        = keycloak_realm.loamz.id
  client_scope_id = keycloak_openid_client_scope.organization.id
  name            = "organization"
  protocol        = "openid-connect"
  protocol_mapper = "oidc-organization-membership-mapper"
  config = {
    "claim.name"                = "organization"
    "jsonType.label"            = "String"
    "multivalued"               = "true"
    "addOrganizationId"         = "true"
    "addOrganizationAttributes" = "false"
    "id.token.claim"            = "true"
    "access.token.claim"        = "true"
    "userinfo.token.claim"      = "true"
    "introspection.token.claim" = "true"
  }
}

resource "keycloak_openid_client_scope" "groups" {
  realm_id               = keycloak_realm.loamz.id
  name                   = "groups"
  include_in_token_scope = false
}

resource "keycloak_openid_group_membership_protocol_mapper" "groups" {
  realm_id        = keycloak_realm.loamz.id
  client_scope_id = keycloak_openid_client_scope.groups.id
  name            = "groups"
  claim_name      = "groups"
  full_path       = false
}

locals {
  delegated_scopes = [
    "data:read",
    "files:read",
    "files:write",
    "documents:read",
    "documents:write",
    "memory:read",
    "memory:write",
  ]
}

resource "keycloak_openid_client_scope" "delegated" {
  for_each               = toset(local.delegated_scopes)
  realm_id               = keycloak_realm.loamz.id
  name                   = each.value
  include_in_token_scope = true
}

# ---------------------------------------------------------------- human-facing clients
resource "keycloak_openid_client" "loamz_ui" {
  realm_id                        = keycloak_realm.loamz.id
  client_id                       = "agents-console"
  name                            = "Loamz agents console"
  enabled                         = true
  access_type                     = "CONFIDENTIAL"
  client_secret                   = var.ui_client_secret
  standard_flow_enabled           = true
  direct_access_grants_enabled    = false
  pkce_code_challenge_method      = "S256"
  root_url                        = var.ui_url
  valid_redirect_uris             = ["${var.ui_url}/auth/callback"]
  web_origins                     = [var.ui_url]
  valid_post_logout_redirect_uris = ["${var.ui_url}/*"]
}

resource "keycloak_openid_client_default_scopes" "loamz_ui" {
  realm_id       = keycloak_realm.loamz.id
  client_id      = keycloak_openid_client.loamz_ui.id
  default_scopes = ["basic", "web-origins", "acr", "profile", "roles", "email", keycloak_openid_client_scope.agents_api.name, keycloak_openid_client_scope.groups.name]
}

resource "keycloak_openid_client_optional_scopes" "loamz_ui" {
  realm_id  = keycloak_realm.loamz.id
  client_id = keycloak_openid_client.loamz_ui.id
  optional_scopes = concat(
    [keycloak_openid_client_scope.organization.name, keycloak_openid_client_scope.data_api.name],
    [for s in keycloak_openid_client_scope.delegated : s.name],
  )
}

resource "keycloak_openid_client" "loamz_data_web" {
  realm_id                        = keycloak_realm.loamz.id
  client_id                       = "data-workspace"
  name                            = "Loamz Data workspace"
  enabled                         = true
  access_type                     = "CONFIDENTIAL"
  client_secret                   = var.data_web_client_secret
  standard_flow_enabled           = true
  direct_access_grants_enabled    = false
  pkce_code_challenge_method      = "S256"
  root_url                        = var.data_web_url
  valid_redirect_uris             = ["${var.data_web_url}/auth/callback"]
  web_origins                     = [var.data_web_url]
  valid_post_logout_redirect_uris = ["${var.data_web_url}/*"]
}

resource "keycloak_openid_client_default_scopes" "loamz_data_web" {
  realm_id       = keycloak_realm.loamz.id
  client_id      = keycloak_openid_client.loamz_data_web.id
  default_scopes = ["basic", "web-origins", "acr", "profile", "roles", "email", keycloak_openid_client_scope.data_api.name, keycloak_openid_client_scope.groups.name]
}

resource "keycloak_openid_client_optional_scopes" "loamz_data_web" {
  realm_id        = keycloak_realm.loamz.id
  client_id       = keycloak_openid_client.loamz_data_web.id
  optional_scopes = [keycloak_openid_client_scope.organization.name, keycloak_openid_client_scope.agents_api.name]
}

resource "keycloak_openid_client" "loamz_cli" {
  realm_id                                  = keycloak_realm.loamz.id
  client_id                                 = "loamz-cli"
  name                                      = "Loamz CLI and SDK"
  enabled                                   = true
  access_type                               = "PUBLIC"
  standard_flow_enabled                     = false
  direct_access_grants_enabled              = false
  oauth2_device_authorization_grant_enabled = true
}

resource "keycloak_openid_client_default_scopes" "loamz_cli" {
  realm_id       = keycloak_realm.loamz.id
  client_id      = keycloak_openid_client.loamz_cli.id
  default_scopes = ["basic", "web-origins", "acr", "profile", "roles", "email", keycloak_openid_client_scope.agents_api.name, keycloak_openid_client_scope.data_api.name, keycloak_openid_client_scope.groups.name]
}

resource "keycloak_openid_client_optional_scopes" "loamz_cli" {
  realm_id  = keycloak_realm.loamz.id
  client_id = keycloak_openid_client.loamz_cli.id
  optional_scopes = concat(
    [keycloak_openid_client_scope.organization.name],
    [for s in keycloak_openid_client_scope.delegated : s.name],
  )
}

# ---------------------------------------------------------------- the platform's service account
resource "keycloak_openid_client" "cell_admin" {
  realm_id                 = keycloak_realm.loamz.id
  client_id                = "cell-admin"
  name                     = "Cell administration"
  enabled                  = true
  access_type              = "CONFIDENTIAL"
  client_secret            = var.cell_admin_client_secret
  standard_flow_enabled    = false
  service_accounts_enabled = true
}

resource "keycloak_openid_client_default_scopes" "cell_admin" {
  realm_id       = keycloak_realm.loamz.id
  client_id      = keycloak_openid_client.cell_admin.id
  default_scopes = ["basic", "profile", "roles", keycloak_openid_client_scope.agents_api.name, keycloak_openid_client_scope.data_api.name]
}

resource "keycloak_openid_client_service_account_role" "cell_admin" {
  for_each                = keycloak_role.tenant_lifecycle
  realm_id                = keycloak_realm.loamz.id
  service_account_user_id = keycloak_openid_client.cell_admin.service_account_user_id
  client_id               = each.value.client_id
  role                    = each.value.name
}
