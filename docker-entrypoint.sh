#!/bin/sh
set -e

# Translate WEBUI=1 or ADMIN=1 into the --webui CLI flag so Docker users
# can enable the admin UI via environment variables without overriding CMD.
if [ "${WEBUI:-0}" = "1" ] || [ "${ADMIN:-0}" = "1" ]; then
    set -- --webui "$@"
fi

# The binary enables the admin UI by default on a bare (no-arg) launch. In a
# container that has no CMD, a plain `docker run` would therefore start an
# (unreachable, loopback-only) admin server on every run. Keep admin opt-in in
# Docker as before: unless it was explicitly requested via WEBUI/ADMIN or a
# --webui/--admin flag, and unless the operator already set DISABLE_ADMIN, force
# it off. Operators still enable it with WEBUI=1 (+ ADMIN_BIND=0.0.0.0).
case " $* " in
    *" --webui "* | *" --admin "*) ;; # explicit request -> leave admin on
    *)
        if [ -z "${DISABLE_ADMIN:-}" ]; then
            export DISABLE_ADMIN=1
        fi
        ;;
esac

exec /usr/local/bin/anyllm-proxy "$@"
