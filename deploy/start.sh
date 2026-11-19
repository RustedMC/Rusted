#!/bin/sh
# Starts Rusted in this terminal. Type commands here (help, stop, op <name>...).
# If the server crashes it starts again by itself; "stop" ends it for good.
cd "$(dirname "$0")"
while true; do
    ./rusted
    code=$?
    [ "$code" -eq 0 ] && break
    echo "Rusted stopped unexpectedly (code $code). Restarting in 5 seconds, press Ctrl+C to cancel."
    sleep 5
done
echo "Rusted has stopped."
