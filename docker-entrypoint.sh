#!/bin/sh
set -e

# Translate WEBUI=1 or ADMIN=1 into the --webui CLI flag so Docker users
# can enable the admin UI via environment variables without overriding CMD.
if [ "${WEBUI:-0}" = "1" ] || [ "${ADMIN:-0}" = "1" ]; then
    set -- --webui "$@"
fi

exec /usr/local/bin/anyllm-proxy "$@"
