# Every request to OpenBao goes to the container's output (docker logs), so a break-glass read is on record. OpenBao takes audit devices
# from its configuration, not from its API.
audit "file" "stdout" {
  description = "every request, to the container's output"
  options {
    file_path = "stdout"
  }
}
