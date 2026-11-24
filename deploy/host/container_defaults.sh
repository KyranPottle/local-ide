# shellcheck shell=bash disable=SC2034
# Example site values. Override each value from the environment.

: "${PROXMOX_SSH_HOST:=build-host}"
: "${TAILNET_HOST:=ide.example.ts.net}"
: "${INFERENCE_CT_ID:=200}"
: "${INFERENCE_KEY_FILE:=/etc/llama-swap/key.env}"
: "${INFERENCE_BASE_URL:=http://192.0.2.10:8080/v1}"
: "${KEY_VARIABLE:=LLAMA_SWAP_KEY}"
: "${CT_ID:=201}"
: "${CT_HOSTNAME:=local-ide}"
: "${CT_ADDRESS:=192.0.2.11}"
: "${CT_PREFIX:=24}"
: "${CT_BRIDGE:=vmbr0}"
: "${CT_GATEWAY:=192.0.2.1}"
: "${CT_NAMESERVER:=1.1.1.1}"
: "${CT_CORES:=2}"
: "${CT_MEMORY_MIB:=256}"
: "${CT_SWAP_MIB:=0}"
: "${CT_DISK_GIB:=2}"
# Empty means the storage of the inference container root volume
: "${STORAGE_ID:=}"
: "${TEMPLATE_STORAGE:=local}"
# Empty means the newest debian-13-standard template in the pveam index, else the newest one on TEMPLATE_STORAGE
: "${TEMPLATE:=}"
: "${ENGINE_PORT:=7878}"
: "${SERVE_PORT:=8443}"
: "${DEPLOY_STATE_DIR:=/var/lib/local-ide-deploy}"
# Host-local, so deploy_to_container.sh does not send it
: "${PVE_CONFIG_DIR:=/etc/pve}"

CONTAINER_VARIABLES=(
  PROXMOX_SSH_HOST TAILNET_HOST INFERENCE_CT_ID INFERENCE_KEY_FILE INFERENCE_BASE_URL KEY_VARIABLE
  CT_ID CT_HOSTNAME CT_ADDRESS CT_PREFIX CT_BRIDGE CT_GATEWAY CT_NAMESERVER
  CT_CORES CT_MEMORY_MIB CT_SWAP_MIB CT_DISK_GIB STORAGE_ID TEMPLATE_STORAGE TEMPLATE
  ENGINE_PORT SERVE_PORT DEPLOY_STATE_DIR
)

# Fixed by local-ide.service, so these values are not overridable
GUEST_INSTALL_DIR=/opt/local-ide
GUEST_CONFIG_DIR=/etc/local-ide
GUEST_DATA_DIR=/var/lib/local-ide
GUEST_USER=local-ide

PUBLIC_URL="https://${TAILNET_HOST}:${SERVE_PORT}/"
ENGINE_URL="http://${CT_ADDRESS}:${ENGINE_PORT}"
