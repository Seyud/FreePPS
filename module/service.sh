#!/system/bin/sh

MODDIR=${0%/*}

wait_until_login() {
    # in case of /data encryption is disabled
    while [ "$(getprop sys.boot_completed)" != "1" ]; do
        sleep 1
    done

    # we doesn't have the permission to rw "/sdcard" before the user unlocks the screen
    local test_file="/sdcard/Android/.PERMISSION_TEST"
    true >"$test_file"
    while [ ! -f "$test_file" ]; do
        true >"$test_file"
        sleep 1
    done
    rm "$test_file"
}

wait_until_login

if [ -f "$MODDIR/debug" ]; then
    nohup $MODDIR/bin/FreePPS >/dev/null 2>&1 &
    FREEPPS_PID=$!
    sleep 0.2
    nohup nice -n 10 logcat -b main --pid=$FREEPPS_PID -s FreePPS:V > "$MODDIR/FreePPS.log" 2>&1 &
else
    nohup $MODDIR/bin/FreePPS >/dev/null 2>&1 &
fi
